import 'dart:convert';
import 'dart:io';

import '../src/rust/api/host.dart' show hostFingerprint;

/// What the sign-in-screen service looks like right now.
class HostServiceStatus {
  const HostServiceStatus({
    required this.installed,
    required this.running,
    this.username,
    this.port,
    this.fingerprint,
  });

  final bool installed;
  final bool running;
  final String? username;
  final int? port;
  final String? fingerprint;

  static const none = HostServiceStatus(installed: false, running: false);
}

/// Thrown when the user cancels the Windows admin prompt, or it can't run.
class ElevationCancelled implements Exception {}

/// Controls the Cipher Desk Host **service** — the background part (Windows
/// only) that lets Share screen reach the sign-in and lock screens, so you can
/// sign in to this computer from another device.
///
/// The service runs as the system account and needs a one-time Windows admin
/// prompt to install or remove. Its install/remove output is read from a small
/// result file, because a program started through the admin prompt can't hand
/// its output back directly.
class HostService {
  static bool get supported => Platform.isWindows;

  /// The service program, installed next to Cipher Desk. Null in a dev build
  /// where it hasn't been placed there.
  static String? exePath() {
    if (!supported) return null;
    final dir = File(Platform.resolvedExecutable).parent.path;
    final path = '$dir${Platform.pathSeparator}cipher-desk-host-service.exe';
    return File(path).existsSync() ? path : null;
  }

  static String get dataDir {
    final base = Platform.environment['ProgramData'] ?? r'C:\ProgramData';
    return '$base\\Cipher Desk\\host-service';
  }

  static Map<String, String> _parse(String text) {
    final map = <String, String>{};
    for (final line in const LineSplitter().convert(text)) {
      final i = line.indexOf('=');
      if (i > 0) map[line.substring(0, i)] = line.substring(i + 1);
    }
    return map;
  }

  /// Reads (and then deletes) the result file written by the last command.
  static Future<Map<String, String>> _readResult() async {
    final file = File('$dataDir\\last-result.txt');
    if (!await file.exists()) return {};
    try {
      final map = _parse(await file.readAsString());
      await file.delete(); // don't leave a password on disk
      return map;
    } on FileSystemException {
      return {};
    }
  }

  static HostServiceStatus _statusFrom(Map<String, String> m) {
    if (m['INSTALLED'] != '1') return HostServiceStatus.none;
    return HostServiceStatus(
      installed: true,
      running: m['RUNNING'] == '1',
      username: m['USERNAME'],
      port: int.tryParse(m['PORT'] ?? ''),
      fingerprint: m['FINGERPRINT'],
    );
  }

  /// Current state. Does not need the admin prompt.
  static Future<HostServiceStatus> status() async {
    final exe = exePath();
    if (exe == null) return HostServiceStatus.none;
    try {
      final r = await Process.run(exe, ['status']);
      if (r.exitCode == 0) return _statusFrom(_parse(r.stdout as String));
    } on ProcessException {
      // Fall through to the result file.
    }
    return _statusFrom(await _readResult());
  }

  /// The service's own certificate identity code (unelevated read).
  static Future<String?> fingerprint() async {
    if (!supported) return null;
    try {
      return hostFingerprint(dataDir: dataDir);
    } on Object {
      return null;
    }
  }

  /// Runs a command with a Windows admin prompt and waits for it to finish.
  /// Throws [ElevationCancelled] if the prompt is declined.
  static Future<void> _elevated(String command) async {
    final exe = exePath();
    if (exe == null) throw ElevationCancelled();
    final ps =
        "Start-Process -FilePath '$exe' -ArgumentList '$command' -Verb RunAs -Wait -WindowStyle Hidden";
    final r = await Process.run('powershell', ['-NoProfile', '-Command', ps]);
    if (r.exitCode != 0) throw ElevationCancelled();
  }

  /// Installs and starts the service; returns the new state plus the sharing
  /// password (shown once on the page).
  static Future<(HostServiceStatus, String?)> install() async {
    await _elevated('install');
    final m = await _readResult();
    return (_statusFrom(m), m['PASSWORD']);
  }

  /// Stops and removes the service.
  static Future<void> uninstall() async {
    await _elevated('uninstall');
    await _readResult();
  }

  /// Makes a new sharing password; returns it.
  static Future<String?> newPassword() async {
    await _elevated('new-password');
    return (await _readResult())['PASSWORD'];
  }

  /// The current sharing password, to show when the user asks.
  static Future<String?> showPassword() async {
    await _elevated('show-password');
    return (await _readResult())['PASSWORD'];
  }
}
