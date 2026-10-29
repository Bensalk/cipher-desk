// README screenshots, taken from the real app (Rust core included).
//
// Skipped unless SHOT_DIR is given, so the normal CI integration run passes over it:
//   xvfb-run -a flutter test integration_test/screenshots/screenshots_test.dart -d linux \
//     --dart-define=SHOT_DIR=$PWD/shots
//
// Only sample data is used: example.com, documentation addresses (192.0.2.x,
// 198.51.100.x) and made-up user names. The live session shots connect to the
// local test host started by scripts/start-test-hosts.sh (RDPV_TEST_* variables,
// random password per run); they are skipped when that host is not running.
// The test host's password is typed into the dialog but never captured.

import 'dart:io';
import 'dart:math';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:cipher_desk/main.dart';
import 'package:cipher_desk/src/rust/api/session.dart' show ClipboardMode;
import 'package:cipher_desk/src/rust/frb_generated.dart';
import 'package:cipher_desk/state/persistence.dart';
import 'package:cipher_desk/state/profiles.dart';
import 'package:cipher_desk/state/settings.dart';

const shotDir = String.fromEnvironment('SHOT_DIR');

/// Logical window size and the pixel ratio the PNG is rendered at.
const devices = <String, (Size, double)>{
  'phone': (Size(412, 915), 2.0),
  'tablet': (Size(800, 1280), 1.5),
  'desktop': (Size(1280, 800), 1.5),
};

final _boundary = GlobalKey();

String _throwaway() {
  final r = Random.secure();
  return List.generate(12, (_) => r.nextInt(36).toRadixString(36)).join();
}

void main() {
  final binding = IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  setUpAll(() async => RustLib.init());

  final env = Platform.environment;
  final liveHost = env['RDPV_TEST_HOST'];
  final livePort = int.tryParse(env['RDPV_TEST_PORT'] ?? '');
  final liveUser = env['RDPV_TEST_USER'];
  final livePassword = env['RDPV_TEST_PASSWORD'];
  final haveLive = liveHost != null && livePort != null && liveUser != null && livePassword != null;

  Future<void> pumpFor(WidgetTester t, Duration d) async {
    final end = DateTime.now().add(d);
    while (DateTime.now().isBefore(end)) {
      await Future<void>.delayed(const Duration(milliseconds: 100));
      await t.pump();
    }
  }

  Future<void> pumpUntil(WidgetTester t, Finder f, {int seconds = 30}) async {
    final end = DateTime.now().add(Duration(seconds: seconds));
    while (f.evaluate().isEmpty) {
      if (DateTime.now().isAfter(end)) throw TestFailure('Timed out waiting for $f');
      await Future<void>.delayed(const Duration(milliseconds: 100));
      await t.pump();
    }
  }

  Future<void> shot(WidgetTester t, String device, String name) async {
    await pumpFor(t, const Duration(milliseconds: 600)); // let ripples and fades finish
    final ratio = devices[device]!.$2;
    final boundary = _boundary.currentContext!.findRenderObject()! as RenderRepaintBoundary;
    final image = await boundary.toImage(pixelRatio: ratio);
    final png = await image.toByteData(format: ui.ImageByteFormat.png);
    image.dispose();
    final file = File('$shotDir/$device-$name.png');
    await file.writeAsBytes(png!.buffer.asUint8List());
    // ignore: avoid_print
    print('saved ${file.path}');
  }

  Future<(ProfileStore, AppSettings)> start(
    WidgetTester t,
    String device, {
    bool samples = true,
    ThemeMode theme = ThemeMode.light,
    List<ConnectionProfile> extra = const [],
  }) async {
    await binding.setSurfaceSize(devices[device]!.$1);
    addTearDown(() => binding.setSurfaceSize(null));
    final dir = await Directory.systemTemp.createTemp('cipher-desk-shots-');
    addTearDown(() => dir.delete(recursive: true));

    final store = ProfileStore();
    if (samples) {
      final office = ConnectionProfile(
        id: store.newId(),
        name: 'Office PC',
        host: '192.0.2.10',
        port: 3389,
        username: 'alice',
        savePassword: true,
        lastUsed: DateTime(2026, 10, 28, 9, 12),
      );
      final server = ConnectionProfile(
        id: store.newId(),
        name: 'Home server',
        host: 'homeserver.example.com',
        port: 3390,
        username: 'alice',
        clipboardMode: ClipboardMode.both,
        wakeMac: '00:11:22:33:44:55',
      );
      final lab = ConnectionProfile(
        id: store.newId(),
        name: 'Lab VM',
        host: '198.51.100.24',
        port: 3389,
        username: 'lab\\tester',
      );
      for (final p in [office, server, lab, ...extra]) {
        store.upsert(p);
      }
      store.addHistory(office.id, 'connected', DateTime(2026, 10, 28, 9, 12));
      store.addHistory(server.id, 'WrongCredentials', DateTime(2026, 10, 27, 20, 41));
      store.addHistory(lab.id, 'HostNotFound', DateTime(2026, 10, 26, 14, 5));
      store.addHistory(office.id, 'connected', DateTime(2026, 10, 25, 8, 58));
    }
    final settings = AppSettings()..themeMode = theme;
    await t.pumpWidget(RepaintBoundary(
      key: _boundary,
      child: CipherDeskApp(store: store, settings: settings, paths: AppPaths(dir.path)),
    ));
    await t.pumpAndSettle();
    return (store, settings);
  }

  Future<void> openSettings(WidgetTester t) async {
    await t.tap(find.text('Settings').last);
    await t.pumpAndSettle();
  }

  Future<void> back(WidgetTester t) async {
    await t.pageBack();
    await t.pumpAndSettle();
  }

  final skip = shotDir.isEmpty ? 'set --dart-define=SHOT_DIR=<folder> to take screenshots' : null;

  for (final device in devices.keys) {
    testWidgets('$device: connections, form, settings', skip: skip != null, (t) async {
      await Directory(shotDir).create(recursive: true);

      await start(t, device, samples: false);
      await shot(t, device, 'empty');

      await start(t, device);
      await shot(t, device, 'connections');

      // Connection menu.
      await t.tap(find.byTooltip('More').at(1));
      await t.pumpAndSettle();
      await shot(t, device, 'connection-menu');
      await t.tapAt(const Offset(5, 5)); // close the menu
      await t.pumpAndSettle();

      // Password prompt, with a throwaway value (shown masked).
      await t.tap(find.text('Lab VM'));
      await t.pumpAndSettle();
      await t.enterText(find.byKey(const Key('connect-password')), _throwaway());
      await t.pumpAndSettle();
      await shot(t, device, 'password');
      await t.tap(find.text('Cancel'));
      await t.pumpAndSettle();

      // New connection form, filled in.
      await t.tap(find.byKey(const Key('button-new')));
      await t.pumpAndSettle();
      await t.enterText(find.byKey(const Key('field-name')), 'Studio laptop');
      await t.enterText(find.byKey(const Key('field-host')), 'studio.example.com');
      await t.enterText(find.byKey(const Key('field-username')), 'alice');
      await t.pumpAndSettle();
      await shot(t, device, 'new-connection');

      // Field errors from the Rust validator.
      await t.enterText(find.byKey(const Key('field-host')), 'rdp://studio.example.com');
      await t.enterText(find.byKey(const Key('field-port')), '70000');
      await t.ensureVisible(find.byKey(const Key('button-save')));
      await t.pumpAndSettle();
      await t.tap(find.byKey(const Key('button-save')));
      await t.pumpAndSettle();
      await t.ensureVisible(find.byKey(const Key('field-name')));
      await t.pumpAndSettle();
      await shot(t, device, 'form-errors');
      await back(t);

      // Settings: top, bottom (About) and the help screens.
      await openSettings(t);
      await shot(t, device, 'settings');
      final credit = find.byKey(const Key('creator'));
      await t.scrollUntilVisible(credit, 300);
      await t.pumpAndSettle();
      await shot(t, device, 'settings-about');

      await t.scrollUntilVisible(find.text('Connection history'), -300);
      await t.tap(find.text('Connection history'));
      await t.pumpAndSettle();
      await shot(t, device, 'history');
      await back(t);

      await t.scrollUntilVisible(find.text('What error messages mean'), 300);
      await t.tap(find.text('What error messages mean'));
      await t.pumpAndSettle();
      await shot(t, device, 'error-help');
      await back(t);

      // Dark theme.
      await start(t, device, theme: ThemeMode.dark);
      await shot(t, device, 'connections-dark');
    });
  }

  for (final device in ['desktop', 'phone']) {
    testWidgets('$device: live session with the local test host',
        skip: skip != null || !haveLive, (t) async {
      final test = ConnectionProfile(
        id: 'live-test-host',
        name: 'Test computer',
        host: liveHost!,
        port: livePort!,
        username: liveUser!,
      );
      await start(t, device, extra: [test]);

      // Readiness check (opens and closes a TCP connection only).
      await t.tap(find.byTooltip('More').last);
      await t.pumpAndSettle();
      await t.tap(find.text('Check readiness'));
      await pumpUntil(t, find.textContaining('answers on port'), seconds: 10);
      await shot(t, device, 'readiness');
      await t.tap(find.text('Close'));
      await t.pumpAndSettle();

      // Wrong password: the error banner with advice.
      await t.tap(find.text('Test computer'));
      await t.pumpAndSettle();
      await t.enterText(find.byKey(const Key('connect-password')), _throwaway());
      await t.tap(find.byKey(const Key('button-connect')));
      await pumpUntil(t, find.byKey(const Key('certificate-fingerprint')));
      await shot(t, device, 'certificate');
      await t.tap(find.byKey(const Key('button-trust')));
      await pumpUntil(t, find.text('Back to connections'), seconds: 45);
      await shot(t, device, 'wrong-password');
      await t.tap(find.text('Back to connections'));
      await pumpFor(t, const Duration(seconds: 1));

      // The real password (never captured): connect and show the remote desktop.
      await t.tap(find.text('Test computer'));
      await t.pumpAndSettle();
      await t.enterText(find.byKey(const Key('connect-password')), livePassword!);
      await t.tap(find.byKey(const Key('button-connect')));
      await pumpUntil(t, find.byType(RawImage), seconds: 45);
      await pumpFor(t, const Duration(seconds: 4)); // let the first screen updates arrive
      await shot(t, device, 'session');

      await t.tap(find.byKey(const Key('button-disconnect')));
      await pumpUntil(t, find.text('Back to connections'), seconds: 20);
      await t.tap(find.text('Back to connections'));
      await pumpFor(t, const Duration(seconds: 1));

      await openSettings(t);
      await t.tap(find.text('Trusted computers'));
      await t.pumpAndSettle();
      await shot(t, device, 'trusted-computers');
    });
  }
}
