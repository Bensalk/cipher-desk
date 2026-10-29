import 'package:flutter/material.dart';

/// App-wide settings. Pure Dart; saving to disk is done by `SettingsPersistence`
/// (lib/state/persistence.dart), which listens for changes.
class AppSettings extends ChangeNotifier {
  ThemeMode _themeMode = ThemeMode.system;
  bool _blockScreenshots = true; // S10: on by default
  bool _clipboardDefault = false; // S3/S6: off by default
  bool _confirmSavedPassword = true; // S5/S13: on by default
  int _idleMinutes = 30; // S7; 0 = never

  ThemeMode get themeMode => _themeMode;
  bool get blockScreenshots => _blockScreenshots;
  bool get clipboardDefault => _clipboardDefault;
  bool get confirmSavedPassword => _confirmSavedPassword;
  int get idleMinutes => _idleMinutes;

  /// Sets every value at once without notifying (used when loading from disk).
  void load({
    required ThemeMode themeMode,
    required bool blockScreenshots,
    required bool clipboardDefault,
    required bool confirmSavedPassword,
    required int idleMinutes,
  }) {
    _themeMode = themeMode;
    _blockScreenshots = blockScreenshots;
    _clipboardDefault = clipboardDefault;
    _confirmSavedPassword = confirmSavedPassword;
    _idleMinutes = idleMinutes;
  }

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

  set confirmSavedPassword(bool value) {
    if (value == _confirmSavedPassword) return;
    _confirmSavedPassword = value;
    notifyListeners();
  }

  set idleMinutes(int value) {
    if (value == _idleMinutes || value < 0) return;
    _idleMinutes = value;
    notifyListeners();
  }
}
