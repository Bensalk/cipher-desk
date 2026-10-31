import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

/// Keeps the app window out of screenshots and screen sharing (S10):
/// FLAG_SECURE on Android, display affinity on Windows (the window is left out
/// of screenshots, recordings and screen sharing). Does nothing on Linux.
///
/// On Windows the window is also kept out while "Share this screen" is on,
/// whatever the setting, so a viewer never sees the sharing password.
class SecureWindow {
  static const _channel = MethodChannel('rdpv/secure_window');

  static bool _setting = true;
  static bool _sharing = false;

  static bool get _supported =>
      !kIsWeb &&
      (defaultTargetPlatform == TargetPlatform.android ||
          defaultTargetPlatform == TargetPlatform.windows);

  /// The "Block screenshots" setting.
  static Future<void> setEnabled(bool enabled) async {
    _setting = enabled;
    await _apply();
  }

  /// Called by "Share this screen" when sharing starts and stops.
  static Future<void> setSharing(bool sharing) async {
    if (_sharing == sharing) return;
    _sharing = sharing;
    await _apply();
  }

  static Future<void> _apply() async {
    if (!_supported) return;
    try {
      await _channel.invokeMethod<void>('setSecure', _setting || _sharing);
    } on PlatformException {
      // Keep the default (protected) state if the call fails.
    } on MissingPluginException {
      // Not available (e.g. in tests).
    }
  }
}
