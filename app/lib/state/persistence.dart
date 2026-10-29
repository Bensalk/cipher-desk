import 'dart:io';

import 'package:flutter/material.dart';
import 'package:path_provider/path_provider.dart';

import '../src/rust/api/store.dart';
import 'profiles.dart';
import 'settings.dart';

/// Where the app keeps its private files (no passwords in any of them).
class AppPaths {
  AppPaths(this.dir);

  final String dir;

  String get connections => '$dir${Platform.pathSeparator}connections.json';
  String get settings => '$dir${Platform.pathSeparator}settings.json';
  String get pins => '$dir${Platform.pathSeparator}certificate_pins.json';

  static Future<AppPaths> resolve() async =>
      AppPaths((await getApplicationSupportDirectory()).path);
}

/// What happened when the saved connections were loaded, for a startup notice.
class LoadNotice {
  const LoadNotice(this.message);
  final String message;
}

/// Loads connections into [store] and saves them whenever they change.
class ProfilePersistence {
  ProfilePersistence(this.paths, this.store, {this.onSaveFailed});

  final AppPaths paths;
  final ProfileStore store;

  /// Called when a change could not be written to disk.
  final void Function()? onSaveFailed;

  /// Loads from disk and starts saving changes. Returns a notice if the user
  /// should be told something (damaged file, skipped entries).
  LoadNotice? start() {
    final r = connectionsLoad(path: paths.connections);
    store.replaceAll(
      [for (final c in r.connections) fromData(c)],
      [
        for (final h in r.history)
          HistoryItem(
            connectionId: h.connectionId,
            time: DateTime.fromMillisecondsSinceEpoch(h.time * 1000),
            outcome: h.outcome,
          ),
      ],
    );
    if (r.readError) {
      // The file exists but could not be read (for example, permissions). Saving
      // now would overwrite it with an empty list, so nothing is saved this time.
      return const LoadNotice("Saved connections couldn't be read. To protect them, changes "
          "made now won't be saved. Restart the app to try again.");
    }
    store.addListener(_saveOrReport);
    if (r.damagedAndReset) {
      return const LoadNotice('The saved connections file was damaged. It was kept aside and '
          'the list was started again.');
    }
    if (r.restoredFromBackup) {
      return const LoadNotice('The saved connections file was damaged; the previous version was restored.');
    }
    if (r.skipped > 0) {
      return LoadNotice('${r.skipped} saved connection(s) were invalid and were skipped.');
    }
    return null;
  }

  void _saveOrReport() {
    if (!save()) onSaveFailed?.call();
  }

  bool save() => connectionsSave(
        path: paths.connections,
        connections: [for (final p in store.storageList) toData(p)],
        history: [
          for (final h in store.storageHistory)
            HistoryData(
              connectionId: h.connectionId,
              time: h.time.millisecondsSinceEpoch ~/ 1000,
              outcome: h.outcome,
            ),
        ],
      );

  static ConnectionData toData(ConnectionProfile p) => ConnectionData(
        id: p.id,
        name: p.name,
        host: p.host,
        port: p.port,
        username: p.username,
        savePassword: p.savePassword,
        clipboard: p.clipboardMode,
        lastUsed: p.lastUsed == null ? null : p.lastUsed!.millisecondsSinceEpoch ~/ 1000,
        wakeMac: p.wakeMac,
      );

  static ConnectionProfile fromData(ConnectionData c) => ConnectionProfile(
        id: c.id,
        name: c.name,
        host: c.host,
        port: c.port,
        username: c.username,
        savePassword: c.savePassword,
        clipboardMode: c.clipboard,
        lastUsed: c.lastUsed == null ? null : DateTime.fromMillisecondsSinceEpoch(c.lastUsed! * 1000),
        wakeMac: c.wakeMac,
      );
}

/// Loads settings into [settings] and saves them whenever they change.
class SettingsPersistence {
  SettingsPersistence(this.paths, this.settings, {this.onSaveFailed});

  final AppPaths paths;
  final AppSettings settings;
  final void Function()? onSaveFailed;

  void start() {
    final s = settingsLoad(path: paths.settings);
    settings.load(
      themeMode: switch (s.theme) {
        AppTheme.light => ThemeMode.light,
        AppTheme.dark => ThemeMode.dark,
        AppTheme.system => ThemeMode.system,
      },
      blockScreenshots: s.blockScreenshots,
      clipboardDefault: s.clipboardDefault,
      confirmSavedPassword: s.confirmSavedPassword,
      idleMinutes: s.idleMinutes,
    );
    settings.addListener(() {
      if (!save()) onSaveFailed?.call();
    });
  }

  bool save() => settingsSave(
        path: paths.settings,
        data: SettingsData(
          theme: switch (settings.themeMode) {
            ThemeMode.light => AppTheme.light,
            ThemeMode.dark => AppTheme.dark,
            ThemeMode.system => AppTheme.system,
          },
          blockScreenshots: settings.blockScreenshots,
          clipboardDefault: settings.clipboardDefault,
          confirmSavedPassword: settings.confirmSavedPassword,
          idleMinutes: settings.idleMinutes,
        ),
      );
}
