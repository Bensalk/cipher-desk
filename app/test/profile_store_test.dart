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

  group('AppSettings', () {
    test('secure defaults', () {
      final s = AppSettings();
      expect(s.blockScreenshots, isTrue);
      expect(s.clipboardDefault, isFalse);
    });
  });
}
