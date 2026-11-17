/// Keeps track, on this device, of wrong passwords typed for Share screen
/// computers, so the app can say how long a computer will keep refusing this
/// device after too many of them.
///
/// It mirrors the host's own rules (`rust/host/src/limiter.rs`): 5 wrong
/// passwords within 10 minutes block the device for 15 minutes, and a real
/// session (20 seconds or longer) forgives earlier typos. It only knows about
/// this device's own tries since the app started; the host's count is the
/// one that decides.
class LockoutTracker {
  LockoutTracker({DateTime Function()? now}) : _now = now ?? DateTime.now;

  static const maxFailures = 5;
  static const window = Duration(minutes: 10);
  static const lockout = Duration(minutes: 15);
  static const realSession = Duration(seconds: 20);

  final DateTime Function() _now;
  final _failures = <String, List<DateTime>>{};
  final _lockedUntil = <String, DateTime>{};

  /// The key for a computer: its address and port.
  static String keyFor(String host, int port) => '${host.trim().toLowerCase()}:$port';

  /// A wrong password was typed for [key].
  void wrongPassword(String key) {
    final now = _now();
    if (lockedUntil(key) != null) return;
    final list = _failures.putIfAbsent(key, () => [])
      ..removeWhere((t) => now.difference(t) >= window)
      ..add(now);
    if (list.length >= maxFailures) {
      _lockedUntil[key] = now.add(lockout);
      list.clear();
    }
  }

  /// A session with [key] ended after [length]; a real one forgives earlier
  /// typos, as the host does.
  void sessionEnded(String key, Duration length) {
    if (length >= realSession) {
      _failures.remove(key);
      _lockedUntil.remove(key);
    }
  }

  /// When [key] stops refusing this device, if it is refusing it now.
  DateTime? lockedUntil(String key) {
    final until = _lockedUntil[key];
    if (until == null) return null;
    if (!_now().isBefore(until)) {
      _lockedUntil.remove(key);
      return null;
    }
    return until;
  }

  /// Wrong passwords this device may still type before it is blocked.
  int triesLeft(String key) {
    final now = _now();
    final recent = (_failures[key] ?? const <DateTime>[])
        .where((t) => now.difference(t) < window)
        .length;
    final left = maxFailures - recent;
    return left < 0 ? 0 : left;
  }

  /// Whole minutes left (rounded up) until [key] stops refusing this device.
  int? minutesLeft(String key) {
    final until = lockedUntil(key);
    if (until == null) return null;
    final minutes = (until.difference(_now()).inSeconds + 59) ~/ 60;
    if (minutes < 1) return 1;
    return minutes > lockout.inMinutes ? lockout.inMinutes : minutes;
  }
}

/// The app's one tracker (kept while the app runs).
final lockoutTracker = LockoutTracker();
