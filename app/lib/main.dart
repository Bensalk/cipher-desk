import 'package:flutter/material.dart';

import 'platform/secure_window.dart';
import 'src/rust/frb_generated.dart';
import 'state/profiles.dart';
import 'state/settings.dart';
import 'ui/home_screen.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await RustLib.init();
  final settings = AppSettings();
  await SecureWindow.setEnabled(settings.blockScreenshots);
  runApp(SecureRdpViewerApp(store: ProfileStore(), settings: settings));
}

class SecureRdpViewerApp extends StatelessWidget {
  const SecureRdpViewerApp({super.key, required this.store, required this.settings});

  final ProfileStore store;
  final AppSettings settings;

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: settings,
      builder: (context, _) => MaterialApp(
        title: 'Secure RDP Viewer',
        debugShowCheckedModeBanner: false,
        theme: ThemeData(colorSchemeSeed: Colors.teal, brightness: Brightness.light),
        darkTheme: ThemeData(colorSchemeSeed: Colors.teal, brightness: Brightness.dark),
        themeMode: settings.themeMode,
        home: HomeScreen(store: store, settings: settings),
      ),
    );
  }
}
