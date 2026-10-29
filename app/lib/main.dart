import 'package:flutter/material.dart';

import 'platform/secure_window.dart';
import 'src/rust/frb_generated.dart';
import 'state/persistence.dart';
import 'state/profiles.dart';
import 'state/settings.dart';
import 'ui/home_screen.dart';

/// Lets start-up code show messages before any screen exists.
final messengerKey = GlobalKey<ScaffoldMessengerState>();

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await RustLib.init();
  final paths = await AppPaths.resolve();
  void saveFailed() => messengerKey.currentState?.showSnackBar(const SnackBar(
        content: Text("Your changes couldn't be saved to this device's storage."),
      ));
  final settings = AppSettings();
  SettingsPersistence(paths, settings, onSaveFailed: saveFailed).start();
  final store = ProfileStore();
  final notice = ProfilePersistence(paths, store, onSaveFailed: saveFailed).start();
  await SecureWindow.setEnabled(settings.blockScreenshots);
  runApp(SecureRdpViewerApp(store: store, settings: settings, paths: paths, startupNotice: notice));
}

class SecureRdpViewerApp extends StatelessWidget {
  const SecureRdpViewerApp({
    super.key,
    required this.store,
    required this.settings,
    this.paths,
    this.startupNotice,
  });

  final ProfileStore store;
  final AppSettings settings;

  /// App data paths; null in tests that do not touch the disk.
  final AppPaths? paths;
  final LoadNotice? startupNotice;

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: settings,
      builder: (context, _) => MaterialApp(
        scaffoldMessengerKey: messengerKey,
        title: 'Secure RDP Viewer',
        debugShowCheckedModeBanner: false,
        theme: ThemeData(colorSchemeSeed: Colors.teal, brightness: Brightness.light),
        darkTheme: ThemeData(colorSchemeSeed: Colors.teal, brightness: Brightness.dark),
        themeMode: settings.themeMode,
        home: HomeScreen(
          store: store,
          settings: settings,
          paths: paths,
          startupNotice: startupNotice?.message,
        ),
      ),
    );
  }
}
