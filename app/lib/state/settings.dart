import 'package:flutter/material.dart';

/// App-wide settings (Stage 3 keeps them in memory; persisted in Stage 7).
class AppSettings extends ChangeNotifier {
  ThemeMode _themeMode = ThemeMode.system;
  bool _blockScreenshots = true; // S10: on by default
  bool _clipboardDefault = false; // S3/S6: off by default

  ThemeMode get themeMode => _themeMode;
  bool get blockScreenshots => _blockScreenshots;
  bool get clipboardDefault => _clipboardDefault;

  set themeMode(ThemeMode value) {
    if (value == _themeMode) return;
    _themeMode = value;
    notifyListeners();
  }

  set blockScreenshots(bool value) {
    if (value == _blockScreenshots) return;
    _blockScreenshots = value;
    notifyListeners();
  }

  set clipboardDefault(bool value) {
    if (value == _clipboardDefault) return;
    _clipboardDefault = value;
    notifyListeners();
  }
}
