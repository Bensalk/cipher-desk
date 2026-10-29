import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

/// Turns Android's screenshot/recent-apps protection (FLAG_SECURE, S10) on or
/// off. Does nothing on other platforms.
class SecureWindow {
  static const _channel = MethodChannel('rdpv/secure_window');

  static Future<void> setEnabled(bool enabled) async {
    if (kIsWeb || defaultTargetPlatform != TargetPlatform.android) return;
    try {
      await _channel.invokeMethod<void>('setSecure', enabled);
    } on PlatformException {
      // Keep the default (protected) state if the call fails.
    } on MissingPluginException {
      // Not available (e.g. in tests).
    }
  }
}
