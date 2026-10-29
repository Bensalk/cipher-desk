import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:local_auth/local_auth.dart';

import '../src/rust/api/vault.dart';

/// Result of a vault operation.
enum VaultOutcome { ok, notFound, unavailable, locked, invalidId }

/// Saved passwords (S4, S5).
///
/// - Windows / Linux: the OS vault through the Rust core. To connect, the
///   password is read inside Rust ([startWithSavedPassword]) and never enters Dart.
/// - Android: the Android Keystore through Kotlin; the system prompt confirms
///   the user before every save and read.
class PasswordVault {
  static const _channel = MethodChannel('rdpv/vault');

  static bool get _isAndroid => !kIsWeb && defaultTargetPlatform == TargetPlatform.android;
  static bool get _isWindows => !kIsWeb && defaultTargetPlatform == TargetPlatform.windows;

  /// True when the password must be read into Dart to connect (Android).
  static bool get readsIntoDart => _isAndroid;

  static VaultOutcome _fromCode(Object? code) => switch (code) {
        'ok' => VaultOutcome.ok,
        'notFound' => VaultOutcome.notFound,
        'locked' => VaultOutcome.locked,
        'invalidId' => VaultOutcome.invalidId,
        _ => VaultOutcome.unavailable,
      };

  static VaultOutcome _fromRust(VaultResult r) => switch (r) {
        VaultResult.ok => VaultOutcome.ok,
        VaultResult.notFound => VaultOutcome.notFound,
        VaultResult.unavailable => VaultOutcome.unavailable,
        VaultResult.locked => VaultOutcome.locked,
        VaultResult.invalidId => VaultOutcome.invalidId,
      };

  static Future<VaultOutcome> _android(String method, Map<String, Object?> args) async {
    try {
      final r = await _channel.invokeMapMethod<String, Object?>(method, args);
      return _fromCode(r?['code']);
    } on PlatformException {
      return VaultOutcome.unavailable;
    } on MissingPluginException {
      return VaultOutcome.unavailable;
    }
  }

  /// Saves (or replaces) the password. On Android the user confirms first;
  /// on Windows Windows Hello is asked first.
  static Future<VaultOutcome> save(String connectionId, String password) async {
    if (_isAndroid) {
      return _android('save', {'id': connectionId, 'password': password});
    }
    if (_isWindows && !await confirmUser('Confirm to save this password')) {
      return VaultOutcome.locked;
    }
    return _fromRust(vaultSavePassword(connectionId: connectionId, password: password));
  }

  /// Deletes the saved password (no error if there was none).
  static Future<VaultOutcome> delete(String connectionId) async {
    if (_isAndroid) return _android('delete', {'id': connectionId});
    return _fromRust(vaultDeletePassword(connectionId: connectionId));
  }

  /// Android only: confirms the user and returns the saved password.
  static Future<(VaultOutcome, String?)> loadOnAndroid(String connectionId) async {
    try {
      final r = await _channel.invokeMapMethod<String, Object?>('load', {'id': connectionId});
      final outcome = _fromCode(r?['code']);
      final pw = r?['password'];
      return (outcome, outcome == VaultOutcome.ok && pw is String ? pw : null);
    } on PlatformException {
      return (VaultOutcome.unavailable, null);
    } on MissingPluginException {
      return (VaultOutcome.unavailable, null);
    }
  }

  /// Confirms the user before a saved password is used (S5).
  ///
  /// - Windows: Windows Hello (face, fingerprint or PIN). If Windows Hello is not
  ///   set up, returns false so the password is typed instead (fail safe).
  /// - Linux: the keyring itself asks for its password when it is locked.
  /// - Android: handled by the Keystore prompt in [loadOnAndroid].
  ///
  /// [required] comes from the "Confirm before using a saved password" setting.
  /// It only affects Windows: on Android the Keystore key itself demands the
  /// confirmation, and on Linux the keyring decides.
  static Future<bool> confirmUser(String reason, {bool required = true}) async {
    if (!_isWindows || !required) return true;
    final auth = LocalAuthentication();
    try {
      if (!await auth.isDeviceSupported()) return false;
      return await auth.authenticate(localizedReason: reason);
    } on PlatformException {
      return false;
    }
  }
}
