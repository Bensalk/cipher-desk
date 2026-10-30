import 'package:flutter/material.dart';

import '../platform/password_vault.dart';
import '../src/rust/api/errors.dart';
import '../src/rust/api/pins.dart';
import '../src/rust/api/store.dart';
import '../state/persistence.dart';
import '../state/profiles.dart';
import '../state/settings.dart';
import 'connection_form.dart';
import 'connection_methods.dart';
import 'error_banner.dart';
import 'session_screen.dart';
import 'settings_screen.dart';
import 'status_chip.dart';

/// Main window: saved connections with search, and quick actions.
/// Uses a side rail on wide screens and a bottom bar on phones.
class HomeScreen extends StatefulWidget {
  const HomeScreen({
    super.key,
    required this.store,
    required this.settings,
    this.paths,
    this.startupNotice,
  });

  final ProfileStore store;
  final AppSettings settings;

  /// App data paths; resolved on first use when null (tests).
  final AppPaths? paths;

  /// Shown once at start-up (for example, a damaged connections file).
  final String? startupNotice;

  @override
  State<HomeScreen> createState() => _HomeScreenState();
}

class _HomeScreenState extends State<HomeScreen> {
  int _tab = 0;
  String _query = '';
  ErrorInfo? _notice;
  final RdpConnectionState _state = RdpConnectionState.disconnected;
  AppPaths? _paths;

  Future<AppPaths> _appPaths() async => _paths ??= widget.paths ?? await AppPaths.resolve();

  @override
  void initState() {
    super.initState();
    final notice = widget.startupNotice;
    if (notice != null) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted) return;
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text(notice), duration: const Duration(seconds: 10)),
        );
      });
    }
  }

  void _snack(String text) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(text)));
  }

  Future<void> _openForm([ConnectionProfile? existing]) async {
    if (existing == null && widget.store.isFull) {
      _snack('You have reached the limit of ${ProfileStore.maxConnections} saved connections.');
      return;
    }
    await Navigator.of(context).push<ConnectionProfile>(
      MaterialPageRoute(
        builder: (_) => ConnectionFormScreen(
          store: widget.store,
          existing: existing,
          clipboardDefault: widget.settings.clipboardDefault,
          portable: widget.paths?.portable ?? false,
        ),
      ),
    );
  }

  Future<void> _connect(ConnectionProfile p) async {
    if (p.method == ConnectionMethod.vnc) {
      _snack('VNC connections arrive in the next version of Cipher Desk.');
      return;
    }
    String? password;
    var useSaved = false;
    String? notice;

    if (p.savePassword) {
      if (PasswordVault.readsIntoDart) {
        // Android: the Keystore prompt confirms the user, then returns the password.
        final (outcome, pw) = await PasswordVault.loadOnAndroid(p.id);
        if (outcome == VaultOutcome.ok && pw != null) {
          password = pw;
        } else if (outcome != VaultOutcome.locked) {
          notice = 'The saved password could not be read. Type it to connect.';
        } else {
          return; // The user cancelled the confirmation.
        }
      } else if (await PasswordVault.confirmUser(
        'Confirm to use the saved password',
        required: widget.settings.confirmSavedPassword,
      )) {
        // Windows / Linux: Rust reads the vault itself; the password never enters Dart.
        useSaved = true;
      } else {
        notice = 'Your identity could not be confirmed, so the saved password was not used.';
      }
    }

    if (!useSaved && password == null) {
      if (!mounted) return;
      password = await showDialog<String>(
        context: context,
        builder: (_) => _PasswordDialog(profile: p, notice: notice),
      );
      if (password == null) return;
    }
    final paths = await _appPaths();
    if (!mounted) return;
    widget.store.markUsed(p.id, DateTime.now());
    setState(() => _notice = null);
    final reconnect = await Navigator.of(context).push<bool>(
      MaterialPageRoute<bool>(
        builder: (_) => SessionScreen(
          profile: p,
          password: password,
          useSavedPassword: useSaved,
          pinsPath: paths.pins,
          idleMinutes: widget.settings.idleMinutes,
          onOutcome: (outcome) => widget.store.addHistory(p.id, outcome, DateTime.now()),
        ),
      ),
    );
    password = null;
    // After re-trusting a changed certificate, connect again (asks to check it).
    if (reconnect == true && mounted) {
      final current = widget.store.byId(p.id);
      if (current != null) await _connect(current);
    }
  }

  Future<void> _wake(ConnectionProfile p) async {
    final mac = p.wakeMac;
    if (mac == null) return;
    switch (wakeOnLan(mac: mac)) {
      case WakeResult.sent:
        _snack('Wake-up signal sent to ${p.displayName}. It may take a minute to start.');
      case WakeResult.invalidMac:
        _snack('The saved MAC address is not valid. Edit the connection to fix it.');
      case WakeResult.sendFailed:
        _snack('The wake-up signal could not be sent. Check that you are on the same network.');
    }
  }

  Future<void> _checkReadiness(ConnectionProfile p) async {
    final paths = await _appPaths();
    if (!mounted) return;
    await showDialog<void>(
      context: context,
      builder: (_) => _ReadinessDialog(profile: p, pinsPath: paths.pins),
    );
  }

  Future<void> _confirmDelete(ConnectionProfile p) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Delete connection?'),
        content: Text('"${p.displayName}" will be removed. '
            'Its saved password, if any, will also be deleted.'),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('Cancel')),
          FilledButton(onPressed: () => Navigator.pop(ctx, true), child: const Text('Delete')),
        ],
      ),
    );
    if (ok != true) return;
    // The trusted certificate belongs to the computer, so keep it while another
    // connection still uses the same computer and port.
    final shared = widget.store.endpointSharedWithOthers(p);
    widget.store.remove(p.id);
    // Remove everything kept for this connection: saved password and trusted certificate.
    await PasswordVault.delete(p.id);
    if (!shared) {
      final paths = await _appPaths();
      forgetCertificate(pinsPath: paths.pins, host: p.host, port: p.port);
    }
  }

  Widget _connections() {
    return ListenableBuilder(
      listenable: widget.store,
      builder: (context, _) {
        final items = widget.store.search(_query);
        return Column(
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 12, 16, 4),
              child: Row(
                children: [
                  Expanded(
                    child: SearchBar(
                      key: const Key('search'),
                      hintText: 'Search connections',
                      leading: const Icon(Icons.search),
                      onChanged: (v) => setState(() => _query = v),
                    ),
                  ),
                  const SizedBox(width: 12),
                  StatusChip(state: _state),
                ],
              ),
            ),
            if (_notice != null)
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: 12),
                child: ErrorBanner(
                  error: _notice!,
                  onDismiss: () => setState(() => _notice = null),
                ),
              ),
            Expanded(
              child: widget.store.isEmpty
                  ? _EmptyState(onAdd: _openForm)
                  : items.isEmpty
                      ? const Center(child: Text('No connections match your search.'))
                      : ListView.builder(
                          itemCount: items.length,
                          itemBuilder: (context, i) => _ProfileTile(
                            profile: items[i],
                            onConnect: () => _connect(items[i]),
                            onEdit: () => _openForm(items[i]),
                            onDelete: () => _confirmDelete(items[i]),
                            onWake: () => _wake(items[i]),
                            onCheck: () => _checkReadiness(items[i]),
                          ),
                        ),
            ),
          ],
        );
      },
    );
  }

  @override
  Widget build(BuildContext context) {
    final wide = MediaQuery.sizeOf(context).width >= 720;
    final body = _tab == 0
        ? _connections()
        : SettingsScreen(
            settings: widget.settings,
            store: widget.store,
            paths: _appPaths,
            portableDir: (widget.paths?.portable ?? false) ? widget.paths!.dir : null,
          );
    final destinations = const [
      (Icons.computer_outlined, Icons.computer, 'Connections'),
      (Icons.settings_outlined, Icons.settings, 'Settings'),
    ];
    return Scaffold(
      appBar: AppBar(title: const Text('Cipher Desk')),
      floatingActionButton: _tab == 0
          ? FloatingActionButton.extended(
              key: const Key('button-new'),
              onPressed: _openForm,
              icon: const Icon(Icons.add),
              label: const Text('New connection'),
            )
          : null,
      bottomNavigationBar: wide
          ? null
          : NavigationBar(
              selectedIndex: _tab,
              onDestinationSelected: (i) => setState(() => _tab = i),
              destinations: [
                for (final d in destinations)
                  NavigationDestination(icon: Icon(d.$1), selectedIcon: Icon(d.$2), label: d.$3),
              ],
            ),
      body: wide
          ? Row(
              children: [
                NavigationRail(
                  selectedIndex: _tab,
                  onDestinationSelected: (i) => setState(() => _tab = i),
                  labelType: NavigationRailLabelType.all,
                  destinations: [
                    for (final d in destinations)
                      NavigationRailDestination(
                        icon: Icon(d.$1),
                        selectedIcon: Icon(d.$2),
                        label: Text(d.$3),
                      ),
                  ],
                ),
                const VerticalDivider(width: 1),
                Expanded(child: body),
              ],
            )
          : body,
    );
  }
}

class _ProfileTile extends StatelessWidget {
  const _ProfileTile({
    required this.profile,
    required this.onConnect,
    required this.onEdit,
    required this.onDelete,
    required this.onWake,
    required this.onCheck,
  });

  final ConnectionProfile profile;
  final VoidCallback onConnect;
  final VoidCallback onEdit;
  final VoidCallback onDelete;
  final VoidCallback onWake;
  final VoidCallback onCheck;

  @override
  Widget build(BuildContext context) {
    final details = [
      profile.endpoint,
      if (profile.username.isNotEmpty) profile.username,
      profile.method.label,
    ].join(' · ');
    return ListTile(
      leading: CircleAvatar(
        child: Tooltip(message: profile.method.label, child: Icon(profile.method.icon)),
      ),
      title: Text(profile.displayName),
      subtitle: Text(details),
      onTap: onConnect,
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (profile.savePassword)
            const Tooltip(message: 'Password saved', child: Icon(Icons.key_outlined, size: 18)),
          if (profile.clipboardEnabled)
            const Tooltip(
              message: 'Clipboard sharing on',
              child: Icon(Icons.content_paste_outlined, size: 18),
            ),
          PopupMenuButton<String>(
            tooltip: 'More',
            onSelected: (v) => switch (v) {
              'edit' => onEdit(),
              'wake' => onWake(),
              'check' => onCheck(),
              _ => onDelete(),
            },
            itemBuilder: (_) => [
              const PopupMenuItem(value: 'edit', child: Text('Edit')),
              if (profile.wakeMac != null) const PopupMenuItem(value: 'wake', child: Text('Wake up')),
              const PopupMenuItem(value: 'check', child: Text('Check readiness')),
              const PopupMenuItem(value: 'delete', child: Text('Delete')),
            ],
          ),
        ],
      ),
    );
  }
}

class _EmptyState extends StatelessWidget {
  const _EmptyState({required this.onAdd});

  final VoidCallback onAdd;

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(Icons.lan_outlined, size: 48, color: Theme.of(context).colorScheme.outline),
            const SizedBox(height: 12),
            Text('No connections yet', style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 4),
            const Text(
              'Add a computer to connect to. Only use this app over your own network or a VPN.',
              textAlign: TextAlign.center,
            ),
            const SizedBox(height: 16),
            OutlinedButton.icon(
              onPressed: onAdd,
              icon: const Icon(Icons.add),
              label: const Text('Add a connection'),
            ),
          ],
        ),
      ),
    );
  }
}

/// Checks what a connection needs before connecting (F12). Nothing is sent to
/// the computer: reachability only opens and closes a TCP connection.
class _ReadinessDialog extends StatefulWidget {
  const _ReadinessDialog({required this.profile, required this.pinsPath});

  final ConnectionProfile profile;
  final String pinsPath;

  @override
  State<_ReadinessDialog> createState() => _ReadinessDialogState();
}

class _ReadinessDialogState extends State<_ReadinessDialog> {
  bool? _reachable;
  late final bool _trusted;

  @override
  void initState() {
    super.initState();
    final p = widget.profile;
    _trusted = trustedComputers(pinsPath: widget.pinsPath).any((t) => t.endpoint == p.endpoint.toLowerCase());
    checkReachable(host: p.host, port: p.port, timeoutMs: 5000).then((ok) {
      if (mounted) setState(() => _reachable = ok);
    });
  }

  Widget _row(bool? ok, String good, String bad, {String? pending}) {
    final icon = ok == null
        ? const SizedBox(width: 20, height: 20, child: CircularProgressIndicator(strokeWidth: 2))
        : Icon(ok ? Icons.check_circle_outline : Icons.error_outline,
            color: ok ? Colors.green : Theme.of(context).colorScheme.error);
    return ListTile(
      contentPadding: EdgeInsets.zero,
      leading: icon,
      title: Text(ok == null ? (pending ?? '') : (ok ? good : bad)),
    );
  }

  @override
  Widget build(BuildContext context) {
    final p = widget.profile;
    return AlertDialog(
      title: Text('Ready to connect to ${p.displayName}?'),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          _row(_reachable, 'The computer answers on port ${p.port}',
              'The computer does not answer on port ${p.port}. Check that it is on, awake, '
                  'and that you are on the same network or VPN.',
              pending: 'Checking the computer…'),
          _row(_trusted, 'Its certificate is trusted',
              'Not connected before: you will be asked to check its certificate'),
          if (p.method.needsUsername)
            _row(p.username.isNotEmpty, 'Username: ${p.username}', 'No username set'),
          _row(p.savePassword, 'Password saved in this device\'s secure storage',
              'The password will be asked for when connecting'),
          if (p.wakeMac != null) _row(true, 'Wake-on-LAN set up', ''),
        ],
      ),
      actions: [TextButton(onPressed: () => Navigator.pop(context), child: const Text('Close'))],
    );
  }
}

/// Asks for the password for one connection. The text is cleared when the
/// dialog closes; it is never stored by the app.
class _PasswordDialog extends StatefulWidget {
  const _PasswordDialog({required this.profile, this.notice});

  final ConnectionProfile profile;
  final String? notice;

  @override
  State<_PasswordDialog> createState() => _PasswordDialogState();
}

class _PasswordDialogState extends State<_PasswordDialog> {
  final _controller = TextEditingController();
  bool _obscure = true;

  @override
  void dispose() {
    _controller.clear();
    _controller.dispose();
    super.dispose();
  }

  void _submit() {
    if (_controller.text.isEmpty) return;
    Navigator.pop(context, _controller.text);
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: Text('Connect to ${widget.profile.displayName}'),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          if (widget.notice != null) ...[
            Text(widget.notice!, style: TextStyle(color: Theme.of(context).colorScheme.error)),
            const SizedBox(height: 8),
          ],
          if (widget.profile.username.isNotEmpty) Text('Username: ${widget.profile.username}'),
          const SizedBox(height: 12),
          TextField(
            key: const Key('connect-password'),
            controller: _controller,
            autofocus: true,
            obscureText: _obscure,
            autocorrect: false,
            enableSuggestions: false,
            enableIMEPersonalizedLearning: false,
            keyboardType: TextInputType.visiblePassword,
            decoration: InputDecoration(
              labelText: 'Password',
              suffixIcon: IconButton(
                tooltip: _obscure ? 'Show password' : 'Hide password',
                icon: Icon(_obscure ? Icons.visibility : Icons.visibility_off),
                onPressed: () => setState(() => _obscure = !_obscure),
              ),
            ),
            onSubmitted: (_) => _submit(),
          ),
        ],
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context), child: const Text('Cancel')),
        FilledButton(
          key: const Key('button-connect'),
          onPressed: _submit,
          child: const Text('Connect'),
        ),
      ],
    );
  }
}

