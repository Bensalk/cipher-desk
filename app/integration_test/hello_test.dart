import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:secure_rdp_viewer/main.dart';
import 'package:secure_rdp_viewer/src/rust/frb_generated.dart';

/// Stage 2 acceptance: the UI loads the Rust core and shows its greeting
/// and the secure defaults. Runs on Linux (CI, under Xvfb), Windows and Android.
void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  setUpAll(() async => RustLib.init());

  testWidgets('Rust core is reachable from Flutter', (tester) async {
    await tester.pumpWidget(const SecureRdpViewerApp());
    await tester.pumpAndSettle();

    final hello = tester.widget<Text>(find.byKey(const Key('hello-from-rust')));
    expect(hello.data, startsWith('Hello from Rust'));
    expect(hello.data, contains('IronRDP'));
    expect(find.textContaining('NLA required: true'), findsOneWidget);
    expect(find.textContaining('Clipboard: Off'), findsOneWidget);
  });
}
