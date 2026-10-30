import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';

import '../platform/secure_window.dart';
import '../src/rust/api/host.dart';
import '../src/rust/api/store.dart' show defaultPortFor, ConnectionMethod;
import '../src/rust/api/vault.dart' show VaultResult;
import 'persistence.dart';

/// Where sharing stands, for the Share page and the "being shared" banner.
enum ShareStatus { off, starting, waiting, viewing, lockedOut, failed }

/// "Share this screen" (Windows only): settings, the running Host and its
/// status. Lives as long as the app, so sharing keeps going on other pages.
///
/// The sharing password is kept in the OS vault by Rust; Dart only sees it when
/// the user taps "Show password".
class ShareController extends ChangeNotifier {
  ShareController(this._paths);

  final Future<AppPaths> Function() _paths;

  /// Only Windows can share its own screen.
  static bool get supported => Platform.isWindows;

  /// Sign-in name the viewer types (not a Windows account).
  String username = 'cipher';
  int port = defaultPortFor(method: ConnectionMethod.shareScreen);

  /// Start sharing as soon as Cipher Desk opens.
  bool startWhenAppOpens = false;

  /// Open Cipher Desk when this Windows user signs in.
  bool openAtSignIn = false;

  ShareStatus status = ShareStatus.off;
  String? message;
  String? fingerprint;
  bool hasPassword = false;

  ShareHost? _host;
  StreamSubscription<HostUpdate>? _sub;

  bool get isOn => status == ShareStatus.starting ||
      status == ShareStatus.waiting ||
      status == ShareStatus.viewing ||
      status == ShareStatus.lockedOut;

  Future<String> _file() async {
    final p = await _paths();
    return '${p.dir}${Platform.pathSeparator}share_screen.json';
  }

  /// Loads the saved settings (no secrets in that file) and the identity code.
  Future<void> load() async {
    if (!supported) return;
    try {
      final f = File(await _file());
      if (await f.exists()) {
        final j = jsonDecode(await f.readAsString()) as Map<String, dynamic>;
        final u = j['username'];
        final p = j['port'];
        if (u is String && u.trim().isNotEmpty) username = u.trim();
        if (p is int && p > 0 && p < 65536) port = p;
        startWhenAppOpens = j['start_when_app_opens'] == true;
        openAtSignIn = j['open_at_sign_in'] == true;
      }
    } on Object {
      // A damaged file falls back to the defaults.
    }
    hasPassword = hostHasPassword() == VaultResult.ok;
    if (!hasPassword && hostNewPassword() == VaultResult.ok) hasPassword = true;
    fingerprint = hostFingerprint(dataDir: (await _paths()).dir);
    notifyListeners();
    if (startWhenAppOpens) await start();
  }

  Future<void> _save() async {
    try {
      await File(await _file()).writeAsString(jsonEncode({
        'username': username,
        'port': port,
        'start_when_app_opens': startWhenAppOpens,
        'open_at_sign_in': openAtSignIn,
      }));
    } on FileSystemException {
      // Settings stay in memory for this run.
    }
  }

  Future<void> update({String? username, int? port, bool? startWhenAppOpens}) async {
    if (username != null && username.trim().isNotEmpty) this.username = username.trim();
    if (port != null && port > 0 && port < 65536) this.port = port;
    if (startWhenAppOpens != null) this.startWhenAppOpens = startWhenAppOpens;
    await _save();
    notifyListeners();
  }

  /// Adds or removes Cipher Desk from this user's sign-in programs.
  /// Returns false if Windows refused.
  Future<bool> setOpenAtSignIn(bool on) async {
    const key = r'HKCU\Software\Microsoft\Windows\CurrentVersion\Run';
    final args = on
        ? ['add', key, '/v', 'Cipher Desk', '/t', 'REG_SZ', '/d', '"${Platform.resolvedExecutable}"', '/f']
        : ['delete', key, '/v', 'Cipher Desk', '/f'];
    final r = await Process.run('reg', args);
    final ok = r.exitCode == 0 || !on; // deleting a missing entry is fine
    if (ok) {
      openAtSignIn = on;
      await _save();
      notifyListeners();
    }
    return ok;
  }

  /// Replaces the sharing password with a new random one. Anyone using the
  /// old one is refused from the next connection on.
  void newPassword() {
    hasPassword = hostNewPassword() == VaultResult.ok;
    notifyListeners();
  }

  /// The sharing password, to show on screen.
  String? revealPassword() => hostRevealPassword();

  Future<void> start() async {
    if (!supported || isOn) return;
    final host = _host ??= ShareHost.create();
    status = ShareStatus.starting;
    message = null;
    notifyListeners();
    await SecureWindow.setSharing(true);
    await _sub?.cancel();
    _sub = host
        .start(port: port, username: username, dataDir: (await _paths()).dir)
        .listen(_onUpdate, onDone: () {
      if (isOn) {
        status = ShareStatus.off;
        notifyListeners();
      }
    });
  }

  void _onUpdate(HostUpdate u) {
    switch (u.kind) {
      case HostUpdateKind.listening:
        status = ShareStatus.waiting;
        message = null;
      case HostUpdateKind.viewerConnected:
        status = ShareStatus.viewing;
      case HostUpdateKind.viewerDisconnected:
        if (status == ShareStatus.viewing) status = ShareStatus.waiting;
      case HostUpdateKind.lockedOut:
        status = ShareStatus.lockedOut;
        message = 'Too many wrong passwords. New connections are refused for '
            '${u.minutes} minutes.';
      case HostUpdateKind.failed:
        status = ShareStatus.failed;
        message = u.message;
        SecureWindow.setSharing(false);
    }
    notifyListeners();
  }

  void stop() {
    SecureWindow.setSharing(false);
    _host?.stop();
    _sub?.cancel();
    _sub = null;
    status = ShareStatus.off;
    message = null;
    notifyListeners();
  }

  @override
  void dispose() {
    stop();
    super.dispose();
  }
}
