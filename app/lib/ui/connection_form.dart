import 'package:flutter/material.dart';

import '../platform/password_vault.dart';
import '../src/rust/api/store.dart';
import '../src/rust/api/form.dart';
import '../src/rust/api/session.dart' show ClipboardMode;
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
  late final TextEditingController _mac;

  bool _obscure = true;
  late bool _savePassword;
  late ClipboardMode _clipboard;
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
    _mac = TextEditingController(text: p?.wakeMac ?? '');
    _savePassword = p?.savePassword ?? false;
    _clipboard = p?.clipboardMode ??
        (widget.clipboardDefault ? ClipboardMode.both : ClipboardMode.off);
  }

  @override
  void dispose() {
    // Clear the password from the controller before disposing it.
    _password.clear();
    for (final c in [_name, _host, _port, _username, _password, _mac]) {
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

  /// Problems from the Rust form check plus the optional Wake-on-LAN address.
  Map<String, String> _allProblems(FormCheckResult r) => {
        for (final p in r.problems) p.field: p.message,
        if (_mac.text.trim().isNotEmpty && !isValidMac(text: _mac.text.trim()))
          'mac': 'Enter a MAC address like 00:11:22:33:44:55, or leave it empty.',
      };

  void _revalidate() {
    if (!_submitted) return;
    final r = _check();
    setState(() => _problems = _allProblems(r));
  }

  bool _saving = false;

  Future<void> _save() async {
    if (_saving) return;
    final r = _check();
    setState(() {
      _submitted = true;
      _problems = _allProblems(r);
    });
    if (!r.valid || _problems.isNotEmpty) return;

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
          VaultOutcome.locked =>
              "Password not saved: it couldn't be confirmed that it's you (cancelled, or Windows Hello isn't set up).",
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
      clipboardMode: _clipboard,
      lastUsed: widget.existing?.lastUsed,
      wakeMac: _mac.text.trim().isEmpty ? null : _mac.text.trim(),
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
                    "On Windows and Android you confirm it's you (Windows Hello, fingerprint, face or "
                    'screen lock) before it is used. On Linux it is protected by your keyring, which '
                    'is usually unlocked when you log in.',
                  ),
                ),
                const SizedBox(height: 8),
                DropdownButtonFormField<ClipboardMode>(
                  key: const Key('field-clipboard'),
                  initialValue: _clipboard,
                  decoration: const InputDecoration(
                    labelText: 'Share clipboard (text only)',
                    helperText: 'Anything you copy while connected can be read by the remote '
                        'computer, and the remote computer can put text on this device.',
                    helperMaxLines: 3,
                  ),
                  items: const [
                    DropdownMenuItem(value: ClipboardMode.off, child: Text('Off')),
                    DropdownMenuItem(value: ClipboardMode.both, child: Text('Both directions')),
                    DropdownMenuItem(
                      value: ClipboardMode.toRemoteOnly,
                      child: Text('Only from this device to the remote computer'),
                    ),
                    DropdownMenuItem(
                      value: ClipboardMode.fromRemoteOnly,
                      child: Text('Only from the remote computer to this device'),
                    ),
                  ],
                  onChanged: (v) => setState(() => _clipboard = v ?? ClipboardMode.off),
                ),
                const SizedBox(height: 16),
                TextField(
                  key: const Key('field-mac'),
                  controller: _mac,
                  autocorrect: false,
                  enableSuggestions: false,
                  onChanged: (_) => _revalidate(),
                  decoration: InputDecoration(
                    labelText: 'Wake-on-LAN MAC address (optional)',
                    hintText: '00:11:22:33:44:55',
                    helperText: 'Lets you wake the computer from the connection menu. '
                        'Works only on the same local network, and Wake-on-LAN must be turned on '
                        'in the computer\'s settings.',
                    helperMaxLines: 3,
                    errorText: _problems['mac'],
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
