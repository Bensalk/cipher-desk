import 'dart:convert';
import 'dart:io';

import '../src/rust/api/host.dart' show hostFingerprint, hostServiceStashPassword;

/// What the sign-in-screen service looks like right now.
class HostServiceStatus {
  const HostServiceStatus({
    required this.installed,
    required this.running,
    this.username,
    this.port,
    this.fingerprint,
    this.twoStep = false,
    this.ask = false,
    this.tailscaleOnly = true,
    this.needsUpdate = false,
  });

  final bool installed;
  final bool running;
  final String? username;
  final int? port;
  final String? fingerprint;

  /// Two-step verification (authenticator app) is on.
  final bool twoStep;

  /// "Ask to connect" (no password; the person at this PC allows it) is on.
  final bool ask;

  /// Connections are accepted only over Tailscale.
  final bool tailscaleOnly;

  /// The installed service is older than this app (or in an unsafe place):
  /// offer to update it.
  final bool needsUpdate;

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
/// result file in this person's own app folder, because a program started
/// through the admin prompt can't hand its output back directly.
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

  /// This person's own Cipher Desk folder (results and the pending password
  /// go here, never to the shared service folder).
  static String get _ownDir {
    final base = Platform.environment['LOCALAPPDATA'] ??
        '${Platform.environment['USERPROFILE'] ?? r'C:\Users\Public'}\\AppData\\Local';
    final dir = Directory('$base\\Cipher Desk');
    if (!dir.existsSync()) dir.createSync(recursive: true);
    return dir.path;
  }

  static String get _resultPath => '$_ownDir\\service-result.txt';

  static Map<String, String> _parse(String text) {
    final map = <String, String>{};
    for (final line in const LineSplitter().convert(text)) {
      final i = line.indexOf('=');
      if (i > 0) map[line.substring(0, i)] = line.substring(i + 1);
    }
    return map;
  }

  /// Reads the result file written by the last command, and always tries to
  /// delete it afterwards (it can hold the sharing password or a two-step
  /// key) — even if reading or parsing it failed.
  static Future<Map<String, String>> _readResult() async {
    final file = File(_resultPath);
    if (!await file.exists()) return {};
    var map = <String, String>{};
    try {
      map = _parse(await file.readAsString());
    } on FileSystemException {
      // Leave map empty; still fall through to delete the file below.
    }
    await _deleteSecretFile(file);
    return map;
  }

  /// Deletes a file that may hold a secret, retrying once on failure. Never
  /// logs the file's content, only that a delete did not go through.
  static Future<void> _deleteSecretFile(File file) async {
    for (var attempt = 0; attempt < 2; attempt++) {
      try {
        if (await file.exists()) await file.delete();
        return;
      } on FileSystemException {
        // Try once more before giving up.
      }
    }
    if (await file.exists()) {
      stderr.writeln('Cipher Desk: could not remove a leftover file in '
          '${file.parent.path}. Delete it by hand if it is still there.');
    }
  }

  /// Removes any `service-result.txt` or `pending-password.bin` left behind
  /// by a previous run that was closed or crashed before it could clean up
  /// (both can hold a secret). Call once at app startup; safe to call even
  /// when the service was never installed.
  static Future<void> cleanupLeftoverSecrets() async {
    if (!supported) return;
    await _deleteSecretFile(File(_resultPath));
    await _deleteSecretFile(File('$_ownDir\\pending-password.bin'));
  }

  static HostServiceStatus _statusFrom(Map<String, String> m) {
    if (m['INSTALLED'] != '1') return HostServiceStatus.none;
    return HostServiceStatus(
      installed: true,
      running: m['RUNNING'] == '1',
      username: m['USERNAME'],
      port: int.tryParse(m['PORT'] ?? ''),
      fingerprint: m['FINGERPRINT'],
      twoStep: m['TWOSTEP'] == '1',
      ask: m['ASK'] == '1',
      tailscaleOnly: m['TAILSCALE_ONLY'] != '0',
      needsUpdate: m['NEEDS_UPDATE'] == '1',
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
  /// Throws [ElevationCancelled] if the prompt is declined or can't be shown.
  static Future<void> _elevated(String command) async {
    final exe = exePath();
    if (exe == null) throw ElevationCancelled();
    // Clear any leftover result so we only read this run's output.
    try {
      final old = File(_resultPath);
      if (await old.exists()) await old.delete();
    } on FileSystemException {
      // ignore
    }
    // Single quotes inside a PowerShell '...' string are doubled.
    String q(String v) => v.replaceAll("'", "''");
    final args = '$command --result "$_resultPath"';
    final ps =
        "Start-Process -FilePath '${q(exe)}' -ArgumentList '${q(args)}' -Verb RunAs -Wait -WindowStyle Hidden";
    try {
      final r = await Process.run('powershell', ['-NoProfile', '-Command', ps]);
      if (r.exitCode != 0) throw ElevationCancelled();
    } on ProcessException {
      throw ElevationCancelled();
    }
  }

  /// Installs and starts the service; returns the new state plus the sharing
  /// password (shown once on the page).
  static Future<(HostServiceStatus, String?)> install() async {
    await _elevated('install');
    final m = await _readResult();
    return (_statusFrom(m), m['PASSWORD']);
  }

  /// Turns sharing back on if it was turned off with "Exit" or "Go offline"
  /// in the tray icon. Quiet: no prompt, nothing shown, errors ignored.
  static Future<void> goOnlineQuietly() async {
    final exe = exePath();
    if (exe == null) return;
    try {
      await Process.run(exe, ['go-online', '--quiet']);
    } catch (_) {}
  }

  /// Updates the installed service to this app's version (same admin
  /// prompt as installing). Returns the new state.
  static Future<HostServiceStatus> update() async {
    await _elevated('install');
    return _statusFrom(await _readResult());
  }

  /// Turns "Ask to connect" on or off (restarts sharing).
  static Future<void> setAsk(bool on) async {
    await _elevated('set ask=${on ? 'on' : 'off'}');
    await _readResult();
  }

  /// Accept connections only over Tailscale, or from any network
  /// (restarts sharing).
  static Future<void> setTailscaleOnly(bool on) async {
    await _elevated('set networks=${on ? 'tailscale' : 'any'}');
    await _readResult();
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

  /// Starts two-step verification setup: returns the setup key to type into
  /// the authenticator app, the account name it shows, and the QR code to
  /// scan (rows of '1' dark / '0' light; empty if unavailable), or null.
  static Future<(String key, String account, List<String> qr)?> beginTwoStep() async {
    await _elevated('twostep-begin');
    final m = await _readResult();
    final key = m['TWOSTEP_KEY'];
    if (key == null || key.isEmpty) return null;
    final qr = (m['TWOSTEP_QR'] ?? '').split('|').where((r) => r.isNotEmpty).toList();
    return (key, m['TWOSTEP_ACCOUNT'] ?? 'this computer', qr);
  }

  /// Confirms setup with a code from the app; true when two-step is now on.
  static Future<bool> finishTwoStep(String code) async {
    final digits = code.replaceAll(RegExp(r'\D'), '');
    if (digits.length != 6) return false;
    await _elevated('twostep-finish $digits');
    return (await _readResult())['TWOSTEP'] == '1';
  }

  /// Turns two-step verification off.
  static Future<void> twoStepOff() async {
    await _elevated('twostep-off');
    await _readResult();
  }

  /// Sets a password the user typed. Never plain text and never on the
  /// command line: the app writes it to a pending file in this person's own
  /// app folder, encrypted with Windows DPAPI at machine scope (any process
  /// on this computer could decrypt it — the real protection is the file's
  /// short lifetime and its folder's normal per-account permissions, not
  /// the encryption itself). The elevated service deletes the file the
  /// moment it has read it, whether that succeeds or fails. Returns true on
  /// success.
  static Future<bool> setOwnPassword(String password) async {
    if (!supported) return false;
    final pending = '$_ownDir\\pending-password.bin';
    if (!hostServiceStashPassword(pendingPath: pending, password: password)) {
      return false;
    }
    await _elevated('import-password "$pending"');
    final ok = (await _readResult())['PASSWORD_SET'] == '1';
    // The service deletes the pending file once it has imported it; remove
    // any leftover (for example if it failed before reaching that step).
    await _deleteSecretFile(File(pending));
    return ok;
  }
}
