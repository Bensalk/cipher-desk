import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../state/share.dart';

/// "Share this screen" (Windows): lets a phone or another computer see and
/// control this screen with the Share screen method, while it stays visible
/// and usable here.
class ShareScreenPage extends StatefulWidget {
  const ShareScreenPage({super.key, required this.share});

  final ShareController share;

  @override
  State<ShareScreenPage> createState() => _ShareScreenPageState();
}

class _ShareScreenPageState extends State<ShareScreenPage> {
  late final _user = TextEditingController(text: widget.share.username);
  late final _port = TextEditingController(text: '${widget.share.port}');
  String? _shownPassword;
  List<String> _addresses = const [];

  @override
  void initState() {
    super.initState();
    _loadAddresses();
  }

  @override
  void dispose() {
    _user.dispose();
    _port.dispose();
    super.dispose();
  }

  Future<void> _loadAddresses() async {
    try {
      final list = await NetworkInterface.list(type: InternetAddressType.IPv4);
      final found = <String>[
        for (final i in list)
          for (final a in i.addresses)
            if (!a.isLoopback) '${a.address}  (${i.name})',
      ];
      // Tailscale first: it is the one to use away from home.
      found.sort((a, b) => (b.startsWith('100.') ? 1 : 0) - (a.startsWith('100.') ? 1 : 0));
      if (mounted) setState(() => _addresses = found);
    } on Object {
      // Addresses are only a hint.
    }
  }

  void _snack(String text) =>
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(text)));

  Future<void> _copy(String label, String text) async {
    await Clipboard.setData(ClipboardData(text: text));
    _snack('$label copied');
  }

  Future<void> _confirmNewPassword() async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Make a new password?'),
        content: const Text(
            'Phones and computers that saved the old password will need the new one.'),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context, false), child: const Text('Cancel')),
          FilledButton(
            key: const Key('share-new-password-confirm'),
            onPressed: () => Navigator.pop(context, true),
            child: const Text('New password'),
          ),
        ],
      ),
    );
    if (ok == true) {
      widget.share.newPassword();
      setState(() => _shownPassword = null);
    }
  }

  Future<void> _saveSettings() async {
    final port = int.tryParse(_port.text.trim());
    if (_user.text.trim().isEmpty || port == null || port < 1 || port > 65535) {
      _snack('Enter a username and a port from 1 to 65535.');
      return;
    }
    await widget.share.update(username: _user.text, port: port);
    _snack(widget.share.isOn ? 'Saved. Stop and start sharing to use the new settings.' : 'Saved');
  }

  @override
  Widget build(BuildContext context) {
    final s = widget.share;
    final scheme = Theme.of(context).colorScheme;
    return ListenableBuilder(
      listenable: s,
      builder: (context, _) => ListView(
        padding: const EdgeInsets.all(16),
        children: [
          Text('Share this screen', style: Theme.of(context).textTheme.headlineSmall),
          const SizedBox(height: 4),
          const Text(
            'Lets your phone or another computer see and control this screen. '
            'The screen stays on and usable here, so people at this computer see everything.',
          ),
          const SizedBox(height: 16),
          _StatusCard(share: s),
          const SizedBox(height: 16),
          Text('To connect, enter these in Cipher Desk on the other device',
              style: Theme.of(context).textTheme.titleMedium),
          const SizedBox(height: 4),
          const Text('New connection → method "Share screen".'),
          const SizedBox(height: 8),
          Card(
            child: Column(
              children: [
                ListTile(
                  leading: const Icon(Icons.lan_outlined),
                  title: const Text('Computer address'),
                  subtitle: Text(_addresses.isEmpty
                      ? 'Not connected to a network'
                      : _addresses.join('\n')),
                  isThreeLine: _addresses.length > 1,
                ),
                ListTile(
                  leading: const Icon(Icons.numbers),
                  title: const Text('Port'),
                  subtitle: Text('${s.port}'),
                ),
                ListTile(
                  leading: const Icon(Icons.person_outline),
                  title: const Text('Username'),
                  subtitle: Text(s.username),
                  trailing: IconButton(
                    tooltip: 'Copy username',
                    icon: const Icon(Icons.copy),
                    onPressed: () => _copy('Username', s.username),
                  ),
                ),
                ListTile(
                  key: const Key('share-password'),
                  leading: const Icon(Icons.password),
                  title: const Text('Password'),
                  subtitle: Text(!s.hasPassword
                      ? 'No password yet'
                      : (_shownPassword ?? '•••• •••• •••• ••••')),
                  trailing: Wrap(
                    children: [
                      IconButton(
                        key: const Key('share-password-show'),
                        tooltip: _shownPassword == null ? 'Show password' : 'Hide password',
                        icon: Icon(_shownPassword == null ? Icons.visibility : Icons.visibility_off),
                        onPressed: !s.hasPassword
                            ? null
                            : () => setState(() =>
                                _shownPassword = _shownPassword == null ? s.revealPassword() : null),
                      ),
                      IconButton(
                        key: const Key('share-new-password'),
                        tooltip: 'Make a new password',
                        icon: const Icon(Icons.refresh),
                        onPressed: _confirmNewPassword,
                      ),
                    ],
                  ),
                ),
                ListTile(
                  leading: const Icon(Icons.verified_user_outlined),
                  title: const Text('Identity code'),
                  subtitle: Text(
                    s.fingerprint ?? 'Not available',
                    style: const TextStyle(fontFamily: 'monospace', fontSize: 12),
                  ),
                  trailing: s.fingerprint == null
                      ? null
                      : IconButton(
                          tooltip: 'Copy identity code',
                          icon: const Icon(Icons.copy),
                          onPressed: () => _copy('Identity code', s.fingerprint!),
                        ),
                ),
                const Padding(
                  padding: EdgeInsets.fromLTRB(16, 0, 16, 12),
                  child: Text(
                    'The first time, the other device shows an identity code. '
                    'Connect only if it matches the one above.',
                  ),
                ),
              ],
            ),
          ),
          const SizedBox(height: 16),
          Text('Settings', style: Theme.of(context).textTheme.titleMedium),
          const SizedBox(height: 8),
          TextField(
            key: const Key('share-username'),
            controller: _user,
            decoration: const InputDecoration(
              labelText: 'Username for viewers',
              helperText: 'Any name. It is not your Windows account.',
              border: OutlineInputBorder(),
            ),
          ),
          const SizedBox(height: 12),
          TextField(
            key: const Key('share-port'),
            controller: _port,
            keyboardType: TextInputType.number,
            decoration: const InputDecoration(
              labelText: 'Port',
              helperText: 'Usually 3395.',
              border: OutlineInputBorder(),
            ),
          ),
          const SizedBox(height: 8),
          Align(
            alignment: Alignment.centerRight,
            child: FilledButton.tonal(onPressed: _saveSettings, child: const Text('Save')),
          ),
          SwitchListTile(
            key: const Key('share-auto-start'),
            title: const Text('Start sharing when Cipher Desk opens'),
            value: s.startWhenAppOpens,
            onChanged: (v) => s.update(startWhenAppOpens: v),
          ),
          SwitchListTile(
            key: const Key('share-sign-in'),
            title: const Text('Open Cipher Desk when I sign in to Windows'),
            subtitle: const Text('Together with the switch above, sharing is ready after a restart.'),
            value: s.openAtSignIn,
            onChanged: (v) async {
              if (!await s.setOpenAtSignIn(v) && mounted) {
                _snack('Windows did not allow this change.');
              }
            },
          ),
          const SizedBox(height: 8),
          Card(
            color: scheme.secondaryContainer,
            child: const Padding(
              padding: EdgeInsets.all(12),
              child: Text(
                'Good to know\n'
                '• Keep this computer signed in and unlocked. Windows hides the lock '
                'screen, password prompts and admin windows from sharing apps.\n'
                '• The Cipher Desk window itself looks black to the viewer while '
                '"Block screenshots" is on, so the password is never shown to them.\n'
                '• Use it on your own network or through Tailscale. Do not open the '
                'port on your router.',
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _StatusCard extends StatelessWidget {
  const _StatusCard({required this.share});

  final ShareController share;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final (icon, title, color) = switch (share.status) {
      ShareStatus.off => (Icons.desktop_access_disabled, 'Sharing is off', scheme.surfaceContainerHighest),
      ShareStatus.starting => (Icons.hourglass_top, 'Starting…', scheme.surfaceContainerHighest),
      ShareStatus.waiting => (Icons.desktop_windows, 'Sharing is on: waiting for a viewer', scheme.primaryContainer),
      ShareStatus.viewing => (Icons.visibility, 'Someone is viewing and controlling this screen', scheme.tertiaryContainer),
      ShareStatus.lockedOut => (Icons.lock_clock, 'Sharing is on, but paused after wrong passwords', scheme.errorContainer),
      ShareStatus.failed => (Icons.error_outline, 'Sharing stopped', scheme.errorContainer),
    };
    return Card(
      color: color,
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Icon(icon),
                const SizedBox(width: 12),
                Expanded(
                  child: Text(title,
                      key: const Key('share-status'),
                      style: Theme.of(context).textTheme.titleMedium),
                ),
              ],
            ),
            if (share.message != null) ...[
              const SizedBox(height: 8),
              Text(share.message!),
            ],
            const SizedBox(height: 12),
            share.isOn
                ? FilledButton.icon(
                    key: const Key('share-stop'),
                    style: FilledButton.styleFrom(backgroundColor: scheme.error, foregroundColor: scheme.onError),
                    onPressed: share.stop,
                    icon: const Icon(Icons.stop),
                    label: const Text('Stop sharing'),
                  )
                : FilledButton.icon(
                    key: const Key('share-start'),
                    onPressed: share.hasPassword ? share.start : null,
                    icon: const Icon(Icons.play_arrow),
                    label: const Text('Start sharing'),
                  ),
          ],
        ),
      ),
    );
  }
}

/// Shown at the top of every page while someone is viewing this screen.
class SharingBanner extends StatelessWidget {
  const SharingBanner({super.key, required this.share});

  final ShareController share;

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: share,
      builder: (context, _) {
        if (share.status != ShareStatus.viewing) return const SizedBox.shrink();
        final scheme = Theme.of(context).colorScheme;
        return Material(
          color: scheme.tertiaryContainer,
          child: SafeArea(
            bottom: false,
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 6),
              child: Row(
                children: [
                  const Icon(Icons.visibility),
                  const SizedBox(width: 12),
                  const Expanded(child: Text('Your screen is being shared')),
                  TextButton(
                    key: const Key('share-banner-stop'),
                    onPressed: share.stop,
                    child: const Text('Stop'),
                  ),
                ],
              ),
            ),
          ),
        );
      },
    );
  }
}
