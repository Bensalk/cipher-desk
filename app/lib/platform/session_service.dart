import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

/// Android only: a foreground service with a notification keeps the session
/// open while the app is in the background (decision 8). Elsewhere it does
/// nothing: desktop sessions keep running when the window is minimised.
class SessionService {
  static const _channel = MethodChannel('rdpv/session_service');

  /// How long a session may stay open in the background before it is ended.
  static const backgroundGrace = Duration(minutes: 5);

  static bool get isAndroid => !kIsWeb && defaultTargetPlatform == TargetPlatform.android;

  static Future<void> start() async {
    if (!isAndroid) return;
    try {
      await _channel.invokeMethod<bool>('start');
    } on PlatformException {
      // Without the service the session still works while the app is open.
    }
  }

  static Future<void> stop() async {
    if (!isAndroid) return;
    try {
      await _channel.invokeMethod<void>('stop');
    } on PlatformException {
      // Nothing to do.
    }
  }
}
