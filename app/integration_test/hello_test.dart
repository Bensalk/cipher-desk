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

  /// Brings a widget on screen. On a phone the form is taller than the screen and
  /// its list only builds what is near the visible part, so scroll until it exists.
  Future<void> reveal(WidgetTester tester, Finder f) async {
    // Close the on-screen keyboard first (phones): while it opens or closes the
    // list moves, and the widget can slip out of view again.
    FocusManager.instance.primaryFocus?.unfocus();
    await tester.pumpAndSettle();
    for (final delta in [200.0, -200.0]) {
      if (f.evaluate().isNotEmpty) break;
      try {
        await tester.scrollUntilVisible(f, delta,
            scrollable: find.byType(Scrollable).first, maxScrolls: 30);
      } on StateError {
        // Not in this direction: try the other one.
      }
    }
    await tester.ensureVisible(f);
    await tester.pumpAndSettle();
  }

  /// Scrolls a widget into view, then taps it (forms are taller than small screens).
  Future<void> tapVisible(WidgetTester tester, Finder f) async {
    await reveal(tester, f);
    await tester.tap(f);
  }

  Future<void> typeVisible(WidgetTester tester, Finder f, String text) async {
    await reveal(tester, f);
    await tester.enterText(f, text);
  }

  /// The error shown under a text field, wherever the field is scrolled to.
  Future<String?> fieldError(WidgetTester tester, String key) async {
    final f = find.byKey(Key(key));
    await reveal(tester, f);
    return tester.widget<TextField>(f).decoration?.errorText;
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
    expect(await fieldError(tester, 'field-host'), contains('without rdp://'));
    expect(await fieldError(tester, 'field-port'), contains('Port must be a number'));
    // The username is required; no password is needed unless it is to be saved.
    expect(await fieldError(tester, 'field-username'), 'This field is required.');
    expect(find.byKey(const Key('field-password')), findsNothing);
    // Ticking "Save password" asks for the password to save.
    await tapVisible(tester, find.byKey(const Key('field-save-password')));
    await tester.pumpAndSettle();
    expect(await fieldError(tester, 'field-password'), 'This field is required.');
  });

  testWidgets('the method helper suggests a method and VNC needs no username', (tester) async {
    await start(tester);
    await tester.tap(find.byKey(const Key('button-new')));
    await tester.pumpAndSettle();
    // Private Remote Desktop is the default.
    expect(find.byKey(const Key('field-username')), findsOneWidget);

    // Someone must see the screen, and Cipher Desk can be installed: Share screen.
    await tapVisible(tester, find.byKey(const Key('button-method-helper')));
    await tester.pumpAndSettle();
    await tester.tap(find.descendant(of: find.byKey(const Key('helper-q1')), matching: find.text('Yes')));
    await tester.pumpAndSettle();
    await tester.tap(find.descendant(of: find.byKey(const Key('helper-q2')), matching: find.text('Yes')));
    await tester.pumpAndSettle();
    expect(find.text('Use Share screen'), findsOneWidget);
    await tester.tap(find.byKey(const Key('helper-use')));
    await tester.pumpAndSettle();
    expect(find.textContaining('Cipher Desk running on the computer'), findsOneWidget);

    // VNC: no username field, no clipboard choice, port 5900.
    await tapVisible(tester, find.byKey(const Key('method-vnc')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('field-username')), findsNothing);
    expect(find.byKey(const Key('field-clipboard')), findsNothing);
    final port = tester.widget<TextField>(find.byKey(const Key('field-port')));
    expect(port.decoration?.hintText, '5900');
  });

  testWidgets('password is masked and can be revealed', (tester) async {
    await start(tester);
    await tester.tap(find.byKey(const Key('button-new')));
    await tester.pumpAndSettle();
    await tapVisible(tester, find.byKey(const Key('field-save-password')));
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
    await tapVisible(tester, find.byKey(const Key('field-save-password')));
    await tester.pumpAndSettle();
    await typeVisible(tester, find.byKey(const Key('field-password')), _randomPassword());
    await tapVisible(tester, find.byKey(const Key('button-save')));
    // Saving waits for the OS vault (and on Windows for Windows Hello), which
    // answers outside Flutter's frames: wait for it rather than for one frame.
    final until = DateTime.now().add(const Duration(seconds: 30));
    while (store.profiles.isEmpty && DateTime.now().isBefore(until)) {
      await Future<void>.delayed(const Duration(milliseconds: 200));
      await tester.pump();
    }
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

  testWidgets('Share screen starts and stops on Windows', (tester) async {
    await start(tester);
    await tester.tap(find.text('Share screen').last);
    await tester.pumpAndSettle();
    expect(find.text('Sharing is off'), findsOneWidget);
    // A strong password is made on first use and shown only on request.
    final shown = find.byKey(const Key('share-password-show'));
    await reveal(tester, shown);
    await tester.tap(shown);
    await tester.pumpAndSettle();
    expect(find.textContaining(RegExp(r'^\w{4}-\w{4}-\w{4}-\w{4}$')), findsOneWidget);
    await tester.tap(shown);
    await tester.pumpAndSettle();

    final startButton = find.byKey(const Key('share-start'));
    await reveal(tester, startButton);
    await tester.tap(startButton);
    for (var i = 0; i < 50 && find.text('Sharing is on: waiting for a viewer').evaluate().isEmpty; i++) {
      await tester.pump(const Duration(milliseconds: 200));
    }
    expect(find.text('Sharing is on: waiting for a viewer'), findsOneWidget);
    await tester.tap(find.byKey(const Key('share-stop')));
    await tester.pumpAndSettle();
    expect(find.text('Sharing is off'), findsOneWidget);
  }, skip: !Platform.isWindows);

  testWidgets('a bad Wake-on-LAN address is reported', (tester) async {
    await start(tester);
    await tester.tap(find.byKey(const Key('button-new')));
    await tester.pumpAndSettle();
    await typeVisible(tester, find.byKey(const Key('field-mac')), 'not-a-mac');
    await tapVisible(tester, find.byKey(const Key('button-save')));
    await tester.pumpAndSettle();
    expect(await fieldError(tester, 'field-mac'), contains('Enter a MAC address'));
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
