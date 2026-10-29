import 'dart:io';
import 'dart:math';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:cipher_desk/main.dart';
import 'package:cipher_desk/platform/session_service.dart';
import 'package:cipher_desk/src/rust/api/errors.dart';
import 'package:cipher_desk/src/rust/frb_generated.dart';
import 'package:cipher_desk/state/persistence.dart';
import 'package:cipher_desk/state/profiles.dart';
import 'package:cipher_desk/state/settings.dart';

/// A throwaway password, different on every run (SA-9: no fixed test passwords).
String _randomPassword() {
  final r = Random.secure();
  return List.generate(16, (_) => r.nextInt(36).toRadixString(36)).join();
}

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
    await tester.pumpWidget(CipherDeskApp(store: store, settings: AppSettings()));
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
    await typeVisible(tester, find.byKey(const Key('field-password')), _randomPassword());
    await tapVisible(tester, find.byKey(const Key('field-save-password')));
    await tapVisible(tester, find.byKey(const Key('button-save')));
    await tester.pumpAndSettle();

    final p = store.profiles.single;
    expect(p.name, 'Office');
    expect(p.endpoint, '192.0.2.10:3389');
    // Saving needs the OS vault (and Windows Hello on Windows). Test machines may
    // have neither: then the profile must say "not saved" and tell the user why,
    // never pretend the password was kept.
    final notSavedMessage = find.textContaining('Password not saved');
    if (p.savePassword) {
      expect(notSavedMessage, findsNothing);
    } else {
      expect(notSavedMessage, findsOneWidget);
    }
    expect(p.clipboardEnabled, isFalse);
    expect(find.text('Office'), findsOneWidget);
  });

  testWidgets('settings shows secure defaults and error help', (tester) async {
    await start(tester);
    await tester.tap(find.text('Settings').last);
    await tester.pumpAndSettle();
    await tester.scrollUntilVisible(find.textContaining('NLA required: true'), 200);
    expect(find.textContaining('NLA required: true'), findsOneWidget);
    await tester.scrollUntilVisible(find.text('What error messages mean'), 200);
    await tester.tap(find.text('What error messages mean'));
    await tester.pumpAndSettle();
    final warning = find.text('Warning: computer identity changed');
    await tester.scrollUntilVisible(warning, 200, scrollable: find.byType(Scrollable).last);
    expect(warning, findsOneWidget);
  });

  testWidgets('creator credit is the last item in About', (tester) async {
    await start(tester);
    await tester.tap(find.text('Settings').last);
    await tester.pumpAndSettle();
    final credit = find.byKey(const Key('creator'));
    await tester.scrollUntilVisible(credit, 200);
    expect(find.text('Created by Bensal'), findsOneWidget);
  });

  testWidgets('a bad Wake-on-LAN address is reported', (tester) async {
    await start(tester);
    await tester.tap(find.byKey(const Key('button-new')));
    await tester.pumpAndSettle();
    await typeVisible(tester, find.byKey(const Key('field-mac')), 'not-a-mac');
    await tapVisible(tester, find.byKey(const Key('button-save')));
    await tester.pumpAndSettle();
    expect(find.textContaining('Enter a MAC address'), findsOneWidget);
  });

  test('connections and settings survive a restart, without passwords', () async {
    final dir = await Directory.systemTemp.createTemp('rdpv-it-');
    addTearDown(() => dir.delete(recursive: true));
    final paths = AppPaths(dir.path);

    final store = ProfileStore();
    expect(ProfilePersistence(paths, store).start(), isNull);
    final p = ConnectionProfile(
      id: store.newId(),
      name: 'Office',
      host: '192.0.2.10',
      port: 3389,
      username: 'alice',
      wakeMac: '00:11:22:33:44:55',
    );
    store.upsert(p);
    store.addHistory(p.id, 'WrongCredentials', DateTime(2026, 10, 29, 9));
    final settings = AppSettings();
    SettingsPersistence(paths, settings).start();
    settings.idleMinutes = 60;

    final store2 = ProfileStore();
    expect(ProfilePersistence(paths, store2).start(), isNull);
    expect(store2.profiles.single.name, 'Office');
    expect(store2.profiles.single.wakeMac, '00:11:22:33:44:55');
    expect(store2.history.single.outcome, 'WrongCredentials');
    final settings2 = AppSettings();
    SettingsPersistence(paths, settings2).start();
    expect(settings2.idleMinutes, 60);

    final text = await File(paths.connections).readAsString();
    expect(text, isNot(contains('"password"')));
  });

  test('a damaged connections file gives a start-up notice', () async {
    final dir = await Directory.systemTemp.createTemp('rdpv-it-');
    addTearDown(() => dir.delete(recursive: true));
    final paths = AppPaths(dir.path);
    await File(paths.connections).writeAsString('{ not json');
    final notice = ProfilePersistence(paths, ProfileStore()).start();
    expect(notice, isNotNull);
  });

  test('background grace period matches the message shown to the user', () {
    final e = allConnectionErrors().firstWhere((e) => e.id == 'AppInBackground');
    final minutes = SessionService.backgroundGrace.inMinutes;
    expect(e.advice, contains('$minutes minutes'));
  });
}
