import 'package:flutter/material.dart' show ThemeMode;
import 'package:flutter_test/flutter_test.dart';
import 'package:secure_rdp_viewer/src/rust/api/session.dart' show ClipboardMode;
import 'package:secure_rdp_viewer/state/profiles.dart';
import 'package:secure_rdp_viewer/state/settings.dart';

ConnectionProfile _p(ProfileStore s, String name, String host, {String user = 'alice'}) =>
    ConnectionProfile(id: s.newId(), name: name, host: host, port: 3389, username: user);

void main() {
  group('ProfileStore', () {
    test('add, edit and delete', () {
      final s = ProfileStore();
      final a = _p(s, 'Office', '192.0.2.10');
      s.upsert(a);
      expect(s.profiles, hasLength(1));
      s.upsert(a.copyWith(name: 'Office PC'));
      expect(s.profiles.single.name, 'Office PC');
      s.remove(a.id);
      expect(s.isEmpty, isTrue);
    });

    test('recently used first, then by name', () {
      final s = ProfileStore();
      final b = _p(s, 'Beta', 'beta.example.com');
      final a = _p(s, 'alpha', 'alpha.example.com');
      final c = _p(s, 'Gamma', 'gamma.example.com');
      for (final p in [b, a, c]) {
        s.upsert(p);
      }
      expect(s.profiles.map((p) => p.name), ['alpha', 'Beta', 'Gamma']);
      s.markUsed(c.id, DateTime(2026, 10, 29));
      expect(s.profiles.first.name, 'Gamma');
    });

    test('search matches name, address and username', () {
      final s = ProfileStore();
      s.upsert(_p(s, 'Office', '192.0.2.10'));
      s.upsert(_p(s, 'Home', 'home.example.com', user: 'bob'));
      expect(s.search('off').single.name, 'Office');
      expect(s.search('192.0.2').single.name, 'Office');
      expect(s.search('BOB').single.name, 'Home');
      expect(s.search(''), hasLength(2));
      expect(s.search('nothing'), isEmpty);
    });

    test('display name and IPv6 endpoint', () {
      const p = ConnectionProfile(id: 'x', name: ' ', host: '2001:db8::1', port: 3390, username: 'u');
      expect(p.endpoint, '[2001:db8::1]:3390');
      expect(p.displayName, '[2001:db8::1]:3390');
    });

    test('new profiles have clipboard and saved password off', () {
      const p = ConnectionProfile(id: 'x', name: 'n', host: 'h', port: 1, username: 'u');
      expect(p.clipboardEnabled, isFalse);
      expect(p.savePassword, isFalse);
    });
  });

  test('clipboard mode defaults to off and can be set per direction', () {
    const p = ConnectionProfile(id: 'x', name: 'n', host: 'h', port: 1, username: 'u');
    expect(p.clipboardMode, ClipboardMode.off);
    expect(p.clipboardEnabled, isFalse);
    final one = p.copyWith(clipboardMode: ClipboardMode.fromRemoteOnly);
    expect(one.clipboardEnabled, isTrue);
    expect(one.copyWith(name: 'm').clipboardMode, ClipboardMode.fromRemoteOnly);
  });

  test('connection ids are random, unique and vault-safe', () {
    final s = ProfileStore();
    final ids = {for (var i = 0; i < 200; i++) s.newId()};
    expect(ids, hasLength(200));
    for (final id in ids) {
      expect(id, matches(RegExp(r'^[0-9a-f]{32}$')));
    }
    // A new store (e.g. after an app restart) must not reproduce earlier ids.
    expect(ids.contains(ProfileStore().newId()), isFalse);
  });

  group('history and stage 7 helpers', () {
    test('history is newest first, capped, and removed with its connection', () {
      final s = ProfileStore();
      final a = _p(s, 'A', 'a.example.com');
      final b = _p(s, 'B', 'b.example.com');
      s.upsert(a);
      s.upsert(b);
      for (var i = 0; i < ProfileStore.maxHistory + 5; i++) {
        s.addHistory(i.isEven ? a.id : b.id, 'connected', DateTime(2026, 1, 1).add(Duration(minutes: i)));
      }
      expect(s.history, hasLength(ProfileStore.maxHistory));
      expect(s.history.first.time.isAfter(s.history.last.time), isTrue);
      s.remove(a.id);
      expect(s.history.every((h) => h.connectionId == b.id), isTrue);
    });

    test('shared endpoint check ignores case and the connection itself', () {
      final s = ProfileStore();
      final a = _p(s, 'A', 'Office-PC');
      final b = _p(s, 'B', 'office-pc');
      final c = _p(s, 'C', 'office-pc').copyWith(port: 3390);
      s.upsert(a);
      expect(s.endpointSharedWithOthers(a), isFalse);
      s.upsert(c);
      expect(s.endpointSharedWithOthers(a), isFalse); // different port
      s.upsert(b);
      expect(s.endpointSharedWithOthers(a), isTrue);
    });

    test('copyWith can change the id and clear the wake address', () {
      const p = ConnectionProfile(
          id: 'x', name: 'n', host: 'h', port: 1, username: 'u', wakeMac: '00:11:22:33:44:55');
      expect(p.copyWith(id: 'y').id, 'y');
      expect(p.copyWith(name: 'm').wakeMac, '00:11:22:33:44:55');
      expect(p.copyWith(clearWakeMac: true).wakeMac, isNull);
    });

    test('replaceAll does not notify (loading must not write the file back)', () {
      final s = ProfileStore();
      var notified = 0;
      s.addListener(() => notified++);
      s.replaceAll([_p(s, 'A', 'a')], const []);
      expect(notified, 0);
      expect(s.profiles, hasLength(1));
      s.addAll([_p(s, 'B', 'b'), _p(s, 'C', 'c')]);
      expect(notified, 1);
      expect(s.byId(s.profiles.first.id), isNotNull);
      expect(s.byId('missing'), isNull);
    });
  });

  test('clearing saved-password flags keeps the connections', () {
    final s = ProfileStore();
    final a = _p(s, 'A', 'a').copyWith(savePassword: true);
    final b = _p(s, 'B', 'b');
    s.upsert(a);
    s.upsert(b);
    var notified = 0;
    s.addListener(() => notified++);
    s.clearSavedPasswordFlags();
    expect(notified, 1);
    expect(s.profiles, hasLength(2));
    expect(s.profiles.any((p) => p.savePassword), isFalse);
  });

  group('AppSettings', () {
    test('secure defaults', () {
      final s = AppSettings();
      expect(s.blockScreenshots, isTrue);
      expect(s.clipboardDefault, isFalse);
      expect(s.confirmSavedPassword, isTrue);
      expect(s.idleMinutes, 30);
    });

    test('load does not notify; setters do, and ignore invalid values', () {
      final s = AppSettings();
      var notified = 0;
      s.addListener(() => notified++);
      s.load(
        themeMode: ThemeMode.dark,
        blockScreenshots: false,
        clipboardDefault: true,
        confirmSavedPassword: false,
        idleMinutes: 0,
      );
      expect(notified, 0);
      expect(s.idleMinutes, 0);
      s.idleMinutes = -1;
      expect(s.idleMinutes, 0);
      s.idleMinutes = 15;
      expect(notified, 1);
    });
  });
}
