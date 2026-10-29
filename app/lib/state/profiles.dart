import 'package:flutter/foundation.dart';

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
    this.clipboardEnabled = false,
    this.lastUsed,
  });

  final String id;
  final String name;
  final String host;
  final int port;
  final String username;

  /// The user ticked "Save password" (the password itself lives in the vault).
  final bool savePassword;

  /// Clipboard sharing for this profile (off by default, S3/S6).
  final bool clipboardEnabled;
  final DateTime? lastUsed;

  /// "host:port", bracketing IPv6 addresses.
  String get endpoint => host.contains(':') ? '[$host]:$port' : '$host:$port';

  /// Name shown in lists: the profile name, or the address if none was given.
  String get displayName => name.trim().isEmpty ? endpoint : name.trim();

  ConnectionProfile copyWith({
    String? name,
    String? host,
    int? port,
    String? username,
    bool? savePassword,
    bool? clipboardEnabled,
    DateTime? lastUsed,
  }) {
    return ConnectionProfile(
      id: id,
      name: name ?? this.name,
      host: host ?? this.host,
      port: port ?? this.port,
      username: username ?? this.username,
      savePassword: savePassword ?? this.savePassword,
      clipboardEnabled: clipboardEnabled ?? this.clipboardEnabled,
      lastUsed: lastUsed ?? this.lastUsed,
    );
  }
}

/// In-memory list of profiles for Stage 3. Saving to disk arrives in Stage 7.
class ProfileStore extends ChangeNotifier {
  final List<ConnectionProfile> _profiles = [];
  int _nextId = 1;

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

  String newId() => 'p${_nextId++}';

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

  void remove(String id) {
    _profiles.removeWhere((p) => p.id == id);
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
