import 'package:flutter/material.dart';

import '../src/rust/api/store.dart' show ConnectionMethod;

/// Names, explanations and icons of the three connection methods, and the
/// "Which method should I use?" helper.
extension ConnectionMethodText on ConnectionMethod {
  String get label => switch (this) {
        ConnectionMethod.privateRemoteDesktop => 'Private Remote Desktop',
        ConnectionMethod.shareScreen => 'Share screen',
        ConnectionMethod.vnc => 'VNC',
      };

  String get shortLabel => switch (this) {
        ConnectionMethod.privateRemoteDesktop => 'Private',
        ConnectionMethod.shareScreen => 'Share',
        ConnectionMethod.vnc => 'VNC',
      };

  String get description => switch (this) {
        ConnectionMethod.privateRemoteDesktop =>
          "Windows Remote Desktop. The computer's own screen locks while you work, so "
              'nobody there can see it. Needs Windows Pro, Enterprise or Education.',
        ConnectionMethod.shareScreen =>
          'The same screen is shown on the computer and on this device, so someone there '
              'can follow along. Needs Cipher Desk running on the computer with '
              '"Share this screen" turned on.',
        ConnectionMethod.vnc =>
          'The same screen at both ends, through a VNC program on the computer (for '
              'example TightVNC). VNC is less secure, so it is only allowed on your local '
              'network or through a VPN such as Tailscale.',
      };

  IconData get icon => switch (this) {
        ConnectionMethod.privateRemoteDesktop => Icons.lock_outline,
        ConnectionMethod.shareScreen => Icons.screen_share_outlined,
        ConnectionMethod.vnc => Icons.cast_connected_outlined,
      };

  /// VNC signs in with a password only.
  bool get needsUsername => this != ConnectionMethod.vnc;

  /// Clipboard sharing works with Windows Remote Desktop only (for now).
  bool get supportsClipboard => this == ConnectionMethod.privateRemoteDesktop;
}

/// Asks two short questions and returns the suggested method, or null if cancelled.
Future<ConnectionMethod?> showMethodHelper(BuildContext context) {
  return showDialog<ConnectionMethod>(
    context: context,
    builder: (_) => const _MethodHelper(),
  );
}

class _MethodHelper extends StatefulWidget {
  const _MethodHelper();

  @override
  State<_MethodHelper> createState() => _MethodHelperState();
}

class _MethodHelperState extends State<_MethodHelper> {
  // Answers so far: null = not answered yet.
  bool? _othersMustSee;
  bool? _second;

  ConnectionMethod? get _result {
    final see = _othersMustSee;
    final second = _second;
    if (see == null || second == null) return null;
    if (!see) {
      // Private: Windows Pro can do it itself; other editions use Cipher Desk Host.
      return second ? ConnectionMethod.privateRemoteDesktop : ConnectionMethod.shareScreen;
    }
    // Shared: Cipher Desk on the computer if possible, otherwise VNC.
    return second ? ConnectionMethod.shareScreen : ConnectionMethod.vnc;
  }

  Widget _question(String text, bool? value, ValueChanged<bool> onAnswer, {Key? key}) {
    return Column(
      key: key,
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(text, style: Theme.of(context).textTheme.titleSmall),
        const SizedBox(height: 8),
        SegmentedButton<bool>(
          segments: const [
            ButtonSegment(value: true, label: Text('Yes')),
            ButtonSegment(value: false, label: Text('No')),
          ],
          emptySelectionAllowed: true,
          selected: {if (value != null) value},
          onSelectionChanged: (s) {
            if (s.isNotEmpty) onAnswer(s.first);
          },
        ),
        const SizedBox(height: 16),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    final see = _othersMustSee;
    final result = _result;
    return AlertDialog(
      title: const Text('Which method should I use?'),
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            _question(
              'Does someone at the computer need to see what you are doing?',
              see,
              (v) => setState(() {
                _othersMustSee = v;
                _second = null;
              }),
              key: const Key('helper-q1'),
            ),
            if (see == false)
              _question(
                'Is the computer Windows Pro, Enterprise or Education? '
                '(Settings → System → About → Edition)',
                _second,
                (v) => setState(() => _second = v),
                key: const Key('helper-q2'),
              ),
            if (see == true)
              _question(
                'Can you install Cipher Desk on that computer (Windows)?',
                _second,
                (v) => setState(() => _second = v),
                key: const Key('helper-q2'),
              ),
            if (result != null)
              Card(
                key: const Key('helper-result'),
                child: ListTile(
                  leading: Icon(result.icon),
                  title: Text('Use ${result.label}'),
                  subtitle: Text(result.description),
                ),
              ),
          ],
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context), child: const Text('Cancel')),
        FilledButton(
          key: const Key('helper-use'),
          onPressed: result == null ? null : () => Navigator.pop(context, result),
          child: const Text('Use this method'),
        ),
      ],
    );
  }
}
