import 'package:flutter/material.dart';
import 'package:secure_rdp_viewer/src/rust/api/info.dart';
import 'package:secure_rdp_viewer/src/rust/frb_generated.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await RustLib.init();
  runApp(const SecureRdpViewerApp());
}

/// Stage 2 start screen: proves the Flutter UI can call the Rust core on
/// every platform. The real connection UI arrives in Stage 3.
class SecureRdpViewerApp extends StatelessWidget {
  const SecureRdpViewerApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Secure RDP Viewer',
      theme: ThemeData(
        colorSchemeSeed: Colors.teal,
        brightness: Brightness.light,
      ),
      darkTheme: ThemeData(
        colorSchemeSeed: Colors.teal,
        brightness: Brightness.dark,
      ),
      themeMode: ThemeMode.system,
      home: const FoundationScreen(),
    );
  }
}

class FoundationScreen extends StatelessWidget {
  const FoundationScreen({super.key});

  @override
  Widget build(BuildContext context) {
    final defaults = securityDefaultsSummary();
    return Scaffold(
      appBar: AppBar(title: const Text('Secure RDP Viewer')),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          Text(
            helloFromRust(),
            key: const Key('hello-from-rust'),
            style: Theme.of(context).textTheme.titleMedium,
          ),
          const SizedBox(height: 16),
          Text(
            'Security defaults',
            style: Theme.of(context).textTheme.titleSmall,
          ),
          for (final line in defaults)
            ListTile(
              dense: true,
              leading: const Icon(Icons.verified_user_outlined),
              title: Text(line),
            ),
        ],
      ),
    );
  }
}
