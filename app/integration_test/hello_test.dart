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

  /// Scrolls a widget into view, then taps it (forms are taller than small screens).
  Future<void> tapVisible(WidgetTester tester, Finder f) async {
    await tester.ensureVisible(f);
    await tester.pumpAndSettle();
    await tester.tap(f);
  }

  Future<void> typeVisible(WidgetTester tester, Finder f, String text) async {
    await tester.ensureVisible(f);
    await tester.enterText(f, text);
  }

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
    await typeVisible(tester, find.byKey(const Key('field-host')), 'rdp://example.com');
    await typeVisible(tester, find.byKey(const Key('field-port')), '70000');
    await tapVisible(tester, find.byKey(const Key('button-save')));
    await tester.pumpAndSettle();
    expect(find.textContaining('without rdp://'), findsOneWidget);
    expect(find.textContaining('Port must be a number'), findsOneWidget);
    expect(find.text('This field is required.'), findsNWidgets(2)); // username, password
  });

  testWidgets('password is masked and can be revealed', (tester) async {
    await start(tester);
    await tester.tap(find.byKey(const Key('button-new')));
    await tester.pumpAndSettle();
    final field = find.byKey(const Key('field-password'));
    expect(tester.widget<TextField>(field).obscureText, isTrue);
    await tapVisible(tester, find.byTooltip('Show password'));
    await tester.pump();
    expect(tester.widget<TextField>(field).obscureText, isFalse);
  });

  testWidgets('valid form saves a profile without the password', (tester) async {
    final store = await start(tester);
    await tester.tap(find.byKey(const Key('button-new')));
    await tester.pumpAndSettle();
    await typeVisible(tester, find.byKey(const Key('field-name')), 'Office');
    await typeVisible(tester, find.byKey(const Key('field-host')), '192.0.2.10');
    await typeVisible(tester, find.byKey(const Key('field-username')), 'alice');
    await typeVisible(tester, find.byKey(const Key('field-password')), ['t', 'e', 's', 't', '9'].join());
    await tapVisible(tester, find.byKey(const Key('field-save-password')));
    await tapVisible(tester, find.byKey(const Key('button-save')));
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
    final warning = find.text('Warning: computer identity changed');
    await tester.scrollUntilVisible(warning, 200, scrollable: find.byType(Scrollable).last);
    expect(warning, findsOneWidget);
  });
}
