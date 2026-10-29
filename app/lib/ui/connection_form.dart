import 'package:flutter/material.dart';

import '../platform/password_vault.dart';
import '../src/rust/api/form.dart';
import '../state/profiles.dart';

/// Create or edit a connection (Stage 3). Returns the saved profile, or null.
///
/// The password is held only in this screen's text field. It is not stored in
/// the profile; with "Save password" ticked it will go to the OS vault (Stage 5).
class ConnectionFormScreen extends StatefulWidget {
  const ConnectionFormScreen({
    super.key,
    required this.store,
    this.existing,
    this.clipboardDefault = false,
  });

  final ProfileStore store;
  final ConnectionProfile? existing;
  final bool clipboardDefault;

  @override
  State<ConnectionFormScreen> createState() => _ConnectionFormScreenState();
}

class _ConnectionFormScreenState extends State<ConnectionFormScreen> {
  late final TextEditingController _name;
  late final TextEditingController _host;
  late final TextEditingController _port;
  late final TextEditingController _username;
  final TextEditingController _password = TextEditingController();

  bool _obscure = true;
  late bool _savePassword;
  late bool _clipboard;
  Map<String, String> _problems = const {};
  bool _submitted = false;

  @override
  void initState() {
    super.initState();
    final p = widget.existing;
    _name = TextEditingController(text: p?.name ?? '');
    _host = TextEditingController(text: p?.host ?? '');
    _port = TextEditingController(text: p == null ? '' : '${p.port}');
    _username = TextEditingController(text: p?.username ?? '');
    _savePassword = p?.savePassword ?? false;
    _clipboard = p?.clipboardEnabled ?? widget.clipboardDefault;
  }

  @override
  void dispose() {
    // Clear the password from the controller before disposing it.
    _password.clear();
    for (final c in [_name, _host, _port, _username, _password]) {
      c.dispose();
    }
    super.dispose();
  }

  FormCheckResult _check() {
    return checkConnectionForm(
      profileName: _name.text,
      host: _host.text,
      port: _port.text,
      username: _username.text,
      password: _password.text,
      // Editing a profile that already has a saved password: empty field keeps it.
      hasSavedPassword: widget.existing?.savePassword ?? false,
    );
  }

  void _revalidate() {
    if (!_submitted) return;
    final r = _check();
    setState(() => _problems = {for (final p in r.problems) p.field: p.message});
  }

  bool _saving = false;

  Future<void> _save() async {
    if (_saving) return;
    final r = _check();
    setState(() {
      _submitted = true;
      _problems = {for (final p in r.problems) p.field: p.message};
    });
    if (!r.valid) return;

    final id = widget.existing?.id ?? widget.store.newId();
    var savePassword = _savePassword;
    String? vaultMessage;
    setState(() => _saving = true);
    if (_savePassword && _password.text.isNotEmpty) {
      final outcome = await PasswordVault.save(id, _password.text);
      if (outcome != VaultOutcome.ok) {
        // Never fall back to storing it anywhere else (I8).
        savePassword = widget.existing?.savePassword ?? false;
        vaultMessage = switch (outcome) {
          VaultOutcome.locked => 'Password not saved: confirmation was cancelled.',
          _ => 'Password not saved: this device has no secure password storage. '
              'You can still connect by typing it.',
        };
      }
    } else if (!_savePassword && (widget.existing?.savePassword ?? false)) {
      await PasswordVault.delete(id);
    }
    if (!mounted) return;
    setState(() => _saving = false);

    final profile = ConnectionProfile(
      id: id,
      name: _name.text.trim(),
      host: r.host ?? _host.text.trim(),
      port: r.port ?? defaultPort(),
      username: _username.text.trim(),
      savePassword: savePassword,
      clipboardEnabled: _clipboard,
      lastUsed: widget.existing?.lastUsed,
    );
    widget.store.upsert(profile);
    _password.clear();
    final messenger = ScaffoldMessenger.of(context);
    Navigator.of(context).pop(profile);
    if (vaultMessage != null) {
      messenger.showSnackBar(SnackBar(content: Text(vaultMessage)));
    }
  }


  @override
  Widget build(BuildContext context) {
    final editing = widget.existing != null;
    return Scaffold(
      appBar: AppBar(title: Text(editing ? 'Edit connection' : 'New connection')),
      body: SafeArea(
        child: Center(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 560),
            child: ListView(
              padding: const EdgeInsets.all(16),
              children: [
                TextField(
                  key: const Key('field-name'),
                  controller: _name,
                  decoration: InputDecoration(
                    labelText: 'Name (optional)',
                    hintText: 'e.g. Office PC',
                    errorText: _problems['profileName'],
                  ),
                  textInputAction: TextInputAction.next,
                  onChanged: (_) => _revalidate(),
                ),
                const SizedBox(height: 12),
                Row(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Expanded(
                      flex: 3,
                      child: TextField(
                        key: const Key('field-host'),
                        controller: _host,
                        decoration: InputDecoration(
                          labelText: 'Computer name or IP address',
                          hintText: 'e.g. 192.0.2.10',
                          errorText: _problems['host'],
                          errorMaxLines: 3,
                        ),
                        keyboardType: TextInputType.url,
                        autocorrect: false,
                        enableSuggestions: false,
                        textInputAction: TextInputAction.next,
                        onChanged: (_) => _revalidate(),
                      ),
                    ),
                    const SizedBox(width: 12),
                    Expanded(
                      child: TextField(
                        key: const Key('field-port'),
                        controller: _port,
                        decoration: InputDecoration(
                          labelText: 'Port',
                          hintText: '${defaultPort()}',
                          errorText: _problems['port'],
                          errorMaxLines: 3,
                        ),
                        keyboardType: TextInputType.number,
                        textInputAction: TextInputAction.next,
                        onChanged: (_) => _revalidate(),
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 12),
                TextField(
                  key: const Key('field-username'),
                  controller: _username,
                  decoration: InputDecoration(
                    labelText: 'Username',
                    hintText: r'user, DOMAIN\user or user@domain',
                    errorText: _problems['username'],
                    errorMaxLines: 2,
                  ),
                  autocorrect: false,
                  enableSuggestions: false,
                  textInputAction: TextInputAction.next,
                  onChanged: (_) => _revalidate(),
                ),
                const SizedBox(height: 12),
                TextField(
                  key: const Key('field-password'),
                  controller: _password,
                  obscureText: _obscure,
                  autocorrect: false,
                  enableSuggestions: false,
                  enableIMEPersonalizedLearning: false,
                  keyboardType: TextInputType.visiblePassword,
                  decoration: InputDecoration(
                    labelText: 'Password',
                    helperText: editing && widget.existing!.savePassword
                        ? 'Leave empty to keep the saved password.'
                        : null,
                    errorText: _problems['password'],
                    suffixIcon: IconButton(
                      tooltip: _obscure ? 'Show password' : 'Hide password',
                      icon: Icon(_obscure ? Icons.visibility : Icons.visibility_off),
                      onPressed: () => setState(() => _obscure = !_obscure),
                    ),
                  ),
                  onChanged: (_) => _revalidate(),
                  onSubmitted: (_) => _save(),
                ),
                const SizedBox(height: 8),
                CheckboxListTile(
                  key: const Key('field-save-password'),
                  contentPadding: EdgeInsets.zero,
                  value: _savePassword,
                  onChanged: (v) => setState(() => _savePassword = v ?? false),
                  title: const Text('Save password'),
                  subtitle: const Text(
                    "Stored only in this device's secure password storage, never in the app's files. "
                    "You'll unlock it (fingerprint, face, PIN or your device password) before it's used.",
                  ),
                ),
                SwitchListTile(
                  key: const Key('field-clipboard'),
                  contentPadding: EdgeInsets.zero,
                  value: _clipboard,
                  onChanged: (v) => setState(() => _clipboard = v),
                  title: const Text('Share clipboard'),
                  subtitle: const Text(
                    'Copy and paste text between this device and the remote computer. '
                    'Anything you copy can be read by the remote computer.',
                  ),
                ),
                const SizedBox(height: 16),
                FilledButton.icon(
                  key: const Key('button-save'),
                  onPressed: _saving ? null : _save,
                  icon: const Icon(Icons.save_outlined),
                  label: Text(editing ? 'Save changes' : 'Save connection'),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
