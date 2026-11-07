//! Stops password guessing: after [`MAX_FAILURES`] failed connections within
//! [`WINDOW`], new connections are refused for [`LOCKOUT`].
//!
//! A failed sign-in ends the connection with an error within seconds, so a
//! connection that ends with an error before [`SHORT`] counts as a failure.
//! A real session that later drops on a bad network lasts longer and does not.

use std::net::SocketAddr;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use ironrdp_server::{ConnectionHandler, PostConnectionAction};

use crate::HostEvent;

pub(crate) const MAX_FAILURES: usize = 5;
pub(crate) const WINDOW: Duration = Duration::from_secs(10 * 60);
pub(crate) const LOCKOUT: Duration = Duration::from_secs(15 * 60);
const SHORT: Duration = Duration::from_secs(20);

pub(crate) struct Limiter {
    failures: Vec<Instant>,
    locked_until: Option<Instant>,
    events: mpsc::Sender<HostEvent>,
}

impl Limiter {
    pub(crate) fn new(events: mpsc::Sender<HostEvent>) -> Self {
        Self {
            failures: Vec::new(),
            locked_until: None,
            events,
        }
    }

    fn locked(&mut self, now: Instant) -> bool {
        match self.locked_until {
            Some(t) if now < t => true,
            Some(_) => {
                self.locked_until = None;
                self.failures.clear();
                false
            }
            None => false,
        }
    }

    /// Records the end of a connection; returns true if this starts a lockout.
    fn record(&mut self, now: Instant, duration: Duration, failed: bool) -> bool {
        if !failed {
            if duration >= SHORT {
                self.failures.clear();
            }
            return false;
        }
        if duration >= SHORT {
            return false;
        }
        self.failures.retain(|t| now.duration_since(*t) < WINDOW);
        self.failures.push(now);
        if self.failures.len() >= MAX_FAILURES {
            self.locked_until = Some(now + LOCKOUT);
            return true;
        }
        false
    }
}

impl ConnectionHandler for Limiter {
    fn on_accept(&mut self, _peer: SocketAddr) -> bool {
        !self.locked(Instant::now())
    }

    fn on_disconnected(
        &mut self,
        _peer: SocketAddr,
        duration: Duration,
        error: Option<&anyhow::Error>,
    ) -> PostConnectionAction {
        let _ = self.events.send(HostEvent::ViewerDisconnected);
        if self.record(Instant::now(), duration, error.is_some()) {
            let _ = self.events.send(HostEvent::LockedOut {
                minutes: LOCKOUT.as_secs() / 60,
            });
        }
        PostConnectionAction::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_quick_failures_lock_and_the_lock_expires() {
        let (tx, _rx) = mpsc::channel();
        let mut l = Limiter::new(tx);
        let t0 = Instant::now();
        for i in 0..4 {
            assert!(!l.record(t0 + Duration::from_secs(i), Duration::from_secs(1), true));
            assert!(!l.locked(t0 + Duration::from_secs(i)));
        }
        assert!(l.record(t0 + Duration::from_secs(5), Duration::from_secs(1), true));
        assert!(l.locked(t0 + Duration::from_secs(6)));
        assert!(!l.locked(t0 + Duration::from_secs(5) + LOCKOUT + Duration::from_secs(1)));
    }

    #[test]
    fn old_failures_and_long_sessions_do_not_count() {
        let (tx, _rx) = mpsc::channel();
        let mut l = Limiter::new(tx);
        let t0 = Instant::now();
        for i in 0..4 {
            l.record(t0 + Duration::from_secs(i), Duration::from_secs(1), true);
        }
        // Eleven minutes later the old failures have expired.
        assert!(!l.record(
            t0 + Duration::from_secs(11 * 60),
            Duration::from_secs(1),
            true
        ));
        // A long session that ends with a network error is not a failed sign-in.
        assert!(!l.record(
            t0 + Duration::from_secs(700),
            Duration::from_secs(600),
            true
        ));
        // A successful session clears the count.
        l.record(
            t0 + Duration::from_secs(800),
            Duration::from_secs(60),
            false,
        );
        assert!(l.failures.is_empty());
    }
}
