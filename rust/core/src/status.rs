//! Connection status shown to the user, and the plain-language error catalogue.
//!
//! Error messages are fixed text: they never include hostnames, usernames,
//! passwords or raw library error strings, so they are safe to display and log.

use core::fmt;

/// Why a connection failed or ended. Every engine error is mapped to one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectionError {
    /// Wrong username or password (NLA logon failure).
    WrongCredentials,
    /// The account is locked, disabled or expired, or its password must be changed.
    AccountProblem,
    /// The host refused the connection (nothing listening on the port).
    HostRefused,
    /// No answer before the connect timeout.
    Timeout,
    /// The name could not be resolved.
    HostNotFound,
    /// The network dropped during the session.
    ConnectionLost,
    /// First connection to this host: the user must approve the certificate.
    CertificateUnknown,
    /// The user chose not to trust the host's certificate. Nothing was sent.
    CertificateRejected,
    /// The host's certificate differs from the pinned one. Possible interception.
    CertificateChanged,
    /// The host does not support NLA or TLS 1.2+.
    SecurityNotSupported,
    /// Windows Home or Remote Desktop disabled on the host.
    RemoteDesktopUnavailable,
    /// The host or an administrator ended the session.
    DisconnectedByHost,
    /// Another user signed in to the same account on the host.
    SessionTakenOver,
    /// The idle timeout ended the session.
    IdleTimeout,
    /// The OS vault could not be used (e.g. no keyring on Linux).
    VaultUnavailable,
    /// Android: the app stayed in the background longer than the grace period.
    AppInBackground,
    /// Anything not covered above.
    Unexpected,
}

impl ConnectionError {
    /// Every variant, for tests and the UI's help screen.
    pub const ALL: [Self; 17] = [
        Self::WrongCredentials,
        Self::AccountProblem,
        Self::HostRefused,
        Self::Timeout,
        Self::HostNotFound,
        Self::ConnectionLost,
        Self::CertificateUnknown,
        Self::CertificateRejected,
        Self::CertificateChanged,
        Self::SecurityNotSupported,
        Self::RemoteDesktopUnavailable,
        Self::DisconnectedByHost,
        Self::SessionTakenOver,
        Self::IdleTimeout,
        Self::VaultUnavailable,
        Self::AppInBackground,
        Self::Unexpected,
    ];

    /// Short title for the error banner.
    pub const fn title(self) -> &'static str {
        match self {
            Self::WrongCredentials => "Wrong username or password",
            Self::AccountProblem => "Account can't sign in",
            Self::HostRefused => "Computer refused the connection",
            Self::Timeout => "Computer didn't respond",
            Self::HostNotFound => "Computer not found",
            Self::ConnectionLost => "Connection lost",
            Self::CertificateUnknown => "New computer — check its identity",
            Self::CertificateRejected => "Connection cancelled",
            Self::CertificateChanged => "Warning: computer identity changed",
            Self::SecurityNotSupported => "Secure connection not supported",
            Self::RemoteDesktopUnavailable => "Remote Desktop isn't available",
            Self::DisconnectedByHost => "Disconnected by the remote computer",
            Self::SessionTakenOver => "Someone else signed in",
            Self::IdleTimeout => "Disconnected after inactivity",
            Self::VaultUnavailable => "Saved password couldn't be used",
            Self::AppInBackground => "Disconnected while in the background",
            Self::Unexpected => "Something went wrong",
        }
    }

    /// What happened and what to do next.
    pub const fn advice(self) -> &'static str {
        match self {
            Self::WrongCredentials => "Check the username and password and try again. Saved passwords are not retried automatically, to avoid locking the account.",
            Self::AccountProblem => "The account may be locked, disabled or need a new password. Sign in on the computer itself or ask its administrator.",
            Self::HostRefused => "Make sure Remote Desktop is turned on for that computer and the port is correct.",
            Self::Timeout => "Check that the computer is on and awake, that you are on the same network or connected to the VPN, and that a firewall isn't blocking the connection.",
            Self::HostNotFound => "Check the computer name or use its IP address.",
            Self::ConnectionLost => "The network connection dropped and reconnecting automatically didn't work. Check your network or VPN, then reconnect.",
            Self::CertificateUnknown => "Compare the fingerprint shown with the one on the remote computer before trusting it. Only continue if they match.",
            Self::CertificateRejected => "You chose not to trust this computer's certificate, so the connection was stopped before your password was sent.",
            Self::CertificateChanged => "Its security certificate is different from last time. This can happen after reinstalling Windows, but it can also mean someone is intercepting the connection. The connection was blocked and no password was sent.",
            Self::SecurityNotSupported => "The computer must allow Network Level Authentication (NLA) and TLS 1.2 or later. Older, less secure methods are not used.",
            Self::RemoteDesktopUnavailable => "Remote Desktop needs Windows Pro, Enterprise or Education, with Remote Desktop turned on in Settings.",
            Self::DisconnectedByHost => "The session was ended on the remote computer. You can reconnect.",
            Self::SessionTakenOver => "Another sign-in to the same account ended this session. You can reconnect, which will end theirs.",
            Self::IdleTimeout => "The session was idle for too long. Reconnect when you need it.",
            Self::VaultUnavailable => "The saved password couldn't be read from this device's secure password storage (it may be locked, missing or unavailable). Type the password to connect, or edit the connection to save it again.",
            Self::AppInBackground => "To save battery and keep the connection secure, a session ends 5 minutes after you leave the app. Reconnect to continue.",
            Self::Unexpected => "Try again. If it keeps happening, restart the app.",
        }
    }

    /// Whether the app may reconnect automatically after this error (SA-8, F13).
    ///
    /// Only network problems qualify. Anything about the password or account,
    /// the certificate, or a decision by the user or the remote computer never
    /// does, so a wrong password is never retried (account lockout).
    /// `HostRefused` and `HostNotFound` only arise while reconnecting (the
    /// computer restarting, or the network still coming back).
    pub const fn auto_reconnect_allowed(self) -> bool {
        matches!(
            self,
            Self::ConnectionLost | Self::Timeout | Self::HostRefused | Self::HostNotFound
        )
    }

    /// Wait before reconnect attempt `attempt` (1-based): 2 s, 5 s, then 10 s.
    pub const fn reconnect_delay(attempt: u8) -> core::time::Duration {
        core::time::Duration::from_secs(match attempt {
            0 | 1 => 2,
            2 => 5,
            _ => 10,
        })
    }

    /// Whether the UI offers a one-tap "Reconnect" after this error. Not after
    /// password, account, vault or certificate problems, or when the computer
    /// cannot host sessions: those need the user to act first.
    pub const fn user_can_reconnect(self) -> bool {
        !matches!(
            self,
            Self::WrongCredentials
                | Self::AccountProblem
                | Self::CertificateUnknown
                | Self::CertificateRejected
                | Self::CertificateChanged
                | Self::SecurityNotSupported
                | Self::RemoteDesktopUnavailable
                | Self::VaultUnavailable
        )
    }

    /// Whether this error is a security warning that needs extra emphasis.
    pub const fn is_security_warning(self) -> bool {
        matches!(
            self,
            Self::CertificateChanged | Self::CertificateUnknown | Self::SecurityNotSupported
        )
    }
}

impl fmt::Display for ConnectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.title())
    }
}

/// The connection status shown in the status indicator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Disconnected,
    Connecting,
    /// Waiting for the user to approve or reject the host's certificate.
    AwaitingCertificateApproval,
    Connected,
    Reconnecting {
        attempt: u8,
    },
    Disconnecting,
    Failed(ConnectionError),
}

/// Events that move the status from one state to another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    ConnectRequested,
    CertificateNeedsApproval,
    CertificateApproved,
    CertificateRejected,
    SessionReady,
    Error(ConnectionError),
    DisconnectRequested,
    Closed,
}

/// Returned when an event is not valid in the current status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidTransition {
    pub from: Status,
    pub event: Event,
}

impl Status {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Disconnected => "Not connected",
            Self::Connecting => "Connecting…",
            Self::AwaitingCertificateApproval => "Check computer identity",
            Self::Connected => "Connected",
            Self::Reconnecting { .. } => "Reconnecting…",
            Self::Disconnecting => "Disconnecting…",
            Self::Failed(_) => "Connection failed",
        }
    }

    /// Applies an event. `max_reconnects` comes from the security policy.
    pub fn next(self, event: Event, max_reconnects: u8) -> Result<Self, InvalidTransition> {
        use Event as E;
        use Status as S;
        let to = match (self, event) {
            (S::Disconnected | S::Failed(_), E::ConnectRequested) => S::Connecting,
            (S::Connecting | S::Reconnecting { .. }, E::CertificateNeedsApproval) => {
                S::AwaitingCertificateApproval
            }
            (S::AwaitingCertificateApproval, E::CertificateApproved) => S::Connecting,
            (S::AwaitingCertificateApproval, E::CertificateRejected) => {
                S::Failed(ConnectionError::CertificateRejected)
            }
            (S::Connecting | S::Reconnecting { .. }, E::SessionReady) => S::Connected,
            (S::Connected, E::Error(err)) if err.auto_reconnect_allowed() && max_reconnects > 0 => {
                S::Reconnecting { attempt: 1 }
            }
            (S::Reconnecting { attempt }, E::Error(err))
                if err.auto_reconnect_allowed() && attempt < max_reconnects =>
            {
                S::Reconnecting {
                    attempt: attempt + 1,
                }
            }
            (
                S::Connecting
                | S::Connected
                | S::Reconnecting { .. }
                | S::AwaitingCertificateApproval,
                E::Error(err),
            ) => S::Failed(err),
            (
                S::Connecting
                | S::Connected
                | S::Reconnecting { .. }
                | S::AwaitingCertificateApproval,
                E::DisconnectRequested,
            ) => S::Disconnecting,
            (S::Disconnecting | S::Connected, E::Closed) => S::Disconnected,
            // An error while already disconnecting just finishes the disconnect.
            (S::Disconnecting, E::Error(_)) => S::Disconnected,
            (from, event) => return Err(InvalidTransition { from, event }),
        };
        Ok(to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAX: u8 = 3;

    fn run(events: &[Event]) -> Result<Status, InvalidTransition> {
        events
            .iter()
            .try_fold(Status::Disconnected, |s, e| s.next(*e, MAX))
    }

    #[test]
    fn normal_connect_and_disconnect() {
        let s = run(&[Event::ConnectRequested, Event::SessionReady]);
        assert_eq!(s, Ok(Status::Connected));
        let s = run(&[
            Event::ConnectRequested,
            Event::SessionReady,
            Event::DisconnectRequested,
            Event::Closed,
        ]);
        assert_eq!(s, Ok(Status::Disconnected));
    }

    #[test]
    fn first_connection_needs_certificate_approval() {
        let s = run(&[Event::ConnectRequested, Event::CertificateNeedsApproval]);
        assert_eq!(s, Ok(Status::AwaitingCertificateApproval));
        let rejected = run(&[
            Event::ConnectRequested,
            Event::CertificateNeedsApproval,
            Event::CertificateRejected,
        ]);
        assert_eq!(
            rejected,
            Ok(Status::Failed(ConnectionError::CertificateRejected))
        );
        let approved = run(&[
            Event::ConnectRequested,
            Event::CertificateNeedsApproval,
            Event::CertificateApproved,
            Event::SessionReady,
        ]);
        assert_eq!(approved, Ok(Status::Connected));
    }

    #[test]
    fn network_drop_reconnects_up_to_the_limit() {
        let lost = Event::Error(ConnectionError::ConnectionLost);
        let s = run(&[Event::ConnectRequested, Event::SessionReady, lost]);
        assert_eq!(s, Ok(Status::Reconnecting { attempt: 1 }));
        let s = run(&[
            Event::ConnectRequested,
            Event::SessionReady,
            lost,
            lost,
            lost,
        ]);
        assert_eq!(s, Ok(Status::Reconnecting { attempt: 3 }));
        let s = run(&[
            Event::ConnectRequested,
            Event::SessionReady,
            lost,
            lost,
            lost,
            lost,
        ]);
        assert_eq!(s, Ok(Status::Failed(ConnectionError::ConnectionLost)));
        let s = run(&[
            Event::ConnectRequested,
            Event::SessionReady,
            lost,
            Event::SessionReady,
        ]);
        assert_eq!(s, Ok(Status::Connected));
    }

    #[test]
    fn wrong_password_never_auto_retries() {
        let wrong = Event::Error(ConnectionError::WrongCredentials);
        assert_eq!(
            run(&[Event::ConnectRequested, wrong]),
            Ok(Status::Failed(ConnectionError::WrongCredentials))
        );
        let lost = Event::Error(ConnectionError::ConnectionLost);
        assert_eq!(
            run(&[Event::ConnectRequested, Event::SessionReady, lost, wrong]),
            Ok(Status::Failed(ConnectionError::WrongCredentials))
        );
        assert!(!ConnectionError::WrongCredentials.auto_reconnect_allowed());
        assert!(!ConnectionError::AccountProblem.auto_reconnect_allowed());
        assert!(!ConnectionError::CertificateChanged.auto_reconnect_allowed());
    }

    #[test]
    fn certificate_change_fails_immediately() {
        let changed = Event::Error(ConnectionError::CertificateChanged);
        assert_eq!(
            run(&[Event::ConnectRequested, changed]),
            Ok(Status::Failed(ConnectionError::CertificateChanged))
        );
        assert!(ConnectionError::CertificateChanged.is_security_warning());
    }

    #[test]
    fn no_reconnect_when_policy_disables_it() {
        let s = Status::Connected.next(Event::Error(ConnectionError::ConnectionLost), 0);
        assert_eq!(s, Ok(Status::Failed(ConnectionError::ConnectionLost)));
    }

    #[test]
    fn invalid_transitions_are_rejected() {
        assert!(Status::Disconnected.next(Event::SessionReady, MAX).is_err());
        assert!(Status::Connected
            .next(Event::ConnectRequested, MAX)
            .is_err());
        assert!(Status::Disconnected
            .next(Event::CertificateApproved, MAX)
            .is_err());
    }

    #[test]
    fn retry_after_failure_is_allowed() {
        let s = Status::Failed(ConnectionError::Timeout).next(Event::ConnectRequested, MAX);
        assert_eq!(s, Ok(Status::Connecting));
    }

    #[test]
    fn every_error_has_safe_text() {
        for e in ConnectionError::ALL {
            assert!(!e.title().is_empty() && !e.advice().is_empty(), "{e:?}");
            assert!(
                e.title().len() <= 48,
                "title too long for the banner: {e:?}"
            );
            // Fixed text only: no format placeholders that could carry user data.
            assert!(!e.advice().contains('{') && !e.title().contains('{'));
        }
    }

    #[test]
    fn status_labels_exist() {
        let all = [
            Status::Disconnected,
            Status::Connecting,
            Status::AwaitingCertificateApproval,
            Status::Connected,
            Status::Reconnecting { attempt: 1 },
            Status::Disconnecting,
            Status::Failed(ConnectionError::Unexpected),
        ];
        for s in all {
            assert!(!s.label().is_empty());
        }
    }

    #[test]
    fn error_while_disconnecting_finishes_the_disconnect() {
        let s = run(&[
            Event::ConnectRequested,
            Event::SessionReady,
            Event::DisconnectRequested,
            Event::Error(ConnectionError::ConnectionLost),
        ]);
        assert_eq!(s, Ok(Status::Disconnected));
    }

    #[test]
    fn rejected_certificate_never_retries() {
        assert!(!ConnectionError::CertificateRejected.auto_reconnect_allowed());
    }

    /// Error matrix (Stage 8): for every error, whether it is retried
    /// automatically, whether "Reconnect" is offered, and whether it is shown
    /// as a security warning. Adding an error forces a decision here.
    #[test]
    fn error_matrix() {
        use ConnectionError as E;
        // (error, auto-retry, reconnect button, security warning)
        let matrix = [
            (E::WrongCredentials, false, false, false),
            (E::AccountProblem, false, false, false),
            (E::HostRefused, true, true, false),
            (E::Timeout, true, true, false),
            (E::HostNotFound, true, true, false),
            (E::ConnectionLost, true, true, false),
            (E::CertificateUnknown, false, false, true),
            (E::CertificateRejected, false, false, false),
            (E::CertificateChanged, false, false, true),
            (E::SecurityNotSupported, false, false, true),
            (E::RemoteDesktopUnavailable, false, false, false),
            (E::DisconnectedByHost, false, true, false),
            (E::SessionTakenOver, false, true, false),
            (E::IdleTimeout, false, true, false),
            (E::VaultUnavailable, false, false, false),
            (E::AppInBackground, false, true, false),
            (E::Unexpected, false, true, false),
        ];
        assert_eq!(
            matrix.len(),
            ConnectionError::ALL.len(),
            "every error is in the matrix"
        );
        for (e, retry, button, warning) in matrix {
            assert_eq!(e.auto_reconnect_allowed(), retry, "auto-retry for {e:?}");
            assert_eq!(e.user_can_reconnect(), button, "reconnect button for {e:?}");
            assert_eq!(e.is_security_warning(), warning, "warning for {e:?}");
        }
    }

    /// SA-8: the exact set of errors that may be retried automatically.
    #[test]
    fn only_network_errors_are_retried() {
        let retried: Vec<_> = ConnectionError::ALL
            .into_iter()
            .filter(|e| e.auto_reconnect_allowed())
            .collect();
        assert_eq!(
            retried,
            [
                ConnectionError::HostRefused,
                ConnectionError::Timeout,
                ConnectionError::HostNotFound,
                ConnectionError::ConnectionLost,
            ]
        );
    }

    #[test]
    fn reconnect_gives_up_after_the_limit() {
        let lost = Event::Error(ConnectionError::ConnectionLost);
        let s = run(&[
            Event::ConnectRequested,
            Event::SessionReady,
            lost,
            lost,
            lost,
        ]);
        assert_eq!(s, Ok(Status::Reconnecting { attempt: 3 }));
        let s = run(&[
            Event::ConnectRequested,
            Event::SessionReady,
            lost,
            lost,
            lost,
            lost,
        ]);
        assert_eq!(s, Ok(Status::Failed(ConnectionError::ConnectionLost)));
    }

    #[test]
    fn reconnect_delays_grow() {
        let d: Vec<u64> = (1..=4)
            .map(|a| ConnectionError::reconnect_delay(a).as_secs())
            .collect();
        assert_eq!(d, [2, 5, 10, 10]);
    }
}
