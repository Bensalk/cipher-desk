import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';

import '../platform/secure_window.dart';
import '../src/rust/api/errors.dart';
import '../src/rust/api/info.dart';
import '../state/settings.dart';

/// Settings and About (Stage 3). Values are in memory until Stage 7.
class SettingsScreen extends StatelessWidget {
  const SettingsScreen({super.key, required this.settings});

  final AppSettings settings;

  @override
  Widget build(BuildContext context) {
    final isAndroid = !kIsWeb && defaultTargetPlatform == TargetPlatform.android;
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
