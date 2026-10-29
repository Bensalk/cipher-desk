import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

/// Puts text from the remote computer on this device's clipboard.
///
/// On Android it is marked as sensitive (Android 13+ then hides it from
/// clipboard previews and keyboard suggestions, S6). Elsewhere the normal
/// clipboard is used.
class RemoteClipboard {
  static const _channel = MethodChannel('rdpv/clipboard');

  static Future<void> setText(String text) async {
    if (!kIsWeb && defaultTargetPlatform == TargetPlatform.android) {
      try {
        await _channel.invokeMethod<void>('setSensitiveText', text);
        return;
      } on PlatformException {
        // Fall through to the normal clipboard.
      } on MissingPluginException {
        // Fall through (tests).
      }
    }
    await Clipboard.setData(ClipboardData(text: text));
  }

  /// Reads this device's clipboard text, or null if there is none.
  static Future<String?> readText() async {
    final data = await Clipboard.getData(Clipboard.kTextPlain);
    final t = data?.text;
    return (t == null || t.isEmpty) ? null : t;
  }
}
