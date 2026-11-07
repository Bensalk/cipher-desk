import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

/// Keeps a remote screen out of screenshots and screen recording (S10):
/// FLAG_SECURE on Android, display affinity on Windows. Does nothing on Linux.
///
/// On Windows the window is only hidden from capture while it shows a remote
/// session. On the computer that is being shared, Cipher Desk stays visible to
/// the viewer: a window hidden from capture still takes clicks, so the viewer
/// would be clicking an invisible window. The sharing password is masked
/// instead while someone is viewing.
class SecureWindow {
  static const _channel = MethodChannel('rdpv/secure_window');

  static bool _setting = true;
  static int _sessions = 0;

  static bool get _isAndroid => !kIsWeb && defaultTargetPlatform == TargetPlatform.android;
  static bool get _isWindows => !kIsWeb && defaultTargetPlatform == TargetPlatform.windows;

  /// The "Block screenshots" setting.
  static Future<void> setEnabled(bool enabled) async {
    _setting = enabled;
    await _apply();
  }

  /// Called when a remote session screen opens (`true`) and closes (`false`).
  static Future<void> setViewing(bool viewing) async {
    _sessions = viewing ? _sessions + 1 : (_sessions - 1).clamp(0, 1 << 20);
    await _apply();
  }

  static Future<void> _apply() async {
    final bool secure;
    if (_isAndroid) {
      secure = _setting;
    } else if (_isWindows) {
      secure = _setting && _sessions > 0;
    } else {
      return;
    }
    try {
      await _channel.invokeMethod<void>('setSecure', secure);
    } on PlatformException {
      // Keep the current state if the call fails.
    } on MissingPluginException {
      // Not available (e.g. in tests).
    }
  }
}
