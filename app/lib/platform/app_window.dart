import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

/// Real full screen for remote sessions. Windows: the window loses its frame
/// and covers the whole monitor, so the taskbar is hidden. Android: the status
/// and navigation bars are hidden (swipe from an edge to see them briefly).
/// Linux: the app only hides its own toolbar.
class AppWindow {
  static const _channel = MethodChannel('rdpv/window');

  static Future<void> setFullScreen(bool on) async {
    if (kIsWeb) return;
    switch (defaultTargetPlatform) {
      case TargetPlatform.windows:
        try {
          await _channel.invokeMethod<void>('setFullScreen', on);
        } on PlatformException {
          // The window stays as it is; the toolbar is still hidden.
        } on MissingPluginException {
          // Not available (e.g. in tests).
        }
      case TargetPlatform.android:
        await SystemChrome.setEnabledSystemUIMode(
            on ? SystemUiMode.immersiveSticky : SystemUiMode.edgeToEdge);
      default:
        break;
    }
  }
}
