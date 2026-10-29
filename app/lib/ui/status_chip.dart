import 'package:flutter/material.dart';

/// Connection states shown by [StatusChip] (mirrors rdpv-core `Status`).
enum RdpConnectionState {
  disconnected('Not connected', Icons.link_off),
  connecting('Connecting…', Icons.sync),
  checkIdentity('Check computer identity', Icons.verified_user_outlined),
  connected('Connected', Icons.link),
  reconnecting('Reconnecting…', Icons.sync_problem),
  disconnecting('Disconnecting…', Icons.link_off),
  failed('Connection failed', Icons.error_outline);

  const RdpConnectionState(this.label, this.icon);
  final String label;
  final IconData icon;
}

/// Small coloured indicator of the connection state.
class StatusChip extends StatelessWidget {
  const StatusChip({super.key, required this.state});

  final RdpConnectionState state;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final (Color bg, Color fg) = switch (state) {
      RdpConnectionState.connected => (scheme.primaryContainer, scheme.onPrimaryContainer),
      RdpConnectionState.failed => (scheme.errorContainer, scheme.onErrorContainer),
      RdpConnectionState.checkIdentity => (scheme.tertiaryContainer, scheme.onTertiaryContainer),
      _ => (scheme.surfaceContainerHighest, scheme.onSurfaceVariant),
    };
    return Semantics(
      label: 'Connection status: ${state.label}',
      child: Chip(
        avatar: Icon(state.icon, size: 18, color: fg),
        label: Text(state.label, style: TextStyle(color: fg)),
        backgroundColor: bg,
        side: BorderSide.none,
      ),
    );
  }
}
