import 'dart:io';

import 'package:file_selector/file_selector.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../platform/secure_window.dart';
import '../src/rust/api/errors.dart';
import '../src/rust/api/info.dart';
import '../src/rust/api/store.dart';
import '../state/persistence.dart';
import '../state/profiles.dart';
import '../state/settings.dart';

/// Settings, saved connections tools and About. Settings are saved to disk by
/// `SettingsPersistence` whenever they change.
class SettingsScreen extends StatelessWidget {
  const SettingsScreen({super.key, required this.settings, required this.store, required this.paths});

  final AppSettings settings;
  final ProfileStore store;
  final Future<AppPaths> Function() paths;

  static bool get _isAndroid => !kIsWeb && defaultTargetPlatform == TargetPlatform.android;
  static bool get _isWindows => !kIsWeb && defaultTargetPlatform == TargetPlatform.windows;

  static String _idleLabel(int m) => switch (m) {
        0 => 'Never',
        60 => '1 hour',
        120 => '2 hours',
        _ => '$m minutes',
      };

  Future<void> _setConfirm(BuildContext context, bool value) async {
    if (value) {
      settings.confirmSavedPassword = true;
      return;
    }
    // Turning it off: explain what that means first (KI-18).
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        icon: const Icon(Icons.warning_amber_outlined),
        title: const Text('Stop asking for Windows Hello?'),
        content: const Text('Anyone who can use your Windows account could then connect with '
            'your saved passwords without confirming it is you. Keep this on unless the computer '
            'is only used by you and locks when you are away.'),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('Keep asking')),
          FilledButton(onPressed: () => Navigator.pop(ctx, true), child: const Text('Stop asking')),
        ],
      ),
    );
    if (ok == true) settings.confirmSavedPassword = false;
  }

  void _snack(BuildContext context, String text) =>
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(text)));

  Future<void> _export(BuildContext context) async {
    final text = connectionsExport(
      connections: [for (final p in store.profiles) ProfilePersistence.toData(p)],
    );
    if (!context.mounted) return;
    if (_isAndroid) {
      await showDialog<void>(context: context, builder: (_) => _ExportTextDialog(text: text));
      return;
    }
    final location = await getSaveLocation(
      suggestedName: 'rdp-connections.json',
      acceptedTypeGroups: const [XTypeGroup(label: 'JSON', extensions: ['json'])],
    );
    if (location == null || !context.mounted) return;
    try {
      await File(location.path).writeAsString(text, flush: true);
      if (context.mounted) _snack(context, 'Exported ${store.profiles.length} connection(s). No passwords were included.');
    } on FileSystemException {
      if (context.mounted) _snack(context, 'The export file could not be written.');
    }
  }

  Future<void> _import(BuildContext context) async {
    String? text;
    if (_isAndroid) {
      text = await showDialog<String>(context: context, builder: (_) => const _ImportTextDialog());
    } else {
      final file = await openFile(
        acceptedTypeGroups: const [XTypeGroup(label: 'JSON', extensions: ['json'])],
      );
      if (file == null) return;
      // The Rust side also refuses anything over 4 MB.
      if (await file.length() > 4 * 1024 * 1024) {
        if (context.mounted) _snack(context, 'That file is too large to be a connections export.');
        return;
      }
      text = await file.readAsString();
    }
    if (text == null || !context.mounted) return;
    final r = connectionsImport(text: text);
    if (r.notAnExport) {
      _snack(context, 'That is not a Secure RDP Viewer connections export.');
      return;
    }
    // Imported connections get fresh ids and never a saved password.
    final room = ProfileStore.maxConnections - store.profiles.length;
    final incoming = [
      for (final c in r.connections.take(room < 0 ? 0 : room))
        ProfilePersistence.fromData(c).copyWith(id: store.newId(), savePassword: false),
    ];
    store.addAll(incoming);
    if (incoming.length < r.connections.length) {
      _snack(context, 'Only ${incoming.length} connection(s) were imported: the limit is '
          '${ProfileStore.maxConnections} saved connections.');
      return;
    }
    final skipped = r.skipped > 0 ? ' ${r.skipped} invalid entr${r.skipped == 1 ? 'y was' : 'ies were'} skipped.' : '';
    _snack(context, 'Imported ${incoming.length} connection(s) with clipboard sharing off.$skipped');
  }

  @override
  Widget build(BuildContext context) {
    final isAndroid = _isAndroid;
    return ListenableBuilder(
      listenable: settings,
      builder: (context, _) => ListView(
        padding: const EdgeInsets.symmetric(vertical: 8),
        children: [
          const _Header('Appearance'),
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
            child: SegmentedButton<ThemeMode>(
              segments: const [
                ButtonSegment(value: ThemeMode.system, label: Text('System'), icon: Icon(Icons.brightness_auto)),
                ButtonSegment(value: ThemeMode.light, label: Text('Light'), icon: Icon(Icons.light_mode)),
                ButtonSegment(value: ThemeMode.dark, label: Text('Dark'), icon: Icon(Icons.dark_mode)),
              ],
              selected: {settings.themeMode},
              onSelectionChanged: (s) => settings.themeMode = s.first,
            ),
          ),
          const _Header('Privacy & security'),
          if (isAndroid)
            SwitchListTile(
              key: const Key('setting-block-screenshots'),
              value: settings.blockScreenshots,
              onChanged: (v) {
                settings.blockScreenshots = v;
                SecureWindow.setEnabled(v);
              },
              title: const Text('Block screenshots'),
              subtitle: const Text(
                'Hides the remote screen from screenshots, screen recording and the recent-apps view.',
              ),
            ),
          SwitchListTile(
            key: const Key('setting-clipboard-default'),
            value: settings.clipboardDefault,
            onChanged: (v) => settings.clipboardDefault = v,
            title: const Text('Share clipboard for new connections'),
            subtitle: const Text('Off by default. You can change it for each connection.'),
          ),
          if (_isWindows)
            SwitchListTile(
              key: const Key('setting-confirm-saved-password'),
              value: settings.confirmSavedPassword,
              onChanged: (v) => _setConfirm(context, v),
              title: const Text('Confirm with Windows Hello before using a saved password'),
              subtitle: const Text('Recommended. On Android the screen-lock check is always used.'),
            ),
          ListTile(
            key: const Key('setting-idle'),
            leading: const Icon(Icons.timer_outlined),
            title: const Text('Disconnect when idle'),
            subtitle: const Text('Ends a session when you have not used it for this long.'),
            trailing: DropdownButton<int>(
              value: idleMinuteChoices().contains(settings.idleMinutes) ? settings.idleMinutes : null,
              items: [
                for (final m in idleMinuteChoices())
                  DropdownMenuItem(value: m, child: Text(_idleLabel(m))),
              ],
              onChanged: (v) {
                if (v != null) settings.idleMinutes = v;
              },
            ),
          ),
          ListTile(
            leading: const Icon(Icons.verified_outlined),
            title: const Text('Trusted computers'),
            subtitle: const Text('Certificates you have approved'),
            onTap: () async {
              final p = await paths();
              if (!context.mounted) return;
              await Navigator.of(context).push(
                MaterialPageRoute<void>(builder: (_) => _TrustedComputersScreen(pinsPath: p.pins)),
              );
            },
          ),
          const _Header('Connections'),
          ListTile(
            leading: const Icon(Icons.history),
            title: const Text('Connection history'),
            onTap: () => Navigator.of(context).push(
              MaterialPageRoute<void>(builder: (_) => _HistoryScreen(store: store)),
            ),
          ),
          ListTile(
            key: const Key('button-export'),
            leading: const Icon(Icons.upload_file_outlined),
            title: const Text('Export connections'),
            subtitle: const Text('Names, addresses and usernames only. Passwords are never exported.'),
            onTap: () => _export(context),
          ),
          ListTile(
            key: const Key('button-import'),
            leading: const Icon(Icons.download_outlined),
            title: const Text('Import connections'),
            subtitle: const Text('Imported connections have clipboard sharing turned off.'),
            onTap: () => _import(context),
          ),
          const _Header('Security defaults'),
          for (final line in securityDefaultsSummary())
            ListTile(
              dense: true,
              leading: const Icon(Icons.verified_user_outlined),
              title: Text(line),
            ),
          ListTile(
            leading: const Icon(Icons.help_outline),
            title: const Text('What error messages mean'),
            onTap: () => Navigator.of(context).push(
              MaterialPageRoute(builder: (_) => const _ErrorHelpScreen()),
            ),
          ),
          const _Header('About'),
          ListTile(
            leading: const Icon(Icons.info_outline),
            title: const Text('Version'),
            subtitle: Text(coreSummary()),
          ),
          const ListTile(
            leading: Icon(Icons.gavel_outlined),
            title: Text('Licence'),
            subtitle: Text('Apache-2.0. Open source.'),
          ),
          ListTile(
            leading: const Icon(Icons.description_outlined),
            title: const Text('Open-source licences'),
            onTap: () => showLicensePage(context: context, applicationName: 'Secure RDP Viewer'),
          ),
        ],
      ),
    );
  }
}

class _Header extends StatelessWidget {
  const _Header(this.text);

  final String text;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 16, 16, 4),
      child: Text(
        text,
        style: Theme.of(context).textTheme.labelLarge?.copyWith(
              color: Theme.of(context).colorScheme.primary,
            ),
      ),
    );
  }
}

class _ErrorHelpScreen extends StatelessWidget {
  const _ErrorHelpScreen();

  @override
  Widget build(BuildContext context) {
    final errors = allConnectionErrors();
    return Scaffold(
      appBar: AppBar(title: const Text('Error messages')),
      body: ListView.separated(
        itemCount: errors.length,
        separatorBuilder: (_, _) => const Divider(height: 1),
        itemBuilder: (context, i) {
          final e = errors[i];
          return ListTile(
            leading: Icon(e.securityWarning ? Icons.gpp_maybe_outlined : Icons.info_outline),
            title: Text(e.title),
            subtitle: Text(e.advice),
          );
        },
      ),
    );
  }
}

class _TrustedComputersScreen extends StatefulWidget {
  const _TrustedComputersScreen({required this.pinsPath});

  final String pinsPath;

  @override
  State<_TrustedComputersScreen> createState() => _TrustedComputersScreenState();
}

class _TrustedComputersScreenState extends State<_TrustedComputersScreen> {
  late List<TrustedComputer> _items = trustedComputers(pinsPath: widget.pinsPath);

  Future<void> _forget(TrustedComputer t) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text('Forget ${t.endpoint}?'),
        content: const Text('Next time you connect you will be asked to check its certificate again.'),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('Cancel')),
          FilledButton(onPressed: () => Navigator.pop(ctx, true), child: const Text('Forget')),
        ],
      ),
    );
    if (ok != true) return;
    forgetTrustedComputer(pinsPath: widget.pinsPath, endpoint: t.endpoint);
    setState(() => _items = trustedComputers(pinsPath: widget.pinsPath));
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Trusted computers')),
      body: _items.isEmpty
          ? const Center(child: Text('No trusted computers yet.'))
          : ListView.separated(
              itemCount: _items.length,
              separatorBuilder: (_, _) => const Divider(height: 1),
              itemBuilder: (context, i) {
                final t = _items[i];
                return ListTile(
                  title: Text(t.endpoint),
                  subtitle: SelectableText(t.fingerprint,
                      style: const TextStyle(fontFamily: 'monospace', fontSize: 12)),
                  trailing: IconButton(
                    tooltip: 'Forget',
                    icon: const Icon(Icons.delete_outline),
                    onPressed: () => _forget(t),
                  ),
                );
              },
            ),
    );
  }
}

class _HistoryScreen extends StatelessWidget {
  const _HistoryScreen({required this.store});

  final ProfileStore store;

  static String _outcome(String o) => switch (o) {
        'connected' => 'Connected',
        'cancelled' => 'Cancelled before connecting',
        _ => allConnectionErrors().where((e) => e.id == o).map((e) => e.title).firstOrNull ?? o,
      };

  static String _time(DateTime t) {
    String two(int n) => n.toString().padLeft(2, '0');
    return '${t.year}-${two(t.month)}-${two(t.day)} ${two(t.hour)}:${two(t.minute)}';
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Connection history')),
      body: ListenableBuilder(
        listenable: store,
        builder: (context, _) {
          final items = store.history;
          if (items.isEmpty) return const Center(child: Text('No connections yet.'));
          return ListView.builder(
            itemCount: items.length,
            itemBuilder: (context, i) {
              final h = items[i];
              final ok = h.outcome == 'connected';
              return ListTile(
                leading: Icon(ok ? Icons.check_circle_outline : Icons.info_outline),
                title: Text(store.byId(h.connectionId)?.displayName ?? 'Deleted connection'),
                subtitle: Text('${_time(h.time)} · ${_outcome(h.outcome)}'),
              );
            },
          );
        },
      ),
    );
  }
}

/// Android: shows the export text so it can be copied (no file picker there).
class _ExportTextDialog extends StatelessWidget {
  const _ExportTextDialog({required this.text});

  final String text;

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: const Text('Export connections'),
      content: SizedBox(
        width: 400,
        child: SingleChildScrollView(
          child: SelectableText(text, style: const TextStyle(fontFamily: 'monospace', fontSize: 12)),
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context), child: const Text('Close')),
        FilledButton(
          onPressed: () {
            Clipboard.setData(ClipboardData(text: text));
            Navigator.pop(context);
          },
          child: const Text('Copy'),
        ),
      ],
    );
  }
}

/// Android: paste the text of an export.
class _ImportTextDialog extends StatefulWidget {
  const _ImportTextDialog();

  @override
  State<_ImportTextDialog> createState() => _ImportTextDialogState();
}

class _ImportTextDialogState extends State<_ImportTextDialog> {
  final _controller = TextEditingController();

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: const Text('Import connections'),
      content: TextField(
        controller: _controller,
        maxLines: 8,
        decoration: const InputDecoration(hintText: 'Paste the exported text here'),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context), child: const Text('Cancel')),
        FilledButton(onPressed: () => Navigator.pop(context, _controller.text), child: const Text('Import')),
      ],
    );
  }
}
