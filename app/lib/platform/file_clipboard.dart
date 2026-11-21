import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

/// Files on this computer's clipboard (what Explorer copies and pastes), for
/// copy and paste of files in a remote session. Windows only;
/// elsewhere it reports no files and does nothing.
class FileClipboard {
  static const _channel = MethodChannel('rdpv/file_clipboard');

  static bool get supported => !kIsWeb && defaultTargetPlatform == TargetPlatform.windows;

  /// A number Windows changes on every clipboard change (null if unknown).
  static Future<int?> sequence() async {
    if (!supported) return null;
    try {
      return await _channel.invokeMethod<int>('sequence');
    } on PlatformException {
      return null;
    } on MissingPluginException {
      return null;
    }
  }

  /// Paths of the files copied on this computer (empty if none).
  static Future<List<String>> readFiles() async {
    if (!supported) return const [];
    try {
      final list = await _channel.invokeListMethod<String>('readFiles');
      return list ?? const [];
    } on PlatformException {
      return const [];
    } on MissingPluginException {
      return const [];
    }
  }

  /// Puts these files on the clipboard, so Ctrl+V in Explorer copies them.
  /// Returns the clipboard's change number right after (null on failure).
  static Future<int?> writeFiles(List<String> paths) async {
    if (!supported || paths.isEmpty) return null;
    try {
      return await _channel.invokeMethod<int>('writeFiles', paths);
    } on PlatformException {
      return null;
    } on MissingPluginException {
      return null;
    }
  }
}
