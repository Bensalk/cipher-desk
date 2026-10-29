import 'dart:async';
import 'dart:io';
import 'dart:math' as math;
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:path_provider/path_provider.dart';

import '../session/scancodes.dart';
import '../src/rust/api/errors.dart';
import '../src/rust/api/session.dart';
import '../state/profiles.dart';
import 'error_banner.dart';
import 'status_chip.dart';

/// Shows a remote desktop session (Stage 4): connection progress, the
/// certificate prompt, the remote screen, keyboard/mouse/touch input and a
/// small bar of special keys.
class SessionScreen extends StatefulWidget {
  const SessionScreen({
    super.key,
    required this.profile,
    this.password,
    this.useSavedPassword = false,
  });

  final ConnectionProfile profile;

  /// Typed (or, on Android, vault-read) password. Used once, never stored here.
  final String? password;

  /// Windows/Linux: the Rust core reads the saved password from the OS vault.
  final bool useSavedPassword;

  @override
  State<SessionScreen> createState() => _SessionScreenState();
}

class _SessionScreenState extends State<SessionScreen> {
  final RdpSession _session = RdpSession.create();
  StreamSubscription<SessionUpdate>? _sub;
  final FocusNode _focus = FocusNode(debugLabel: 'remote-desktop');

  RdpConnectionState _state = RdpConnectionState.connecting;
  String _statusLabel = 'Connecting…';
  ErrorInfo? _error;
  bool _ended = false;

  // Remote framebuffer (RGBA) and the image decoded from it.
  int _width = 0;
  int _height = 0;
  Uint8List _pixels = Uint8List(0);
  ui.Image? _image;
  bool _decoding = false;
  bool _dirty = false;

  @override
  void initState() {
    super.initState();
    _start();
  }

  Future<void> _start() async {
    final dir = await getApplicationSupportDirectory();
    final pinsPath = pinsFilePath(dir.path);
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
          )
        : _session.start(
            host: widget.profile.host,
            port: widget.profile.port,
            username: widget.profile.username,
            password: widget.password ?? '',
            width: w,
            height: h,
            pinsPath: pinsPath,
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
        if (u.connected) _focus.requestFocus();
      case UpdateKind.certificateApproval:
        _askApproval(u.fingerprint ?? '');
      case UpdateKind.certificateChanged:
        setState(() => _changedFingerprints = (u.pinned ?? '', u.fingerprint ?? ''));
      case UpdateKind.resized:
        setState(() {
          _width = u.width;
          _height = u.height;
          _pixels = Uint8List(_width * _height * 4);
        });
      case UpdateKind.frame:
        _applyFrame(u);
      case UpdateKind.ended:
        _onEnded(u.error);
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
    if (_ended) return;
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

  @override
  void dispose() {
    if (!_ended) _session.disconnect();
    _sub?.cancel();
    _image?.dispose();
    _focus.dispose();
    super.dispose();
  }

  // ---------------------------------------------------------------- input

  /// Converts a position in the view to remote-desktop coordinates.
  (int, int)? _toRemote(Offset local, Size box) {
    if (_width == 0 || _height == 0) return null;
    // Same scaling as BoxFit.contain: whole desktop visible, centred.
    final scale = math.min(box.width / _width, box.height / _height);
    if (scale <= 0) return null;
    final dx = (box.width - _width * scale) / 2;
    final dy = (box.height - _height * scale) / 2;
    final x = ((local.dx - dx) / scale).round().clamp(0, _width - 1);
    final y = ((local.dy - dy) / scale).round().clamp(0, _height - 1);
    return (x, y);
  }

  int _buttonFor(int buttons) {
    if (buttons & kSecondaryMouseButton != 0) return 1;
    if (buttons & kMiddleMouseButton != 0) return 2;
    return 0;
  }

  int _lastButtons = 0;

  void _pointer(PointerEvent e, Size box, {bool? down}) {
    if (e.kind == PointerDeviceKind.touch) return; // handled by gestures
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
    final p = _toRemote(local, box);
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
            onTapUp: (d) => _touchClick(d.localPosition, box, 0),
            onLongPressStart: (d) => _touchClick(d.localPosition, box, 1),
            onPanStart: (d) {
              final p = _toRemote(d.localPosition, box);
              if (p != null) _session.mouseButton(x: p.$1, y: p.$2, button: 0, down: true);
            },
            onPanUpdate: (d) {
              final p = _toRemote(d.localPosition, box);
              if (p != null) _session.mouseMove(x: p.$1, y: p.$2);
            },
            onPanEnd: (d) {
              final p = _toRemote(d.localPosition, box);
              if (p != null) _session.mouseButton(x: p.$1, y: p.$2, button: 0, down: false);
            },
            child: Listener(
              onPointerHover: (e) => _pointer(e, box),
              onPointerMove: (e) => _pointer(e, box),
              onPointerDown: (e) {
                _focus.requestFocus();
                _pointer(e, box, down: true);
              },
              onPointerUp: (e) => _pointer(e, box, down: false),
              onPointerSignal: (e) {
                if (e is PointerScrollEvent && e.scrollDelta.dy != 0) {
                  _session.wheel(vertical: true, delta: e.scrollDelta.dy > 0 ? -120 : 120);
                }
              },
              child: SizedBox.expand(
                child: RawImage(image: image, fit: BoxFit.contain, filterQuality: FilterQuality.medium),
              ),
            ),
          ),
        );
      },
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
              tooltip: 'Type text',
              icon: const Icon(Icons.keyboard_outlined),
              onPressed: _typeText,
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

/// Location of the certificate pin file in the app's private data folder.
String pinsFilePath(String appDataDir) =>
    '$appDataDir${Platform.pathSeparator}certificate_pins.json';

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
