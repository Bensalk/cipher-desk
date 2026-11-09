import 'dart:async';
import 'dart:io';
import 'dart:math' as math;
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:file_selector/file_selector.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:path_provider/path_provider.dart';
import 'package:share_plus/share_plus.dart';

import '../platform/app_window.dart';
import '../platform/file_clipboard.dart';
import '../platform/remote_clipboard.dart';
import '../platform/secure_window.dart';
import '../platform/session_service.dart';
import '../session/scancodes.dart';
import '../src/rust/api/errors.dart';
import '../src/rust/api/log.dart' as rust_log;
import '../src/rust/api/pins.dart';
import '../src/rust/api/session.dart';
import '../src/rust/api/store.dart' show ConnectionMethod;
import '../state/profiles.dart';
import 'connection_methods.dart';
import 'error_banner.dart';
import 'picture_quality.dart';
import 'status_chip.dart';

/// The remote desktop size to ask for, in pixels (even numbers).
///
/// Computers: the size of this window, within 800×600 and 3840×2160.
/// Phones and tablets: always landscape (a portrait Windows desktop is awkward to use,
/// and the phone can be turned), at most 1920×1080 so text stays readable when zoomed.
(int, int) desktopSizeFor(Size physical, {required bool phoneOrTablet}) {
  var width = physical.width;
  var height = physical.height;
  double maxW = 3840, maxH = 2160;
  if (phoneOrTablet) {
    if (height > width) (width, height) = (height, width);
    maxW = 1920;
    maxH = 1080;
  }
  int even(double v, double lo, double hi) => (v.clamp(lo, hi) ~/ 2) * 2;
  return (even(width, 800, maxW), even(height, 600, maxH));
}

/// Shows a remote desktop session: connection progress, the certificate
/// prompt, the remote screen, keyboard/mouse/touch input (direct touch or
/// trackpad mode, with pinch-zoom) and a small bar of special keys.
///
/// Pops with `true` when the user asks to connect again (the Reconnect button,
/// or after re-trusting a changed certificate).
class SessionScreen extends StatefulWidget {
  const SessionScreen({
    super.key,
    required this.profile,
    required this.pinsPath,
    this.password,
    this.useSavedPassword = false,
    this.idleMinutes = 30,
    this.onOutcome,
    this.onQualityChanged,
    this.onClipboardModeChanged,
    this.authCode = '',
    this.onAuthCode,
  });

  final ConnectionProfile profile;

  /// File holding trusted certificate fingerprints.
  final String pinsPath;

  /// Disconnect after this many idle minutes (0 = never).
  final int idleMinutes;

  /// Called once when the session ends, with "connected" (it was used and
  /// then closed), "cancelled" (closed before connecting) or an error id.
  final void Function(String outcome)? onOutcome;

  /// Saves a new picture quality for this connection. The screen then closes
  /// with `true`, so the caller connects again with the new setting.
  final void Function(PictureQuality quality)? onQualityChanged;

  /// Saves a new clipboard mode for this connection (from the "Turn on"
  /// button on the copy-and-paste-is-off notice). The screen then closes
  /// with `true`, so the caller connects again with the new setting.
  final void Function(ClipboardMode mode)? onClipboardModeChanged;

  /// Share screen with two-step verification: the 6-digit code the user
  /// typed for this attempt (empty if none).
  final String authCode;

  /// Called with a code the user typed after the host asked for one; the
  /// screen then closes with `true`, so the caller connects again with it.
  final void Function(String code)? onAuthCode;

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
  // Stretch the remote picture to fill the window (ignores aspect ratio);
  // off = fit the whole screen with correct proportions.
  _ViewMode _view = _ViewMode.shrink;

  /// Screen pixels per logical pixel, for "Original size" (one remote pixel
  /// on one screen pixel).
  double _dpr = 1;
  // Immersive: hide the app bar and key bar so the remote screen fills the window.
  bool _immersive = false;
  Offset _cursor = Offset.zero; // remote coordinates, trackpad mode
  double _startZoom = 1;
  Offset _startFocal = Offset.zero;
  Offset _startOrigin = Offset.zero;
  bool _dragging = false;
  // Touch fingers currently down, and whether this touch sequence included a
  // pinch: after a pinch, the finger that stays down must not click or drag.
  int _touches = 0;
  bool _pinched = false;
  // Two fingers either zoom (pinch) or scroll (move together up or down); which
  // one is decided by the first clear movement and kept until the fingers lift.
  _TwoFinger _twoFinger = _TwoFinger.undecided;
  double _scrollRest = 0; // finger movement not yet turned into wheel notches
  double _pinchScaleAtStart = 1; // the gesture's scale when zooming began

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

  // Android background grace period (decision 8).
  Timer? _backgroundTimer;
  bool _serviceStarted = false;

  // Clipboard (S6).
  Timer? _clipTimer;
  String? _lastLocalClip;

  // Copy and paste of files with Explorer, like AnyDesk (Windows).
  // Files copied here go to the remote; files from the remote are put on
  // this computer's clipboard. `_fileSeq` is the last clipboard change seen,
  // `_ownFileSeq` the one made by putting received files there.
  int? _fileSeq;
  int? _ownFileSeq;
  String _receiveDir = '';
  final List<String> _receivedBatch = [];
  DateTime? _lastReceived;
  Timer? _receivedBatchTimer;
  bool get _sendsClipboard =>
      widget.profile.method.supportsClipboard &&
      (widget.profile.clipboardMode == ClipboardMode.both ||
      widget.profile.clipboardMode == ClipboardMode.toRemoteOnly);
  static bool get _isAndroid => !kIsWeb && defaultTargetPlatform == TargetPlatform.android;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    // "Block screenshots" protects the remote screen while it is shown here.
    SecureWindow.setViewing(true);
    // Start after the first frame: _start reads the window size with View.of,
    // which must not be called from initState.
    WidgetsBinding.instance.addPostFrameCallback((_) => _start());
  }

  /// Desktop: check this device's clipboard every second while connected.
  /// Android: only when the app comes back to the foreground or on request,
  /// because Android shows a notice every time an app reads the clipboard.
  void _startClipboardSync() {
    _clipTimer?.cancel(); // also called again after an automatic reconnect
    if (!_sendsClipboard) return;
    // A new (or restored) session knows nothing yet: offer the current text again.
    _lastLocalClip = null;
    _offerLocalClipboard();
    if (!_isAndroid) {
      // Files already copied before connecting are not sent: only a new copy.
      _fileSeq = null;
      FileClipboard.sequence().then((seq) => _fileSeq ??= seq);
      _clipTimer = Timer.periodic(const Duration(seconds: 1), (_) {
        _offerLocalClipboard();
        _offerCopiedFiles();
      });
    }
  }

  /// Files copied in Explorer on this computer are sent to the remote, which
  /// puts them on its clipboard (Ctrl+V there pastes them).
  bool _checkingFiles = false;

  Future<void> _offerCopiedFiles() async {
    if (_ended || !_sendsClipboard || !FileClipboard.supported || _checkingFiles) return;
    _checkingFiles = true;
    try {
      await _checkCopiedFiles();
    } finally {
      _checkingFiles = false;
    }
  }

  Future<void> _checkCopiedFiles() async {
    final seq = await FileClipboard.sequence();
    if (seq == null) return;
    if (_fileSeq == null) {
      _fileSeq = seq;
      return;
    }
    if (seq == _fileSeq) return;
    // Explorer puts several formats on the clipboard one after another: let it
    // finish before reading the file list, or the copy could be missed.
    await Future<void>.delayed(const Duration(milliseconds: 150));
    final settled = await FileClipboard.sequence() ?? seq;
    _fileSeq = settled;
    if (settled == _ownFileSeq) return;
    final files = await FileClipboard.readFiles();
    if (files.isEmpty || _ended) return;
    // Files that just came from the remote: don't send them back.
    if (_receiveDir.isNotEmpty && files.every((f) => f.startsWith(_receiveDir))) return;
    _session.sendFiles(paths: files);
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(
      content: Text(files.length == 1
          ? 'Copying "${files.first.split(Platform.pathSeparator).last}" to the remote computer. '
              'Paste it there with Ctrl+V when it has arrived.'
          : 'Copying ${files.length} items to the remote computer. '
              'Paste them there with Ctrl+V when they have arrived.'),
    ));
  }

  /// How long to wait, after the last file of a batch lands, before putting
  /// the batch on the clipboard. Several files (or a folder) arrive as
  /// separate events close together; writing the clipboard again for each one
  /// can land mid-paste if the user is quick, which Explorer shows as
  /// "The object invoked has disconnected from its clients." Waiting for the
  /// batch to go quiet first means Explorer only ever sees one, complete write.
  static const _receivedBatchSettle = Duration(milliseconds: 600);

  /// Puts files received from the remote on this computer's clipboard, so
  /// Ctrl+V in a folder pastes them. Files arriving close together form one
  /// batch, pasted together once no more have arrived for a short while.
  void _putReceivedOnClipboard(String path) {
    if (!FileClipboard.supported) return;
    final now = DateTime.now();
    final last = _lastReceived;
    if (last == null || now.difference(last) > const Duration(seconds: 5)) {
      _receivedBatch.clear();
    }
    _lastReceived = now;
    _receivedBatch.add(path);
    _receivedBatchTimer?.cancel();
    _receivedBatchTimer = Timer(_receivedBatchSettle, () async {
      if (!mounted) return;
      final seq = await FileClipboard.writeFiles(List.of(_receivedBatch));
      if (seq != null) {
        _ownFileSeq = seq;
        _fileSeq = seq;
      }
    });
  }

  Future<void> _offerLocalClipboard() async {
    if (_ended || !_sendsClipboard) return;
    final text = await RemoteClipboard.readText();
    if (text == null || text == _lastLocalClip || _ended) return;
    _lastLocalClip = text;
    _session.offerClipboardText(text: text);
  }

  bool get _sessionOpen =>
      _state == RdpConnectionState.connected || _state == RdpConnectionState.reconnecting;

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    switch (state) {
      case AppLifecycleState.resumed:
        _backgroundTimer?.cancel();
        _backgroundTimer = null;
        if (_state == RdpConnectionState.connected) _offerLocalClipboard();
      case AppLifecycleState.paused:
      case AppLifecycleState.hidden:
        _armBackgroundTimer();
      case AppLifecycleState.inactive:
      case AppLifecycleState.detached:
        break;
    }
  }

  bool get _inBackground {
    final s = WidgetsBinding.instance.lifecycleState;
    return s == AppLifecycleState.paused || s == AppLifecycleState.hidden;
  }

  /// Android: end the session if the app stays in the background (decision 8).
  /// Also called when a connection completes while the app is already in the
  /// background, so such a session is never left open without a time limit.
  void _armBackgroundTimer() {
    if (!SessionService.isAndroid || !_sessionOpen || _ended) return;
    _backgroundTimer ??= Timer(SessionService.backgroundGrace, () {
      if (!_ended) _session.disconnectInBackground();
    });
  }

  Future<void> _start() async {
    rust_log.sessionLogClear(); // fresh log for each connection attempt
    final pinsPath = widget.pinsPath;
    if (!mounted) return;
    final (w, h) = desktopSizeFor(View.of(context).physicalSize, phoneOrTablet: _isAndroid);
    // Clipboard is text-only and off for VNC; the method decides.
    final clipboard =
        widget.profile.method.supportsClipboard ? widget.profile.clipboardMode : ClipboardMode.off;
    // Files the remote sends land in a "Received" folder beside the app data.
    // Empty turns file receiving off (when the clipboard is off).
    final downloadDir = clipboard == ClipboardMode.off
        ? ''
        : '${File(pinsPath).parent.path}${Platform.pathSeparator}Received';
    _receiveDir = downloadDir;
    // Copy and paste is off for this connection: offer to turn it on right
    // here, since copying text or files would otherwise silently do nothing.
    // (The global Settings default only applies to new connections, so an
    // already-saved one like this can still be off even after that is
    // turned on — this button fixes THIS connection, with no settings menu.)
    if (clipboard == ClipboardMode.off && widget.profile.method.supportsClipboard) {
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(
        duration: const Duration(seconds: 20),
        content: const Text('Copy and paste is off for this connection.'),
        action: widget.onClipboardModeChanged == null
            ? null
            : SnackBarAction(
                label: 'Turn on',
                onPressed: () {
                  widget.onClipboardModeChanged?.call(ClipboardMode.both);
                  Navigator.of(context).pop(true);
                },
              ),
      ));
    }
    final vnc = widget.profile.method == ConnectionMethod.vnc;
    final stream = vnc
        ? (widget.useSavedPassword
            ? _session.startVncWithSavedPassword(
                connectionId: widget.profile.id,
                host: widget.profile.host,
                port: widget.profile.port,
                idleMinutes: widget.idleMinutes,
              )
            : _session.startVnc(
                host: widget.profile.host,
                port: widget.profile.port,
                password: widget.password ?? '',
                idleMinutes: widget.idleMinutes,
              ))
        : widget.useSavedPassword
        ? _session.startWithSavedPassword(
            connectionId: widget.profile.id,
            host: widget.profile.host,
            port: widget.profile.port,
            username: widget.profile.username,
            width: w,
            height: h,
            pinsPath: pinsPath,
            clipboardDirection: clipboard,
            downloadDir: downloadDir,
            idleMinutes: widget.idleMinutes,
            quality: widget.profile.quality,
            shareScreen: widget.profile.method == ConnectionMethod.shareScreen,
            authCode: widget.authCode,
          )
        : _session.start(
            host: widget.profile.host,
            port: widget.profile.port,
            username: widget.profile.username,
            password: widget.password ?? '',
            width: w,
            height: h,
            pinsPath: pinsPath,
            clipboardDirection: clipboard,
            downloadDir: downloadDir,
            idleMinutes: widget.idleMinutes,
            quality: widget.profile.quality,
            shareScreen: widget.profile.method == ConnectionMethod.shareScreen,
            authCode: widget.authCode,
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
          if (!_serviceStarted) {
            _serviceStarted = true;
            SessionService.start();
          }
          if (_inBackground) _armBackgroundTimer();
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
      case UpdateKind.reconnecting:
        // The network dropped; Rust is reconnecting (F13). Input is ignored meanwhile.
        // The new session starts with no keys held.
        _clipTimer?.cancel();
        setState(() {
          _heldModifiers.clear();
          _state = RdpConnectionState.reconnecting;
          _statusLabel = u.label ?? RdpConnectionState.reconnecting.label;
        });
      case UpdateKind.remoteClipboardTooLarge:
        if (!mounted) return;
        ScaffoldMessenger.of(context).showSnackBar(const SnackBar(
          content: Text('Text copied on the remote computer was larger than 1 MB and was not copied.'),
        ));
      case UpdateKind.filesIncoming:
        if (!mounted) return;
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
          content: Text('Receiving ${u.text ?? 'files'} from the remote computer…'),
          duration: const Duration(seconds: 6),
        ));
      case UpdateKind.fileReceived:
        if (!mounted) return;
        final name = u.text ?? 'a file';
        final path = u.label;
        if (path != null && path.isNotEmpty) _putReceivedOnClipboard(path);
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
          content: Text(FileClipboard.supported
              ? 'Received "$name". Paste it in any folder with Ctrl+V.'
              : 'Received "$name" into the Received folder.'),
        ));
    }
  }

  /// Picks local files and sends them to the remote computer.
  Future<void> _sendFiles() async {
    final files = await openFiles();
    if (files.isEmpty) return;
    _session.sendFiles(paths: files.map((f) => f.path).toList());
    if (!mounted) return;
    final many = files.length > 1;
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(
      content: Text(many
          ? 'Sending ${files.length} files to the remote computer…'
          : 'Sending "${files.first.name}" to the remote computer…'),
    ));
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
    if (_immersive) _setImmersive(false);
    _heldModifiers.clear();
    _clipTimer?.cancel();
    _backgroundTimer?.cancel();
    if (_serviceStarted) SessionService.stop();
    if (_ended) return;
    widget.onOutcome?.call(error?.id ?? (_wasConnected ? 'connected' : 'cancelled'));
    setState(() {
      _ended = true;
      _error = error;
      _state = error == null ? RdpConnectionState.disconnected : RdpConnectionState.failed;
      _statusLabel = _state.label;
    });
    // Two-step verification: ask for the authenticator code at once.
    if (error?.id == 'CodeNeeded' && widget.onAuthCode != null) {
      WidgetsBinding.instance.addPostFrameCallback((_) => _askCode());
    }
  }

  /// Exports the in-memory session log so the user can share it.
  /// Android: share sheet.  Windows/Linux: file-save dialog → Desktop.
  Future<void> _reportProblem() async {
    if (!mounted) return;
    // TODO(next-bump): update this when pubspec version changes.
    // Adding package_info_plus would let us read it at runtime.
    const version = '0.3.5';
    final content = rust_log.sessionLogExport(appVersion: version);
    final fileName = 'cipher-desk-log-${DateTime.now().millisecondsSinceEpoch}.txt';

    if (_isAndroid) {
      // Write to a temp file, then hand off to the OS share sheet.
      final tmp = await getTemporaryDirectory();
      final file = File('${tmp.path}/$fileName');
      await file.writeAsString(content);
      await Share.shareXFiles(
        [XFile(file.path, mimeType: 'text/plain')],
        subject: 'Cipher Desk connection log',
      );
    } else {
      // Desktop: file-save dialog.
      final location = await getSaveLocation(
        suggestedName: fileName,
        acceptedTypeGroups: [
          const XTypeGroup(label: 'Text files', extensions: ['txt']),
        ],
      );
      if (location == null || !mounted) return;
      final file = File(location.path);
      await file.writeAsString(content);
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Log saved to ${location.path}')),
      );
    }
  }

  /// Asks for the 6-digit authenticator code, then connects again with it.
  Future<void> _askCode() async {
    if (!mounted) return;
    final code = await showDialog<String>(
      context: context,
      builder: (_) => const _CodeDialog(),
    );
    if (code == null || !mounted) return;
    widget.onAuthCode?.call(code);
    Navigator.of(context).pop(true);
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
  /// the computer itself and wants to trust it.
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
    if (_immersive) AppWindow.setFullScreen(false);
    SecureWindow.setViewing(false);
    WidgetsBinding.instance.removeObserver(this);
    _clipTimer?.cancel();
    _backgroundTimer?.cancel();
    _receivedBatchTimer?.cancel();
    if (_serviceStarted) SessionService.stop();
    if (!_ended) {
      // Leaving the screen ends the session; record it before the stream is cancelled.
      // Deferred: listeners must not rebuild widgets while the tree is locked.
      final report = widget.onOutcome;
      final outcome = _wasConnected ? 'connected' : 'cancelled';
      if (report != null) Future.microtask(() => report(outcome));
      _releaseModifiers();
      _session.disconnect();
    }
    _sub?.cancel();
    _image?.dispose();
    _focus.dispose();
    super.dispose();
  }

  /// Whether to offer "Reconnect" comes from the Rust error catalogue.
  bool get _canReconnect => _error?.canReconnect ?? false;

  // ---------------------------------------------------------------- input

  static const maxZoom = 4.0;

  /// Scale with zoom 1 = whole desktop visible (like BoxFit.contain).
  double _baseScale(Size box) {
    if (_width == 0 || _height == 0) return 0.0;
    final fit = math.min(box.width / _width, box.height / _height);
    return switch (_view) {
      // One remote pixel per screen pixel.
      _ViewMode.original => 1 / _dpr,
      // The whole screen, but never enlarged (enlarging only blurs it).
      _ViewMode.shrink => math.min(fit, 1 / _dpr),
      _ViewMode.stretch => fit,
    };
  }

  double _scale(Size box) => _baseScale(box) * _zoom;

  Offset _centred(Size box, double s) =>
      Offset((box.width - _width * s) / 2, (box.height - _height * s) / 2);

  /// Top-left of the remote desktop in view coordinates.
  Offset _origin(Size box) => _centred(box, _scale(box)) + _pan;

  // Per-axis scale and origin, so "stretch" can fill the window on each axis
  // independently. In fit mode both axes share the aspect-correct scale.
  bool get _stretch => _view == _ViewMode.stretch;
  double _sx(Size box) => _stretch && _width != 0 ? box.width / _width : _scale(box);
  double _sy(Size box) => _stretch && _height != 0 ? box.height / _height : _scale(box);
  Offset _origin2(Size box) {
    if (_stretch) return Offset.zero;
    final o = _origin(box);
    // On whole screen pixels, so 1:1 pixels stay sharp.
    return Offset((o.dx * _dpr).roundToDouble() / _dpr, (o.dy * _dpr).roundToDouble() / _dpr);
  }

  /// "Original size" with a remote screen larger than the window: the view
  /// follows the mouse, so moving it towards an edge shows that part.
  void _followMouse(Offset local, Size box) {
    if (_view != _ViewMode.original || _zoom != 1) return;
    final s = _scale(box);
    double axis(double pointer, double view, double content, double centred) {
      if (content <= view) return 0;
      final f = (pointer / view).clamp(0.0, 1.0);
      return -f * (content - view) - centred;
    }

    final c = _centred(box, s);
    final pan = Offset(
      axis(local.dx, box.width, _width * s, c.dx),
      axis(local.dy, box.height, _height * s, c.dy),
    );
    if ((pan - _pan).distance >= 0.5) setState(() => _pan = pan);
  }

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
    final sx = _sx(box);
    final sy = _sy(box);
    if (sx <= 0 || sy <= 0) return null;
    final o = _origin2(box);
    final x = ((local.dx - o.dx) / sx).round().clamp(0, _width - 1);
    final y = ((local.dy - o.dy) / sy).round().clamp(0, _height - 1);
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
      _pinched = true;
      if (_dragging) _endDrag(box, d.localFocalPoint);
      if (_twoFinger == _TwoFinger.undecided) {
        _scrollRest += d.focalPointDelta.dy;
        if ((d.scale - 1).abs() > 0.08) {
          _twoFinger = _TwoFinger.zoom;
          // Zoom from here, so the view does not jump.
          _startZoom = _zoom;
          _startFocal = d.localFocalPoint;
          _startOrigin = _origin(box);
          _pinchScaleAtStart = d.scale;
        } else if (_scrollRest.abs() > 16) {
          _twoFinger = _TwoFinger.scroll;
          _scrollRest = 0;
          return;
        } else {
          return;
        }
      }
      if (_twoFinger == _TwoFinger.scroll) {
        // Two-finger scroll: fingers up = page moves up, like scrolling on a phone.
        _scrollRest += d.focalPointDelta.dy;
        const step = 40.0; // finger movement per wheel notch
        while (_scrollRest.abs() >= step) {
          final up = _scrollRest > 0;
          _session.wheel(vertical: true, delta: up ? 120 : -120);
          _scrollRest += up ? -step : step;
        }
        return;
      }
      // Pinch: zoom and move the view; nothing is sent to the remote computer.
      final base = _baseScale(box);
      if (base <= 0) return;
      final zoom = (_startZoom * d.scale / _pinchScaleAtStart).clamp(1.0, maxZoom);
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
    if (_touches == 0) {
      _pinched = false;
      _twoFinger = _TwoFinger.undecided;
      _scrollRest = 0;
    }
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
    if (down == null) _followMouse(e.localPosition, box);
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

  /// Ctrl, Alt and Shift from the key bar stay held until tapped again, so key
  /// combinations (Ctrl+C, Alt+Tab, Shift+F10) work on touch screens.
  final Set<int> _heldModifiers = {};

  void _toggleModifier(int code) {
    final down = !_heldModifiers.contains(code);
    setState(() => down ? _heldModifiers.add(code) : _heldModifiers.remove(code));
    _session.key(scancode: code, extended: false, down: down);
    _focus.requestFocus();
  }

  void _releaseModifiers() {
    for (final code in _heldModifiers) {
      _session.key(scancode: code, extended: false, down: false);
    }
    _heldModifiers.clear();
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
              child: _state == RdpConnectionState.reconnecting
                  ? Stack(
                      fit: StackFit.expand,
                      children: [
                        _desktop(image, box),
                        // Blocks input while the connection is being restored.
                        AbsorbPointer(
                          child: ColoredBox(
                            color: Colors.black54,
                            child: Center(
                              child: Card(
                                child: Padding(
                                  padding: const EdgeInsets.all(16),
                                  child: Row(
                                    mainAxisSize: MainAxisSize.min,
                                    children: [
                                      const SizedBox(
                                        width: 20,
                                        height: 20,
                                        child: CircularProgressIndicator(strokeWidth: 2),
                                      ),
                                      const SizedBox(width: 12),
                                      Flexible(
                                        child: Text(_statusLabel, key: const Key('reconnecting-label')),
                                      ),
                                    ],
                                  ),
                                ),
                              ),
                            ),
                          ),
                        ),
                      ],
                    )
                  : _desktop(image, box),
            ),
          ),
        );
      },
    );
  }

  /// The remote desktop at the current zoom and pan, plus the trackpad cursor.
  /// Full screen: hides the toolbar and, on Windows and Android, the taskbar
  /// or system bars too.
  void _setImmersive(bool on) {
    if (_immersive == on) return;
    if (mounted) setState(() => _immersive = on);
    AppWindow.setFullScreen(on);
    _focus.requestFocus();
  }

  Widget _viewMenu() => PopupMenuButton<_ViewMode>(
        key: const Key('button-view-mode'),
        tooltip: 'View: ${_view.label}',
        icon: Icon(_view.icon),
        initialValue: _view,
        onSelected: (v) => setState(() {
          _view = v;
          _zoom = 1;
          _pan = Offset.zero;
          _focus.requestFocus();
        }),
        itemBuilder: (_) => [
          for (final v in _ViewMode.values)
            CheckedPopupMenuItem(
              value: v,
              checked: v == _view,
              child: ListTile(
                contentPadding: EdgeInsets.zero,
                title: Text(v.label),
                subtitle: Text(v.description),
              ),
            ),
        ],
      );

  Widget _qualityMenu() => PopupMenuButton<PictureQuality>(
        key: const Key('button-quality'),
        tooltip: 'Picture quality: ${widget.profile.quality.label}',
        icon: Icon(widget.profile.quality.icon),
        initialValue: widget.profile.quality,
        onSelected: (q) {
          if (q == widget.profile.quality) return;
          // The quality is agreed when connecting: save it and connect again.
          widget.onQualityChanged?.call(q);
          Navigator.of(context).pop(true);
        },
        itemBuilder: (_) => [
          for (final q in PictureQuality.values)
            CheckedPopupMenuItem(
              value: q,
              checked: q == widget.profile.quality,
              child: ListTile(
                contentPadding: EdgeInsets.zero,
                title: Text(q.label),
                subtitle: Text(q.description),
              ),
            ),
          const PopupMenuDivider(),
          const PopupMenuItem<PictureQuality>(
            enabled: false,
            child: Text('Changing it reconnects.'),
          ),
        ],
      );

  Widget _desktop(ui.Image image, Size box) {
    final sx = _sx(box);
    final sy = _sy(box);
    final o = _origin2(box);
    return ClipRect(
      child: SizedBox.expand(
        child: Stack(
          children: [
            Positioned(
              left: o.dx,
              top: o.dy,
              width: _width * sx,
              height: _height * sy,
              child: RawImage(
                image: image,
                fit: BoxFit.fill,
                // 1:1 pixels need no smoothing; scaled views do.
                filterQuality: _view == _ViewMode.original && _zoom == 1
                    ? FilterQuality.none
                    : FilterQuality.medium,
              ),
            ),
            if (_trackpad)
              Positioned(
                left: o.dx + _cursor.dx * sx - 2,
                top: o.dy + _cursor.dy * sy - 2,
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
            for (final (label, code) in const [('Ctrl', 0x1D), ('Alt', 0x38), ('Shift', 0x2A)])
              _ModifierButton(
                label,
                held: _heldModifiers.contains(code),
                onPressed: () => _toggleModifier(code),
              ),
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
            if (_sendsClipboard)
              IconButton(
                key: const Key('button-send-file'),
                tooltip: 'Send a file to the remote computer',
                icon: const Icon(Icons.upload_file_outlined),
                onPressed: _sendFiles,
              ),
          ],
        ),
      ),
    );
  }

  /// Back button / back gesture while connected: ask first, because on phones
  /// a stray swipe would otherwise end the session.
  Future<void> _confirmLeave() async {
    final leave = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text('Disconnect from ${widget.profile.displayName}?'),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('Stay connected')),
          FilledButton(
            key: const Key('button-leave-confirm'),
            onPressed: () => Navigator.pop(ctx, true),
            child: const Text('Disconnect'),
          ),
        ],
      ),
    );
    if (leave == true && mounted) Navigator.of(context).pop();
  }

  @override
  Widget build(BuildContext context) {
    _dpr = MediaQuery.devicePixelRatioOf(context);
    final changed = _changedFingerprints;
    return PopScope(
      canPop: !_sessionOpen,
      onPopInvokedWithResult: (didPop, _) {
        if (!didPop) _confirmLeave();
      },
      child: Scaffold(
      appBar: _immersive
          ? null
          : AppBar(
        title: Text(widget.profile.displayName),
        actions: [
          if (!_ended && _state == RdpConnectionState.connected) ...[
            _viewMenu(),
            if (widget.profile.method != ConnectionMethod.vnc && widget.onQualityChanged != null)
              _qualityMenu(),
            IconButton(
              key: const Key('button-fullscreen'),
              tooltip: 'Full screen',
              icon: const Icon(Icons.fullscreen),
              onPressed: () => _setImmersive(true),
            ),
          ],
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
          if (_ended && _error?.id == 'CodeNeeded' && widget.onAuthCode != null)
            Padding(
              padding: const EdgeInsets.only(top: 8),
              child: FilledButton.icon(
                key: const Key('button-enter-code'),
                onPressed: _askCode,
                icon: const Icon(Icons.pin_outlined),
                label: const Text('Enter authenticator code'),
              ),
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
              child: Wrap(
                spacing: 12,
                runSpacing: 8,
                alignment: WrapAlignment.center,
                children: [
                  if (_canReconnect)
                    FilledButton.icon(
                      key: const Key('button-reconnect'),
                      onPressed: () => Navigator.of(context).pop(true),
                      icon: const Icon(Icons.refresh),
                      label: const Text('Reconnect'),
                    ),
                  FilledButton.tonal(
                    onPressed: () => Navigator.of(context).pop(),
                    child: const Text('Back to connections'),
                  ),
                  if (_error != null)
                    OutlinedButton.icon(
                      key: const Key('button-report-problem'),
                      onPressed: _reportProblem,
                      icon: const Icon(Icons.bug_report_outlined),
                      label: const Text('Report Problem'),
                    ),
                ],
              ),
            ),
          if (!_ended)
            Expanded(
              child: LayoutBuilder(
                builder: (context, constraints) {
                  final bounds = constraints.biggest;
                  return Stack(
                    children: [
                      Positioned.fill(child: _remoteView()),
                      // Full-screen controls: a small draggable button so you can
                      // move it off whatever it's covering; tap to show the view
                      // and exit-full-screen buttons, tap again to hide them.
                      if (_immersive)
                        _FloatingToggle(
                          icon: Icons.more_horiz,
                          tooltip: 'Full-screen controls',
                          bounds: bounds,
                          initial: Offset(bounds.width - 56, 16),
                          panelAbove: false,
                          panelBuilder: (context) => SafeArea(
                            child: Material(
                              color: Colors.black54,
                              shape: const StadiumBorder(),
                              child: IconTheme(
                                data: const IconThemeData(color: Colors.white),
                                child: Row(
                                  mainAxisSize: MainAxisSize.min,
                                  children: [
                                    _viewMenu(),
                                    IconButton(
                                      key: const Key('button-exit-fullscreen'),
                                      tooltip: 'Exit full screen',
                                      icon: const Icon(Icons.fullscreen_exit),
                                      onPressed: () => _setImmersive(false),
                                    ),
                                  ],
                                ),
                              ),
                            ),
                          ),
                        ),
                      // Keyboard shortcuts: same idea — a draggable button instead
                      // of a bar that always takes up space across the bottom.
                      if (!_immersive && _state == RdpConnectionState.connected)
                        _FloatingToggle(
                          icon: Icons.keyboard_outlined,
                          tooltip: 'Keyboard shortcuts',
                          bounds: bounds,
                          // Lines the button up so the panel (up to 560 wide) opens
                          // roughly centred above it instead of running off-screen.
                          initial: Offset((bounds.width - 560).clamp(0.0, double.infinity) / 2, bounds.height - 64),
                          panelAbove: true,
                          // A horizontal scroller needs a bounded width; give it
                          // most of the screen width, capped so it isn't huge on a
                          // big monitor.
                          panelBuilder: (context) => SizedBox(
                            width: (bounds.width - 32).clamp(200, 560),
                            child: _keysBar(),
                          ),
                        ),
                    ],
                  );
                },
              ),
            ),
        ],
      ),
    ),
    );
  }
}

enum _TwoFinger { undecided, zoom, scroll }

/// A small round button that floats over the remote picture. Drag it
/// anywhere so it isn't covering something on the remote screen. Tap it to
/// open a panel next to it (the keyboard shortcuts, or the full-screen
/// controls); tap again to close the panel. The button itself always stays
/// on screen — only the panel can be closed — so full screen and disconnect
/// are never out of reach.
class _FloatingToggle extends StatefulWidget {
  const _FloatingToggle({
    required this.icon,
    required this.tooltip,
    required this.bounds,
    required this.initial,
    required this.panelBuilder,
    required this.panelAbove,
  });

  final IconData icon;
  final String tooltip;
  final Size bounds;
  final Offset initial;
  final Widget Function(BuildContext context) panelBuilder;

  /// Whether the panel opens above the button (for a button near the bottom
  /// of the screen) or below it (for a button near the top).
  final bool panelAbove;

  @override
  State<_FloatingToggle> createState() => _FloatingToggleState();
}

class _FloatingToggleState extends State<_FloatingToggle> {
  static const double _size = 44;
  // Idle opacity: visible enough to find, unobtrusive over the remote picture.
  static const double _opacityIdle = 0.25;
  static const double _opacityActive = 1.0;

  late Offset _pos = _clamp(widget.initial, widget.bounds);
  bool _open = false;
  bool _hovering = false;
  bool _dragging = false;

  bool get _active => _open || _hovering || _dragging;

  static Offset _clamp(Offset p, Size bounds) {
    final maxX = (bounds.width - _size).clamp(0.0, double.infinity);
    final maxY = (bounds.height - _size).clamp(0.0, double.infinity);
    return Offset(p.dx.clamp(0.0, maxX), p.dy.clamp(0.0, maxY));
  }

  @override
  void didUpdateWidget(covariant _FloatingToggle old) {
    super.didUpdateWidget(old);
    // The window was resized or rotated: keep the button on screen.
    if (old.bounds != widget.bounds) _pos = _clamp(_pos, widget.bounds);
  }

  @override
  Widget build(BuildContext context) {
    final button = MouseRegion(
      onEnter: (_) => setState(() => _hovering = true),
      onExit: (_) => setState(() => _hovering = false),
      child: GestureDetector(
        onPanStart: (_) => setState(() => _dragging = true),
        onPanUpdate: (d) => setState(() => _pos = _clamp(_pos + d.delta, widget.bounds)),
        onPanEnd: (_) => setState(() => _dragging = false),
        onPanCancel: () => setState(() => _dragging = false),
        onTap: () => setState(() => _open = !_open),
        child: AnimatedOpacity(
          opacity: _active ? _opacityActive : _opacityIdle,
          duration: const Duration(milliseconds: 200),
          child: Tooltip(
            message: widget.tooltip,
            child: Material(
              color: Colors.black54,
              shape: const CircleBorder(),
              elevation: 4,
              child: SizedBox(
                width: _size,
                height: _size,
                child: Icon(_open ? Icons.close : widget.icon, color: Colors.white, size: 22),
              ),
            ),
          ),
        ),
      ),
    );
    return Positioned(
      left: _pos.dx,
      top: _pos.dy,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisSize: MainAxisSize.min,
        children: widget.panelAbove
            ? [
                if (_open) Padding(padding: const EdgeInsets.only(bottom: 6), child: widget.panelBuilder(context)),
                button,
              ]
            : [
                button,
                if (_open) Padding(padding: const EdgeInsets.only(top: 6), child: widget.panelBuilder(context)),
              ],
      ),
    );
  }
}

class _ModifierButton extends StatelessWidget {
  const _ModifierButton(this.label, {required this.held, required this.onPressed});

  final String label;
  final bool held;
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 2),
      child: Tooltip(
        message: held ? '$label is held: tap to release' : 'Hold $label for the next keys',
        child: held
            ? FilledButton(onPressed: onPressed, child: Text(label))
            : OutlinedButton(onPressed: onPressed, child: Text(label)),
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

/// How the remote screen fits the window (like AnyDesk's view modes).
enum _ViewMode {
  original,
  shrink,
  stretch;

  String get label => switch (this) {
        _ViewMode.original => 'Original size',
        _ViewMode.shrink => 'Shrink to fit',
        _ViewMode.stretch => 'Stretch to fill',
      };

  String get description => switch (this) {
        _ViewMode.original => 'Sharpest: one remote pixel per screen pixel. Move the mouse to an edge to see more.',
        _ViewMode.shrink => 'The whole screen, never enlarged.',
        _ViewMode.stretch => 'Fills the window.',
      };

  IconData get icon => switch (this) {
        _ViewMode.original => Icons.crop_original,
        _ViewMode.shrink => Icons.fit_screen_outlined,
        _ViewMode.stretch => Icons.aspect_ratio_outlined,
      };
}

/// Asks for the 6-digit code from the authenticator app.
class _CodeDialog extends StatefulWidget {
  const _CodeDialog();

  @override
  State<_CodeDialog> createState() => _CodeDialogState();
}

class _CodeDialogState extends State<_CodeDialog> {
  final _controller = TextEditingController();

  bool get _valid => RegExp(r'^\d{6}$').hasMatch(_controller.text.trim());

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  void _submit() {
    if (_valid) Navigator.pop(context, _controller.text.trim());
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: const Text('Authenticator code'),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const Text('Open Google Authenticator on your phone and type the '
              '6-digit code for Cipher Desk. This computer is then remembered '
              'for 30 days.'),
          const SizedBox(height: 12),
          TextField(
            key: const Key('field-auth-code'),
            controller: _controller,
            autofocus: true,
            keyboardType: TextInputType.number,
            maxLength: 6,
            inputFormatters: [FilteringTextInputFormatter.digitsOnly],
            decoration: const InputDecoration(labelText: '6-digit code'),
            onChanged: (_) => setState(() {}),
            onSubmitted: (_) => _submit(),
          ),
        ],
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context), child: const Text('Cancel')),
        FilledButton(onPressed: _valid ? _submit : null, child: const Text('Connect')),
      ],
    );
  }
}
