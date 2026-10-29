import 'package:flutter/material.dart';

import 'package:path_provider/path_provider.dart';

import '../platform/password_vault.dart';
import '../src/rust/api/errors.dart';
import '../src/rust/api/pins.dart';
import '../state/profiles.dart';
import '../state/settings.dart';
import 'connection_form.dart';
import 'error_banner.dart';
import 'session_screen.dart';
import 'settings_screen.dart';
import 'status_chip.dart';

/// Main window: saved connections with search, and quick actions.
/// Uses a side rail on wide screens and a bottom bar on phones.
class HomeScreen extends StatefulWidget {
  const HomeScreen({super.key, required this.store, required this.settings});

  final ProfileStore store;
  final AppSettings settings;

  @override
  State<HomeScreen> createState() => _HomeScreenState();
}

class _HomeScreenState extends State<HomeScreen> {
  int _tab = 0;
  String _query = '';
  ErrorInfo? _notice;
  final RdpConnectionState _state = RdpConnectionState.disconnected;

  Future<void> _openForm([ConnectionProfile? existing]) async {
    await Navigator.of(context).push<ConnectionProfile>(
      MaterialPageRoute(
        builder: (_) => ConnectionFormScreen(
          store: widget.store,
          existing: existing,
          clipboardDefault: widget.settings.clipboardDefault,
        ),
      ),
    );
  }

  Future<void> _connect(ConnectionProfile p) async {
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
      } else if (await PasswordVault.confirmUser('Confirm to use the saved password')) {
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
    if (!mounted) return;
    widget.store.markUsed(p.id, DateTime.now());
    setState(() => _notice = null);
    await Navigator.of(context).push(
      MaterialPageRoute<void>(
        builder: (_) => SessionScreen(
          profile: p,
          password: password,
          useSavedPassword: useSaved,
        ),
      ),
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
    widget.store.remove(p.id);
    // Remove everything kept for this connection: saved password and trusted certificate.
    await PasswordVault.delete(p.id);
    final dir = await getApplicationSupportDirectory();
    forgetCertificate(pinsPath: pinsFilePath(dir.path), host: p.host, port: p.port);
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
    final body = _tab == 0 ? _connections() : SettingsScreen(settings: widget.settings);
    final destinations = const [
      (Icons.computer_outlined, Icons.computer, 'Connections'),
      (Icons.settings_outlined, Icons.settings, 'Settings'),
    ];
    return Scaffold(
      appBar: AppBar(title: const Text('Secure RDP Viewer')),
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
  });

  final ConnectionProfile profile;
  final VoidCallback onConnect;
  final VoidCallback onEdit;
  final VoidCallback onDelete;

  @override
  Widget build(BuildContext context) {
    final details = [
      profile.endpoint,
      if (profile.username.isNotEmpty) profile.username,
    ].join(' · ');
    return ListTile(
      leading: const CircleAvatar(child: Icon(Icons.desktop_windows_outlined)),
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
            onSelected: (v) => v == 'edit' ? onEdit() : onDelete(),
            itemBuilder: (_) => const [
              PopupMenuItem(value: 'edit', child: Text('Edit')),
              PopupMenuItem(value: 'delete', child: Text('Delete')),
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

/// Asks for the password for one connection. The text is cleared when the
/// dialog closes; it is never stored by the app (saving arrives in Stage 5).
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
          Text('Username: ${widget.profile.username}'),
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

