import 'dart:async';
import 'dart:math' as math;
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/foundation.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../platform/remote_clipboard.dart';
import '../session/scancodes.dart';
import '../src/rust/api/errors.dart';
import '../src/rust/api/pins.dart';
import '../src/rust/api/session.dart';
import '../state/profiles.dart';
import 'error_banner.dart';
import 'status_chip.dart';

/// Shows a remote desktop session: connection progress, the certificate
/// prompt, the remote screen, keyboard/mouse/touch input (direct touch or
/// trackpad mode, with pinch-zoom) and a small bar of special keys.
///
/// Pops with `true` when the user asks to reconnect after re-trusting a
/// changed certificate.
class SessionScreen extends StatefulWidget {
  const SessionScreen({
    super.key,
    required this.profile,
    required this.pinsPath,
    this.password,
    this.useSavedPassword = false,
    this.idleMinutes = 30,
    this.onOutcome,
  });

  final ConnectionProfile profile;

  /// File holding trusted certificate fingerprints.
  final String pinsPath;

  /// Disconnect after this many idle minutes (0 = never).
  final int idleMinutes;

  /// Called once when the session ends, with "connected" (it was used and
  /// then closed), "cancelled" (closed before connecting) or an error id.
  final void Function(String outcome)? onOutcome;

  /// Typed (or, on Android, vault-read) password. Used once, never stored here.
  final String? password;

  /// Windows/Linux: the Rust core reads the saved password from the OS vault.
  final bool useSavedPassword;

  @override
  State<SessionScreen> createState() => _SessionScreenState();
}

class _SessionScreenState extends State<SessionScreen> with WidgetsBindingObserver {
  final RdpSession _session = RdpSession.create();
  StreamSubscription<SessionUpdate>? _sub;
  final FocusNode _focus = FocusNode(debugLabel: 'remote-desktop');

  RdpConnectionState _state = RdpConnectionState.connecting;
  String _statusLabel = 'Connecting…';
  ErrorInfo? _error;
  bool _ended = false;
  bool _wasConnected = false;

  // View: zoom and pan (pinch-zoom), and trackpad mode for touch screens.
  double _zoom = 1;
  Offset _pan = Offset.zero;
  bool _trackpad = false;
  Offset _cursor = Offset.zero; // remote coordinates, trackpad mode
  double _startZoom = 1;
  Offset _startFocal = Offset.zero;
  Offset _startOrigin = Offset.zero;
  bool _dragging = false;
  // Touch fingers currently down, and whether this touch sequence included a
  // pinch: after a pinch, the finger that stays down must not click or drag.
  int _touches = 0;
  bool _pinched = false;

  static const _touchKinds = {
    PointerDeviceKind.touch,
    PointerDeviceKind.stylus,
    PointerDeviceKind.invertedStylus,
  };

  // Remote framebuffer (RGBA) and the image decoded from it.
  int _width = 0;
  int _height = 0;
  Uint8List _pixels = Uint8List(0);
  ui.Image? _image;
  bool _decoding = false;
  bool _dirty = false;

  // Clipboard (S6).
  Timer? _clipTimer;
  String? _lastLocalClip;
  bool get _sendsClipboard =>
      widget.profile.clipboardMode == ClipboardMode.both ||
      widget.profile.clipboardMode == ClipboardMode.toRemoteOnly;
  static bool get _isAndroid => !kIsWeb && defaultTargetPlatform == TargetPlatform.android;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _start();
  }

  /// Desktop: check this device's clipboard every second while connected.
  /// Android: only when the app comes back to the foreground or on request,
  /// because Android shows a notice every time an app reads the clipboard.
  void _startClipboardSync() {
    if (!_sendsClipboard) return;
    _offerLocalClipboard();
    if (!_isAndroid) {
      _clipTimer = Timer.periodic(const Duration(seconds: 1), (_) => _offerLocalClipboard());
    }
  }

  Future<void> _offerLocalClipboard() async {
    if (_ended || !_sendsClipboard) return;
    final text = await RemoteClipboard.readText();
    if (text == null || text == _lastLocalClip || _ended) return;
    _lastLocalClip = text;
    _session.offerClipboardText(text: text);
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed && _state == RdpConnectionState.connected) {
      _offerLocalClipboard();
    }
  }

  Future<void> _start() async {
    final pinsPath = widget.pinsPath;
    if (!mounted) return;
    final view = View.of(context);
    final size = view.physicalSize;
    // Ask for a desktop the size of this window, within sensible limits.
    final w = (size.width.clamp(800, 3840) ~/ 2) * 2;
    final h = (size.height.clamp(600, 2160) ~/ 2) * 2;
    final stream = widget.useSavedPassword
        ? _session.startWithSavedPassword(
            connectionId: widget.profile.id,
            host: widget.profile.host,
            port: widget.profile.port,
            username: widget.profile.username,
            width: w,
            height: h,
            pinsPath: pinsPath,
            clipboardDirection: widget.profile.clipboardMode,
            idleMinutes: widget.idleMinutes,
          )
        : _session.start(
            host: widget.profile.host,
            port: widget.profile.port,
            username: widget.profile.username,
            password: widget.password ?? '',
            width: w,
            height: h,
            pinsPath: pinsPath,
            clipboardDirection: widget.profile.clipboardMode,
            idleMinutes: widget.idleMinutes,
          );
    _sub = stream.listen(_onUpdate, onError: (_) => _onEnded(null));
  }

  void _onUpdate(SessionUpdate u) {
    switch (u.kind) {
      case UpdateKind.status:
        setState(() {
          _statusLabel = u.label ?? '';
          _state = u.connected
              ? RdpConnectionState.connected
              : (_statusLabel.startsWith('Check')
                  ? RdpConnectionState.checkIdentity
                  : RdpConnectionState.connecting);
        });
        if (u.connected) {
          _wasConnected = true;
          _focus.requestFocus();
          _startClipboardSync();
        }
      case UpdateKind.certificateApproval:
        _askApproval(u.fingerprint ?? '');
      case UpdateKind.certificateChanged:
        setState(() => _changedFingerprints = (u.pinned ?? '', u.fingerprint ?? ''));
      case UpdateKind.resized:
        setState(() {
          _width = u.width;
          _height = u.height;
          _pixels = Uint8List(_width * _height * 4);
          _cursor = Offset(_width / 2, _height / 2);
        });
      case UpdateKind.frame:
        _applyFrame(u);
      case UpdateKind.ended:
        _onEnded(u.error);
      case UpdateKind.remoteClipboard:
        final text = u.text;
        if (text != null && text.isNotEmpty) {
          _lastLocalClip = text; // don't send it straight back
          RemoteClipboard.setText(text);
        }
      case UpdateKind.remoteClipboardTooLarge:
        if (!mounted) return;
        ScaffoldMessenger.of(context).showSnackBar(const SnackBar(
          content: Text('Text copied on the remote computer was larger than 1 MB and was not copied.'),
        ));
    }
  }

  (String, String)? _changedFingerprints;

  void _applyFrame(SessionUpdate u) {
    if (_width == 0 || _height == 0) return;
    final rowBytes = u.width * 4;
    for (var row = 0; row < u.height; row++) {
      final y = u.y + row;
      if (y >= _height) break;
      final dst = (y * _width + u.x) * 4;
      final src = row * rowBytes;
      final n = rowBytes.clamp(0, (_width - u.x) * 4);
      if (src + n > u.rgba.length || dst + n > _pixels.length) break;
      _pixels.setRange(dst, dst + n, u.rgba, src);
    }
    _dirty = true;
    _decode();
  }

  /// Turns the framebuffer into an image, at most one decode at a time.
  void _decode() {
    if (_decoding || !_dirty) return;
    _decoding = true;
    _dirty = false;
    ui.decodeImageFromPixels(_pixels, _width, _height, ui.PixelFormat.rgba8888, (img) {
      _decoding = false;
      if (!mounted) {
        img.dispose();
        return;
      }
      final old = _image;
      setState(() => _image = img);
      old?.dispose();
      if (_dirty) _decode();
    });
  }

  void _onEnded(ErrorInfo? error) {
    _clipTimer?.cancel();
    if (_ended) return;
    widget.onOutcome?.call(error?.id ?? (_wasConnected ? 'connected' : 'cancelled'));
    setState(() {
      _ended = true;
      _error = error;
      _state = error == null ? RdpConnectionState.disconnected : RdpConnectionState.failed;
      _statusLabel = _state.label;
    });
  }

  Future<void> _askApproval(String fingerprint) async {
    final approve = await showDialog<bool>(
      context: context,
      barrierDismissible: false,
      builder: (ctx) => AlertDialog(
        icon: const Icon(Icons.verified_user_outlined),
        title: const Text('Check this computer\'s identity'),
        content: SingleChildScrollView(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            mainAxisSize: MainAxisSize.min,
            children: [
              Text('This is the first connection to ${widget.profile.displayName}. '
                  'Compare this certificate fingerprint with the one shown on that computer. '
                  'Only trust it if they match. Nothing has been sent to the computer yet.'),
              const SizedBox(height: 12),
              SelectableText(
                fingerprint,
                key: const Key('certificate-fingerprint'),
                style: const TextStyle(fontFamily: 'monospace', fontSize: 13),
              ),
            ],
          ),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('Cancel')),
          FilledButton(
            key: const Key('button-trust'),
            onPressed: () => Navigator.pop(ctx, true),
            child: const Text('They match — trust'),
          ),
        ],
      ),
    );
    _session.approveCertificate(approve: approve ?? false);
  }

  /// After a certificate change: the user has checked the new fingerprint on
  /// the computer itself and wants to trust it (KI-11).
  Future<void> _retrust() async {
    final changed = _changedFingerprints;
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        icon: const Icon(Icons.gpp_maybe_outlined),
        title: const Text('Trust the new certificate?'),
        content: const Text('Only do this if you know why the certificate changed (for example, '
            'Windows was reinstalled) and you have compared the new fingerprint with the one shown '
            'on the computer itself. Otherwise someone may be intercepting the connection.'),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('Cancel')),
          FilledButton(
            key: const Key('button-retrust-confirm'),
            onPressed: () => Navigator.pop(ctx, true),
            child: const Text('I checked — reconnect'),
          ),
        ],
      ),
    );
    if (ok != true || changed == null || !mounted) return;
    // Forget the old fingerprint; reconnecting shows the first-connection check again.
    forgetCertificate(pinsPath: widget.pinsPath, host: widget.profile.host, port: widget.profile.port);
    Navigator.of(context).pop(true);
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _clipTimer?.cancel();
    if (!_ended) {
      // Leaving the screen ends the session; record it before the stream is cancelled.
      // Deferred: listeners must not rebuild widgets while the tree is locked.
      final report = widget.onOutcome;
      final outcome = _wasConnected ? 'connected' : 'cancelled';
      if (report != null) Future.microtask(() => report(outcome));
      _session.disconnect();
    }
    _sub?.cancel();
    _image?.dispose();
    _focus.dispose();
    super.dispose();
  }

  // ---------------------------------------------------------------- input

  static const maxZoom = 4.0;

  /// Scale with zoom 1 = whole desktop visible (like BoxFit.contain).
  double _baseScale(Size box) =>
      (_width == 0 || _height == 0) ? 0 : math.min(box.width / _width, box.height / _height);

  double _scale(Size box) => _baseScale(box) * _zoom;

  Offset _centred(Size box, double s) =>
      Offset((box.width - _width * s) / 2, (box.height - _height * s) / 2);

  /// Top-left of the remote desktop in view coordinates.
  Offset _origin(Size box) => _centred(box, _scale(box)) + _pan;

  /// Keeps the zoomed desktop covering the view (no empty margins when zoomed in).
  Offset _clampPan(Size box, double s, Offset pan) {
    final c = _centred(box, s);
    double axis(double centred, double p, double content, double view) {
      if (content <= view) return 0;
      final o = (centred + p).clamp(view - content, 0.0);
      return o - centred;
    }

    return Offset(
      axis(c.dx, pan.dx, _width * s, box.width),
      axis(c.dy, pan.dy, _height * s, box.height),
    );
  }

  /// Converts a position in the view to remote-desktop coordinates.
  (int, int)? _toRemote(Offset local, Size box) {
    final s = _scale(box);
    if (s <= 0) return null;
    final o = _origin(box);
    final x = ((local.dx - o.dx) / s).round().clamp(0, _width - 1);
    final y = ((local.dy - o.dy) / s).round().clamp(0, _height - 1);
    return (x, y);
  }

  (int, int) get _cursorInt => (
        _cursor.dx.round().clamp(0, math.max(0, _width - 1)),
        _cursor.dy.round().clamp(0, math.max(0, _height - 1)),
      );

  void _onScaleStart(ScaleStartDetails d, Size box) {
    _startZoom = _zoom;
    _startFocal = d.localFocalPoint;
    _startOrigin = _origin(box);
    _dragging = false;
    if (d.pointerCount >= 2) _pinched = true;
    if (d.pointerCount == 1 && !_trackpad && !_pinched) {
      final p = _toRemote(d.localFocalPoint, box);
      if (p != null) {
        _session.mouseButton(x: p.$1, y: p.$2, button: 0, down: true);
        _dragging = true;
      }
    }
  }

  void _onScaleUpdate(ScaleUpdateDetails d, Size box) {
    if (d.pointerCount >= 2) {
      // Pinch: zoom and move the view; nothing is sent to the remote computer.
      _pinched = true;
      if (_dragging) _endDrag(box, d.localFocalPoint);
      final base = _baseScale(box);
      if (base <= 0) return;
      final zoom = (_startZoom * d.scale).clamp(1.0, maxZoom);
      final s = base * zoom;
      // Keep the desktop point that was under the fingers under them.
      final remotePt = (_startFocal - _startOrigin) / (base * _startZoom);
      final origin = d.localFocalPoint - remotePt * s;
      setState(() {
        _zoom = zoom;
        _pan = _clampPan(box, s, origin - _centred(box, s));
      });
      return;
    }
    if (_pinched) return; // finishing a pinch with one finger still down
    if (_trackpad) {
      // Trackpad: one finger moves the pointer relative to where it is.
      final s = _scale(box);
      if (s <= 0) return;
      setState(() {
        _cursor = Offset(
          (_cursor.dx + d.focalPointDelta.dx / s).clamp(0.0, (_width - 1).toDouble()),
          (_cursor.dy + d.focalPointDelta.dy / s).clamp(0.0, (_height - 1).toDouble()),
        );
      });
      final c = _cursorInt;
      _session.mouseMove(x: c.$1, y: c.$2);
    } else if (_dragging) {
      final p = _toRemote(d.localFocalPoint, box);
      if (p != null) _session.mouseMove(x: p.$1, y: p.$2);
    }
  }

  void _endDrag(Size box, Offset at) {
    _dragging = false;
    final p = _toRemote(at, box);
    if (p != null) _session.mouseButton(x: p.$1, y: p.$2, button: 0, down: false);
  }

  Offset _lastFocal = Offset.zero;

  void _touchEnded(PointerEvent e) {
    if (!_touchKinds.contains(e.kind)) return;
    if (_touches > 0) _touches--;
    if (_touches == 0) _pinched = false;
  }

  void _resetZoom() => setState(() {
        _zoom = 1;
        _pan = Offset.zero;
      });

  int _buttonFor(int buttons) {
    if (buttons & kSecondaryMouseButton != 0) return 1;
    if (buttons & kMiddleMouseButton != 0) return 2;
    return 0;
  }

  int _lastButtons = 0;

  /// Mouse input (touch and pen are handled by the gesture detector, which only
  /// accepts those, so a mouse click is never sent twice).
  void _pointer(PointerEvent e, Size box, {bool? down}) {
    if (_touchKinds.contains(e.kind)) return;
    final p = _toRemote(e.localPosition, box);
    if (p == null) return;
    if (down == null) {
      _session.mouseMove(x: p.$1, y: p.$2);
    } else if (down) {
      _lastButtons = e.buttons;
      _session.mouseButton(x: p.$1, y: p.$2, button: _buttonFor(e.buttons), down: true);
    } else {
      _session.mouseButton(x: p.$1, y: p.$2, button: _buttonFor(_lastButtons), down: false);
    }
  }

  void _touchClick(Offset local, Size box, int button) {
    final p = _trackpad ? _cursorInt : _toRemote(local, box);
    if (p == null) return;
    _session.mouseButton(x: p.$1, y: p.$2, button: button, down: true);
    _session.mouseButton(x: p.$1, y: p.$2, button: button, down: false);
  }

  KeyEventResult _onKey(FocusNode node, KeyEvent e) {
    if (_ended) return KeyEventResult.ignored;
    final sc = scancodeFor(e.physicalKey);
    if (sc == null) return KeyEventResult.ignored;
    final down = e is KeyDownEvent || e is KeyRepeatEvent;
    _session.key(scancode: sc.code, extended: sc.extended, down: down);
    return KeyEventResult.handled;
  }

  void _pressKey(int code, {bool extended = false}) {
    _session.key(scancode: code, extended: extended, down: true);
    _session.key(scancode: code, extended: extended, down: false);
  }

  Future<void> _typeText() async {
    final controller = TextEditingController();
    final text = await showDialog<String>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Type text on the remote computer'),
        content: TextField(
          controller: controller,
          autofocus: true,
          autocorrect: false,
          enableSuggestions: false,
          enableIMEPersonalizedLearning: false,
          onSubmitted: (v) => Navigator.pop(ctx, v),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx), child: const Text('Cancel')),
          FilledButton(onPressed: () => Navigator.pop(ctx, controller.text), child: const Text('Send')),
        ],
      ),
    );
    controller.dispose();
    if (text != null && text.isNotEmpty) _session.typeText(text: text);
    _focus.requestFocus();
  }

  // ---------------------------------------------------------------- UI

  Widget _remoteView() {
    final image = _image;
    if (image == null) {
      return Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            const CircularProgressIndicator(),
            const SizedBox(height: 12),
            Text(_statusLabel),
          ],
        ),
      );
    }
    return LayoutBuilder(
      builder: (context, constraints) {
        final box = constraints.biggest;
        return Focus(
          focusNode: _focus,
          autofocus: true,
          onKeyEvent: _onKey,
          child: GestureDetector(
            supportedDevices: _touchKinds,
            onTapUp: (d) => _touchClick(d.localPosition, box, 0),
            onLongPressStart: (d) => _touchClick(d.localPosition, box, 1),
            onScaleStart: (d) {
              _lastFocal = d.localFocalPoint;
              _onScaleStart(d, box);
            },
            onScaleUpdate: (d) {
              _lastFocal = d.localFocalPoint;
              _onScaleUpdate(d, box);
            },
            onScaleEnd: (_) {
              if (_dragging) _endDrag(box, _lastFocal);
            },
            child: Listener(
              onPointerHover: (e) => _pointer(e, box),
              onPointerMove: (e) => _pointer(e, box),
              onPointerDown: (e) {
                _focus.requestFocus();
                if (_touchKinds.contains(e.kind)) _touches++;
                _pointer(e, box, down: true);
              },
              onPointerUp: (e) {
                _touchEnded(e);
                _pointer(e, box, down: false);
              },
              onPointerCancel: _touchEnded,
              onPointerSignal: (e) {
                if (e is PointerScrollEvent && e.scrollDelta.dy != 0) {
                  _session.wheel(vertical: true, delta: e.scrollDelta.dy > 0 ? -120 : 120);
                }
              },
              child: _desktop(image, box),
            ),
          ),
        );
      },
    );
  }

  /// The remote desktop at the current zoom and pan, plus the trackpad cursor.
  Widget _desktop(ui.Image image, Size box) {
    final s = _scale(box);
    final o = _origin(box);
    return ClipRect(
      child: SizedBox.expand(
        child: Stack(
          children: [
            Positioned(
              left: o.dx,
              top: o.dy,
              width: _width * s,
              height: _height * s,
              child: RawImage(image: image, fit: BoxFit.fill, filterQuality: FilterQuality.medium),
            ),
            if (_trackpad)
              Positioned(
                left: o.dx + _cursor.dx * s - 2,
                top: o.dy + _cursor.dy * s - 2,
                child: const IgnorePointer(
                  child: Icon(
                    Icons.navigation,
                    key: Key('trackpad-cursor'),
                    size: 22,
                    shadows: [Shadow(blurRadius: 3)],
                  ),
                ),
              ),
          ],
        ),
      ),
    );
  }

  Widget _keysBar() {
    return Material(
      elevation: 2,
      child: SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
        child: Row(
          children: [
            _KeyButton('Esc', () => _pressKey(0x01)),
            _KeyButton('Tab', () => _pressKey(0x0F)),
            _KeyButton('Win', () => _pressKey(0x5B, extended: true)),
            _KeyButton('Ctrl+Alt+Del', _session.ctrlAltDel),
            _KeyButton('←', () => _pressKey(0x4B, extended: true)),
            _KeyButton('↑', () => _pressKey(0x48, extended: true)),
            _KeyButton('↓', () => _pressKey(0x50, extended: true)),
            _KeyButton('→', () => _pressKey(0x4D, extended: true)),
            for (var i = 1; i <= 12; i++)
              _KeyButton('F$i', () => _pressKey(i <= 10 ? 0x3A + i : 0x56 + (i - 10))),
            IconButton(
              key: const Key('button-trackpad'),
              tooltip: _trackpad ? 'Touch: tap where you want to click' : 'Touch: trackpad mode',
              isSelected: _trackpad,
              icon: const Icon(Icons.touch_app_outlined),
              selectedIcon: const Icon(Icons.mouse_outlined),
              onPressed: () => setState(() => _trackpad = !_trackpad),
            ),
            if (_zoom > 1)
              IconButton(
                tooltip: 'Show the whole screen',
                icon: const Icon(Icons.zoom_out_map),
                onPressed: _resetZoom,
              ),
            IconButton(
              tooltip: 'Type text',
              icon: const Icon(Icons.keyboard_outlined),
              onPressed: _typeText,
            ),
            if (_isAndroid && _sendsClipboard)
              IconButton(
                tooltip: 'Send clipboard to the remote computer',
                icon: const Icon(Icons.content_paste_go_outlined),
                onPressed: _offerLocalClipboard,
              ),
          ],
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final changed = _changedFingerprints;
    return Scaffold(
      appBar: AppBar(
        title: Text(widget.profile.displayName),
        actions: [
          if (widget.profile.clipboardEnabled && _state == RdpConnectionState.connected)
            Tooltip(
              message: switch (widget.profile.clipboardMode) {
                ClipboardMode.toRemoteOnly => 'Clipboard sharing on: this device → remote only',
                ClipboardMode.fromRemoteOnly => 'Clipboard sharing on: remote → this device only',
                _ => 'Clipboard sharing on (both directions)',
              },
              child: const Padding(
                padding: EdgeInsets.symmetric(horizontal: 4),
                child: Icon(Icons.content_paste_outlined, key: Key('clipboard-indicator')),
              ),
            ),
          Padding(
            padding: const EdgeInsets.only(right: 8),
            child: StatusChip(state: _state),
          ),
          if (!_ended)
            IconButton(
              key: const Key('button-disconnect'),
              tooltip: 'Disconnect',
              icon: const Icon(Icons.link_off),
              onPressed: () {
                setState(() => _state = RdpConnectionState.disconnecting);
                _session.disconnect();
              },
            ),
        ],
      ),
      body: Column(
        children: [
          if (_error != null)
            Padding(
              padding: const EdgeInsets.all(12),
              child: ErrorBanner(error: _error!),
            ),
          if (changed != null)
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 12),
              child: _FingerprintCompare(pinned: changed.$1, presented: changed.$2),
            ),
          if (changed != null && _ended && _error?.id == 'CertificateChanged')
            Padding(
              padding: const EdgeInsets.only(top: 8),
              child: TextButton.icon(
                key: const Key('button-retrust'),
                onPressed: _retrust,
                icon: const Icon(Icons.gpp_maybe_outlined),
                label: const Text('I know why it changed — trust the new certificate'),
              ),
            ),
          if (_ended)
            Padding(
              padding: const EdgeInsets.all(12),
              child: FilledButton.tonal(
                onPressed: () => Navigator.of(context).pop(),
                child: const Text('Back to connections'),
              ),
            ),
          if (!_ended) Expanded(child: _remoteView()),
          if (!_ended && _state == RdpConnectionState.connected) _keysBar(),
        ],
      ),
    );
  }
}

class _KeyButton extends StatelessWidget {
  const _KeyButton(this.label, this.onPressed);

  final String label;
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 2),
      child: OutlinedButton(onPressed: onPressed, child: Text(label)),
    );
  }
}

class _FingerprintCompare extends StatelessWidget {
  const _FingerprintCompare({required this.pinned, required this.presented});

  final String pinned;
  final String presented;

  @override
  Widget build(BuildContext context) {
    const mono = TextStyle(fontFamily: 'monospace', fontSize: 12);
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            const Text('Trusted fingerprint:'),
            SelectableText(pinned, style: mono),
            const SizedBox(height: 8),
            const Text('Fingerprint presented now:'),
            SelectableText(presented, style: mono),
          ],
        ),
      ),
    );
  }
}
