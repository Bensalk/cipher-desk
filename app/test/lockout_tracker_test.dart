import 'package:cipher_desk/state/lockout_tracker.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  late DateTime now;
  late LockoutTracker t;
  const key = '100.64.0.1:3395';

  setUp(() {
    now = DateTime(2026, 11, 17, 10, 0);
    t = LockoutTracker(now: () => now);
  });

  test('five wrong passwords block for 15 minutes, then it clears', () {
    for (var i = 0; i < 4; i++) {
      t.wrongPassword(key);
      expect(t.lockedUntil(key), isNull);
    }
    expect(t.triesLeft(key), 1);
    t.wrongPassword(key);
    expect(t.lockedUntil(key), now.add(const Duration(minutes: 15)));
    expect(t.minutesLeft(key), 15);

    now = now.add(const Duration(minutes: 14, seconds: 1));
    expect(t.minutesLeft(key), 1);
    now = now.add(const Duration(minutes: 1));
    expect(t.lockedUntil(key), isNull);
    expect(t.triesLeft(key), 5);
  });

  test('old wrong passwords and other computers do not count', () {
    for (var i = 0; i < 4; i++) {
      t.wrongPassword(key);
    }
    now = now.add(const Duration(minutes: 10));
    t.wrongPassword(key);
    expect(t.lockedUntil(key), isNull);
    expect(t.triesLeft('100.64.0.2:3395'), 5);
  });

  test('a real session forgives earlier typos, a short one does not', () {
    for (var i = 0; i < 3; i++) {
      t.wrongPassword(key);
    }
    t.sessionEnded(key, const Duration(seconds: 5));
    expect(t.triesLeft(key), 2);
    t.sessionEnded(key, const Duration(seconds: 20));
    expect(t.triesLeft(key), 5);
  });

  test('keys ignore case and spaces in the address', () {
    expect(LockoutTracker.keyFor(' MyPC ', 3395), 'mypc:3395');
  });
}
