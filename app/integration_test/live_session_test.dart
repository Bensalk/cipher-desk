// A real remote desktop session from the app, on any platform (CI runs it on an
// Android emulator against the local test host). Skipped unless the host is given:
//   flutter test integration_test/live_session_test.dart -d <device> \
//     --dart-define-from-file=<private JSON file with RDPV_HOST, RDPV_PORT, RDPV_USER, RDPV_PASSWORD>
// A file, never a command line, carries the password (a random test-host password
// created for each CI run).

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:cipher_desk/main.dart';
import 'package:cipher_desk/src/rust/frb_generated.dart';
import 'package:cipher_desk/state/persistence.dart';
import 'package:cipher_desk/state/profiles.dart';
import 'package:cipher_desk/state/settings.dart';

const host = String.fromEnvironment('RDPV_HOST');
const port = int.fromEnvironment('RDPV_PORT', defaultValue: 3389);
const user = String.fromEnvironment('RDPV_USER');
const password = String.fromEnvironment('RDPV_PASSWORD');
// CI sets this, so a missing host makes the test fail instead of being skipped.
const requireLive = bool.fromEnvironment('RDPV_REQUIRE_LIVE');

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  setUpAll(() async => RustLib.init());

  Future<void> pumpUntil(WidgetTester t, Finder f, {int seconds = 60}) async {
    final end = DateTime.now().add(Duration(seconds: seconds));
    while (f.evaluate().isEmpty) {
      if (DateTime.now().isAfter(end)) throw TestFailure('Timed out waiting for $f');
      await Future<void>.delayed(const Duration(milliseconds: 200));
      await t.pump();
    }
  }

  testWidgets('connects, checks the certificate, shows the desktop and disconnects',
      skip: !requireLive && (host.isEmpty || password.isEmpty), (t) async {
    if (host.isEmpty || password.isEmpty) fail('RDPV_HOST and RDPV_PASSWORD must be set');
    final dir = await Directory.systemTemp.createTemp('cipher-desk-live-');
    addTearDown(() => dir.delete(recursive: true));
    final store = ProfileStore()
      ..upsert(const ConnectionProfile(
        id: 'live-test',
        name: 'Test computer',
        host: host,
        port: port,
        username: user,
      ));
    await t.pumpWidget(CipherDeskApp(store: store, settings: AppSettings(), paths: AppPaths(dir.path)));
    await t.pumpAndSettle();

    // Connect: type the password, then trust the certificate on first use.
    await t.tap(find.text('Test computer'));
    await t.pumpAndSettle();
    await t.enterText(find.byKey(const Key('connect-password')), password);
    await t.tap(find.byKey(const Key('button-connect')));
    await pumpUntil(t, find.byKey(const Key('certificate-fingerprint')));
    await t.tap(find.byKey(const Key('button-trust')));

    // The remote screen appears and the status says Connected.
    await pumpUntil(t, find.byType(RawImage));
    await pumpUntil(t, find.text('Connected'));
    expect(find.byKey(const Key('button-disconnect')), findsOneWidget);

    // Touch input reaches the session without errors: a tap and the key bar.
    await t.tap(find.byType(RawImage), warnIfMissed: false);
    await t.pump();

    // Disconnect cleanly.
    await t.tap(find.byKey(const Key('button-disconnect')));
    await pumpUntil(t, find.text('Back to connections'), seconds: 30);
    expect(store.history.isNotEmpty, isTrue);
  });
}
