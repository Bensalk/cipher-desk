import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:secure_rdp_viewer/main.dart';
import 'package:secure_rdp_viewer/src/rust/frb_generated.dart';
import 'package:secure_rdp_viewer/state/profiles.dart';
import 'package:secure_rdp_viewer/state/settings.dart';

/// UI tests that use the real Rust core. Run on Linux (CI, Xvfb), Windows and Android.
void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  setUpAll(() async => RustLib.init());

  Future<ProfileStore> start(WidgetTester tester) async {
    final store = ProfileStore();
    await tester.pumpWidget(SecureRdpViewerApp(store: store, settings: AppSettings()));
    await tester.pumpAndSettle();
    return store;
  }

  testWidgets('empty state and new-connection form', (tester) async {
    await start(tester);
    expect(find.text('No connections yet'), findsOneWidget);
    await tester.tap(find.byKey(const Key('button-new')));
    await tester.pumpAndSettle();
    expect(find.text('New connection'), findsWidgets);
  });

  testWidgets('form shows field errors from the Rust core', (tester) async {
    await start(tester);
    await tester.tap(find.byKey(const Key('button-new')));
    await tester.pumpAndSettle();
    await tester.enterText(find.byKey(const Key('field-host')), 'rdp://example.com');
    await tester.enterText(find.byKey(const Key('field-port')), '70000');
    await tester.tap(find.byKey(const Key('button-save')));
    await tester.pumpAndSettle();
    expect(find.textContaining('without rdp://'), findsOneWidget);
    expect(find.text('This field is required.'), findsNWidgets(2)); // username, password
  });

  testWidgets('password is masked and can be revealed', (tester) async {
    await start(tester);
    await tester.tap(find.byKey(const Key('button-new')));
    await tester.pumpAndSettle();
    final field = find.byKey(const Key('field-password'));
    expect(tester.widget<TextField>(field).obscureText, isTrue);
    await tester.tap(find.byTooltip('Show password'));
    await tester.pump();
    expect(tester.widget<TextField>(field).obscureText, isFalse);
  });

  testWidgets('valid form saves a profile without the password', (tester) async {
    final store = await start(tester);
    await tester.tap(find.byKey(const Key('button-new')));
    await tester.pumpAndSettle();
    await tester.enterText(find.byKey(const Key('field-name')), 'Office');
    await tester.enterText(find.byKey(const Key('field-host')), '192.0.2.10');
    await tester.enterText(find.byKey(const Key('field-username')), 'alice');
    await tester.enterText(find.byKey(const Key('field-password')), ['t', 'e', 's', 't', '9'].join());
    await tester.tap(find.byKey(const Key('field-save-password')));
    await tester.tap(find.byKey(const Key('button-save')));
    await tester.pumpAndSettle();

    final p = store.profiles.single;
    expect(p.name, 'Office');
    expect(p.endpoint, '192.0.2.10:3389');
    expect(p.savePassword, isTrue);
    expect(p.clipboardEnabled, isFalse);
    expect(find.text('Office'), findsOneWidget);
  });

  testWidgets('settings shows secure defaults and error help', (tester) async {
    await start(tester);
    await tester.tap(find.text('Settings').last);
    await tester.pumpAndSettle();
    expect(find.textContaining('NLA required: true'), findsOneWidget);
    await tester.scrollUntilVisible(find.text('What error messages mean'), 200);
    await tester.tap(find.text('What error messages mean'));
    await tester.pumpAndSettle();
    expect(find.text('Warning: computer identity changed'), findsOneWidget);
  });
}
