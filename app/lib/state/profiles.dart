import 'dart:math';

import 'package:flutter/foundation.dart';

import '../src/rust/api/session.dart' show ClipboardMode;
import '../src/rust/api/store.dart' show ConnectionMethod;

/// A saved connection. Deliberately has **no password field**: passwords are
/// only ever stored in the operating system's vault (Stage 5).
@immutable
class ConnectionProfile {
  const ConnectionProfile({
    required this.id,
    required this.name,
    required this.host,
    required this.port,
    required this.username,
    this.savePassword = false,
    this.clipboardMode = ClipboardMode.off,
    this.lastUsed,
    this.wakeMac,
    this.method = ConnectionMethod.privateRemoteDesktop,
  });

  final String id;
  final String name;
  final String host;
  final int port;
  final String username;

  /// The user ticked "Save password" (the password itself lives in the vault).
  final bool savePassword;

  /// Clipboard sharing for this profile (off by default, S3/S6).
  final ClipboardMode clipboardMode;

  bool get clipboardEnabled => clipboardMode != ClipboardMode.off;
  final DateTime? lastUsed;

  /// MAC address for Wake-on-LAN (F11), or null.
  final String? wakeMac;

  /// Private Remote Desktop, Share screen or VNC.
  final ConnectionMethod method;

  /// "host:port", bracketing IPv6 addresses.
  String get endpoint => host.contains(':') ? '[$host]:$port' : '$host:$port';

  /// Name shown in lists: the profile name, or the address if none was given.
  String get displayName => name.trim().isEmpty ? endpoint : name.trim();

  ConnectionProfile copyWith({
    String? id,
    String? name,
    String? host,
    int? port,
    String? username,
    bool? savePassword,
    ClipboardMode? clipboardMode,
    DateTime? lastUsed,
    String? wakeMac,
    bool clearWakeMac = false,
    ConnectionMethod? method,
  }) {
    return ConnectionProfile(
      id: id ?? this.id,
      name: name ?? this.name,
      host: host ?? this.host,
      port: port ?? this.port,
      username: username ?? this.username,
      savePassword: savePassword ?? this.savePassword,
      clipboardMode: clipboardMode ?? this.clipboardMode,
      lastUsed: lastUsed ?? this.lastUsed,
      wakeMac: clearWakeMac ? null : (wakeMac ?? this.wakeMac),
      method: method ?? this.method,
    );
  }
}

/// One connection attempt, for the history screen (no secrets).
@immutable
class HistoryItem {
  const HistoryItem({required this.connectionId, required this.time, required this.outcome});

  final String connectionId;
  final DateTime time;

  /// "connected", "disconnected", "cancelled" or an error id such as "WrongCredentials".
  final String outcome;
}

/// Saved connections and their history. Pure Dart; saving to disk is done by
/// `ProfilePersistence` (lib/state/persistence.dart), which listens for changes.
class ProfileStore extends ChangeNotifier {
  final List<ConnectionProfile> _profiles = [];
  final List<HistoryItem> _history = [];

  /// Most history entries kept (matches the Rust store).
  static const maxHistory = 100;

  /// Most saved connections (matches the Rust store, which refuses more).
  static const maxConnections = 500;
  final Random _random = Random.secure();

  /// Profiles, most recently used first, then by name.
  List<ConnectionProfile> get profiles {
    final list = List<ConnectionProfile>.of(_profiles);
    list.sort((a, b) {
      final au = a.lastUsed, bu = b.lastUsed;
      if (au != null && bu != null) return bu.compareTo(au);
      if (au != null) return -1;
      if (bu != null) return 1;
      return a.displayName.toLowerCase().compareTo(b.displayName.toLowerCase());
    });
    return List.unmodifiable(list);
  }

  bool get isEmpty => _profiles.isEmpty;

  bool get isFull => _profiles.length >= maxConnections;

  /// History, newest first.
  List<HistoryItem> get history => List.unmodifiable(_history.reversed);

  /// Replaces everything (used when loading from disk). Does not notify, so
  /// loading does not immediately write the file back.
  void replaceAll(List<ConnectionProfile> profiles, List<HistoryItem> history) {
    _profiles
      ..clear()
      ..addAll(profiles);
    _history
      ..clear()
      ..addAll(history);
  }

  /// Raw lists in storage order (for saving).
  List<ConnectionProfile> get storageList => List.unmodifiable(_profiles);
  List<HistoryItem> get storageHistory => List.unmodifiable(_history);

  void addHistory(String connectionId, String outcome, DateTime when) {
    _history.add(HistoryItem(connectionId: connectionId, time: when, outcome: outcome));
    if (_history.length > maxHistory) _history.removeRange(0, _history.length - maxHistory);
    notifyListeners();
  }

  /// True if another connection uses the same host and port.
  /// (Names are compared ignoring case, like the certificate store does.)
  bool endpointSharedWithOthers(ConnectionProfile p) => _profiles.any(
      (o) => o.id != p.id && o.endpoint.toLowerCase() == p.endpoint.toLowerCase());

  ConnectionProfile? byId(String id) {
    for (final p in _profiles) {
      if (p.id == id) return p;
    }
    return null;
  }

  /// A random 128-bit id. Ids key saved passwords in the OS vault, so they must
  /// never be reused for a different connection (e.g. after an app restart).
  String newId() {
    final bytes = List<int>.generate(16, (_) => _random.nextInt(256));
    return bytes.map((b) => b.toRadixString(16).padLeft(2, '0')).join();
  }

  /// Adds a profile, or replaces the one with the same id.
  void upsert(ConnectionProfile profile) {
    final i = _profiles.indexWhere((p) => p.id == profile.id);
    if (i >= 0) {
      _profiles[i] = profile;
    } else {
      _profiles.add(profile);
    }
    notifyListeners();
  }

  /// Adds several new profiles with one change notification (import).
  void addAll(Iterable<ConnectionProfile> profiles) {
    _profiles.addAll(profiles);
    notifyListeners();
  }

  /// Marks every connection as having no saved password (one notification).
  void clearSavedPasswordFlags() {
    for (var i = 0; i < _profiles.length; i++) {
      if (_profiles[i].savePassword) _profiles[i] = _profiles[i].copyWith(savePassword: false);
    }
    notifyListeners();
  }

  void remove(String id) {
    _profiles.removeWhere((p) => p.id == id);
    _history.removeWhere((h) => h.connectionId == id);
    notifyListeners();
  }

  void markUsed(String id, DateTime when) {
    final i = _profiles.indexWhere((p) => p.id == id);
    if (i < 0) return;
    _profiles[i] = _profiles[i].copyWith(lastUsed: when);
    notifyListeners();
  }

  /// Profiles whose name, address or username contains [query] (F1 search).
  List<ConnectionProfile> search(String query) {
    final q = query.trim().toLowerCase();
    if (q.isEmpty) return profiles;
    return profiles
        .where((p) =>
            p.displayName.toLowerCase().contains(q) ||
            p.endpoint.toLowerCase().contains(q) ||
            p.username.toLowerCase().contains(q))
        .toList(growable: false);
  }
}
