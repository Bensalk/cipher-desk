import 'package:flutter/material.dart';

import '../src/rust/api/errors.dart';
import '../state/profiles.dart';
import '../state/settings.dart';
import 'connection_form.dart';
import 'error_banner.dart';
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
  RdpConnectionState _state = RdpConnectionState.disconnected;

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

  void _connect(ConnectionProfile p) {
    widget.store.markUsed(p.id, DateTime.now());
    // The connection engine arrives in Stage 4. Until then, explain clearly.
    setState(() {
      _state = RdpConnectionState.disconnected;
      _notice = const ErrorInfo(
        id: 'NotYetAvailable',
        title: 'Connecting is not available yet',
        advice: 'This development build has the interface only. '
            'The secure connection engine is added in the next stage.',
        securityWarning: false,
      );
    });
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
    if (ok == true) widget.store.remove(p.id);
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
