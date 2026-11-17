//! Stops password guessing. A device (IP address) with [`MAX_FAILURES`]
//! wrong passwords within [`WINDOW`] is refused for [`LOCKOUT`]; other
//! devices can still connect, so one person's typos do not lock out
//! everyone. As a safety net against guessing from many addresses at once,
//! [`MAX_FAILURES_ALL`] wrong passwords from any devices together within
//! [`WINDOW`] refuse everyone for [`LOCKOUT`].
//!
//! A failed sign-in ends the connection with an error within seconds, so a
//! connection that ends with an error before [`SHORT`] counts as a failure.
//! A real session that later drops on a bad network lasts longer and does not.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use ironrdp_server::{ConnectionHandler, PostConnectionAction};

use crate::HostEvent;

pub(crate) const MAX_FAILURES: usize = 5;
pub(crate) const MAX_FAILURES_ALL: usize = 20;
pub(crate) const WINDOW: Duration = Duration::from_secs(10 * 60);
pub(crate) const LOCKOUT: Duration = Duration::from_secs(15 * 60);
const SHORT: Duration = Duration::from_secs(20);
/// Most devices remembered (oldest forgotten first), so a flood of addresses
/// cannot use up memory.
const MAX_DEVICES: usize = 1024;

/// Wrong passwords counted for one device, or for all of them together.
#[derive(Default)]
struct Counter {
    failures: Vec<Instant>,
    locked_until: Option<Instant>,
}

impl Counter {
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

    /// Counts one failure; returns true if this starts a lockout.
    fn fail(&mut self, now: Instant, max: usize) -> bool {
        self.failures.retain(|t| now.duration_since(*t) < WINDOW);
        self.failures.push(now);
        if self.failures.len() >= max && self.locked_until.is_none() {
            self.locked_until = Some(now + LOCKOUT);
            return true;
        }
        false
    }

    fn last_seen(&self) -> Option<Instant> {
        self.failures.last().copied()
    }
}

/// Wrong passwords tried by different devices together, within the window.
/// Counts *distinct devices*, not raw attempts — its job is to catch many
/// addresses guessing together (a flood), not to duplicate the per-device
/// block above, so one device retrying (even many times, even with
/// successful sign-ins mixed in) never by itself locks out everyone.
#[derive(Default)]
struct AllCounter {
    recent: Vec<(IpAddr, Instant)>,
    locked_until: Option<Instant>,
}

impl AllCounter {
    fn locked(&mut self, now: Instant) -> bool {
        match self.locked_until {
            Some(t) if now < t => true,
            Some(_) => {
                self.locked_until = None;
                self.recent.clear();
                false
            }
            None => false,
        }
    }

    /// Records a failure from `ip`; returns true if this starts a lockout.
    fn fail(&mut self, ip: IpAddr, now: Instant, max: usize) -> bool {
        self.recent.retain(|(_, t)| now.duration_since(*t) < WINDOW);
        match self.recent.iter_mut().find(|(a, _)| *a == ip) {
            Some(entry) => entry.1 = now,
            None => self.recent.push((ip, now)),
        }
        if self.recent.len() >= max && self.locked_until.is_none() {
            self.locked_until = Some(now + LOCKOUT);
            return true;
        }
        false
    }
}

/// The address of the connection being served. The server takes one
/// connection at a time, so the last accepted address is the current one.
#[derive(Clone, Default)]
pub(crate) struct Peer(Arc<Mutex<Option<SocketAddr>>>);

impl Peer {
    fn set(&self, addr: SocketAddr) {
        if let Ok(mut p) = self.0.lock() {
            *p = Some(addr);
        }
    }

    /// The IP address alone (no port), or "unknown".
    pub(crate) fn ip(&self) -> String {
        self.0
            .lock()
            .ok()
            .and_then(|p| *p)
            .map_or_else(|| "unknown".to_owned(), show_ip)
    }
}

/// An IP address for people: IPv4-mapped IPv6 shown as plain IPv4.
fn show_ip(addr: SocketAddr) -> String {
    match addr.ip() {
        std::net::IpAddr::V6(v6) => v6
            .to_ipv4_mapped()
            .map_or_else(|| v6.to_string(), |v4| v4.to_string()),
        ip => ip.to_string(),
    }
}

/// True for addresses on a Tailscale network and for this computer itself.
/// The IPv6 range (`fd7a:115c:a1e0::/48`) is Tailscale's own prefix, so it is
/// a real check. The IPv4 range (`100.64.0.0/10`) is not: it is the whole
/// IANA carrier-grade-NAT block, which some ISPs also hand out to ordinary
/// mobile and broadband customers, so an address in it does not prove the
/// peer is actually on this Tailscale network — only that it looks like it
/// could be. The listener binds to all interfaces, so this is the only
/// check standing between "Tailscale only" and a non-Tailscale peer whose
/// address happens to fall in the same IPv4 block.
pub fn is_tailnet_or_local(ip: std::net::IpAddr) -> bool {
    use std::net::IpAddr;
    let ip = match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4),
        v4 => v4,
    };
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_loopback() || (o[0] == 100 && (o[1] & 0xC0) == 64)
        }
        IpAddr::V6(v6) => {
            let s = v6.segments();
            v6.is_loopback() || (s[0] == 0xfd7a && s[1] == 0x115c && s[2] == 0xa1e0)
        }
    }
}

/// Whether a connection that arrived on network interface `local` should be
/// accepted when "Tailscale only" is (or isn't) turned on.
///
/// `is_tailnet_or_local` on the *peer*'s address only checks whether the peer
/// *claims* an address in Tailscale's own IPv4 block — which a device on
/// ordinary carrier-grade NAT can also have, with nothing to do with
/// Tailscale. This checks the *local* side instead: which of this computer's
/// own network interfaces actually received the connection. A connection
/// that arrived on a LAN or public interface is refused in Tailscale-only
/// mode even if the peer's address happens to look like a Tailscale one; one
/// that arrived on this computer's own Tailscale interface (or on loopback)
/// is accepted. When Tailscale-only mode is off, every interface is allowed,
/// same as before.
pub fn local_address_allowed(local: std::net::IpAddr, tailnet_only: bool) -> bool {
    !tailnet_only || is_tailnet_or_local(local)
}

pub(crate) struct Limiter {
    devices: HashMap<IpAddr, Counter>,
    all: AllCounter,
    events: mpsc::Sender<HostEvent>,
    peer: Peer,
    tailnet_only: bool,
    /// Set by the person at this computer (see [`crate::Unblocker`]): forget
    /// every wrong-password block before the next connection is checked.
    unblock: Arc<AtomicBool>,
}

/// What a wrong password led to.
#[derive(Debug, PartialEq, Eq)]
enum Lock {
    None,
    /// That device is refused for a while.
    Device,
    /// Everyone is refused for a while.
    All,
}

impl Limiter {
    pub(crate) fn new(
        events: mpsc::Sender<HostEvent>,
        peer: Peer,
        tailnet_only: bool,
        unblock: Arc<AtomicBool>,
    ) -> Self {
        Self {
            devices: HashMap::new(),
            all: AllCounter::default(),
            events,
            peer,
            tailnet_only,
            unblock,
        }
    }

    fn locked(&mut self, ip: IpAddr, now: Instant) -> bool {
        if self.unblock.swap(false, Ordering::SeqCst) {
            self.devices.clear();
            self.all = AllCounter::default();
            tracing::info!("wrong-password blocks cleared by the person at this computer");
        }
        self.all.locked(now) || self.devices.get_mut(&ip).is_some_and(|c| c.locked(now))
    }

    /// Records the end of a connection from `ip`.
    fn record(&mut self, ip: IpAddr, now: Instant, duration: Duration, failed: bool) -> Lock {
        if !failed {
            if duration >= SHORT {
                // A real session: this device's earlier typos are forgiven.
                self.devices.remove(&ip);
            }
            return Lock::None;
        }
        if duration >= SHORT {
            return Lock::None;
        }
        if self.devices.len() >= MAX_DEVICES && !self.devices.contains_key(&ip) {
            let oldest = self
                .devices
                .iter()
                .filter(|(_, c)| c.locked_until.is_none())
                .min_by_key(|(_, c)| c.last_seen())
                .map(|(k, _)| *k);
            if let Some(k) = oldest {
                self.devices.remove(&k);
            }
        }
        let device = self.devices.entry(ip).or_default().fail(now, MAX_FAILURES);
        let all = self.all.fail(ip, now, MAX_FAILURES_ALL);
        if all {
            Lock::All
        } else if device {
            Lock::Device
        } else {
            Lock::None
        }
    }
}

impl ConnectionHandler for Limiter {
    fn on_accept(&mut self, peer: SocketAddr, local: SocketAddr) -> bool {
        if self.tailnet_only && !is_tailnet_or_local(peer.ip()) {
            tracing::info!(address = %show_ip(peer), "refused: not from the Tailscale network");
            return false;
        }
        if !local_address_allowed(local.ip(), self.tailnet_only) {
            tracing::info!(
                address = %show_ip(peer),
                local = %show_ip(local),
                "refused: did not arrive on the Tailscale interface"
            );
            return false;
        }
        let allowed = !self.locked(peer.ip(), Instant::now());
        if !allowed {
            tracing::info!(address = %show_ip(peer), "refused: too many wrong passwords");
        }
        if allowed {
            self.peer.set(peer);
        }
        allowed
    }

    fn on_disconnected(
        &mut self,
        peer: SocketAddr,
        duration: Duration,
        error: Option<&anyhow::Error>,
    ) -> PostConnectionAction {
        let _ = self.events.send(HostEvent::ViewerDisconnected);
        // Only credential failures count toward lockout. TLS noise from relay
        // networks (accept_begin failed, UnknownCA) and post-auth network drops
        // must not trigger the 15-minute lockout: they are not password guesses.
        let failed = is_credential_failure(error);
        if failed {
            let _ = self.events.send(HostEvent::SignInFailed {
                address: show_ip(peer),
            });
        }
        let who = match self.record(peer.ip(), Instant::now(), duration, failed) {
            Lock::None => None,
            Lock::Device => Some(show_ip(peer)),
            Lock::All => Some("all devices".to_owned()),
        };
        if let Some(address) = who {
            let _ = self.events.send(HostEvent::LockedOut {
                minutes: LOCKOUT.as_secs() / 60,
                address,
            });
        }
        PostConnectionAction::Continue
    }
}

/// True only when the error chain contains evidence of a failed sign-in
/// (wrong password / wrong user). Pure TLS errors, early connection drops, and
/// network noise return false and are not counted against the lockout budget.
fn is_credential_failure(error: Option<&anyhow::Error>) -> bool {
    let Some(err) = error else { return false };
    // The CredSSP server reports "LogonDenied" when NLA credentials are wrong.
    let chain = format!("{err:#}");
    chain.contains("LogonDenied")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_tailscale_and_this_computer_when_asked() {
        let ok = [
            "100.64.0.1",
            "100.112.147.20",
            "100.127.255.254",
            "127.0.0.1",
            "::1",
            "fd7a:115c:a1e0::1",
            "::ffff:100.100.1.1",
        ];
        let no = [
            "100.63.255.255",
            "100.128.0.1",
            "192.168.1.5",
            "10.0.0.2",
            "8.8.8.8",
            "fd7a:115c:a1e1::1",
            "::ffff:192.168.1.5",
        ];
        for a in ok {
            assert!(
                is_tailnet_or_local(a.parse().unwrap_or_else(|_| panic!("{a}"))),
                "{a}"
            );
        }
        for a in no {
            assert!(
                !is_tailnet_or_local(a.parse().unwrap_or_else(|_| panic!("{a}"))),
                "{a}"
            );
        }
        let (tx, _rx) = mpsc::channel();
        let mut l = Limiter::new(tx, Peer::default(), true, Arc::default());
        let any_local: SocketAddr = "100.64.0.9:3389".parse().unwrap_or_else(|_| panic!());
        assert!(!l.on_accept(
            "192.168.1.5:5000".parse().unwrap_or_else(|_| panic!()),
            any_local
        ));
        assert!(l.on_accept(
            "100.112.147.20:5000".parse().unwrap_or_else(|_| panic!()),
            any_local
        ));
    }

    #[test]
    fn local_address_allowed_checks_which_interface_the_connection_arrived_on() {
        // Tailscale-only: a Tailscale or loopback local address is fine...
        assert!(local_address_allowed(ip("100.64.0.1"), true));
        assert!(local_address_allowed(ip("fd7a:115c:a1e0::1"), true));
        assert!(local_address_allowed(ip("127.0.0.1"), true));
        // ...but a LAN or public local address is refused, even though it
        // would pass a check on the *peer*'s address alone.
        assert!(!local_address_allowed(ip("192.168.1.5"), true));
        assert!(!local_address_allowed(ip("10.0.0.2"), true));
        assert!(!local_address_allowed(ip("8.8.8.8"), true));
        // Carrier-grade NAT overlaps Tailscale's own IPv4 block, so a local
        // address in it is treated the same as a real Tailscale one here —
        // this check is about which interface received the connection, not
        // about telling Tailscale apart from CGNAT on the IP alone.
        assert!(local_address_allowed(ip("100.112.147.20"), true));
        // "Any network": every interface is allowed, same as before.
        assert!(local_address_allowed(ip("192.168.1.5"), false));
        assert!(local_address_allowed(ip("8.8.8.8"), false));
    }

    #[test]
    fn on_accept_refuses_a_connection_that_did_not_arrive_on_the_tailscale_interface() {
        let (tx, _rx) = mpsc::channel();
        let mut l = Limiter::new(tx, Peer::default(), true, Arc::default());
        // The peer's address alone looks like Tailscale/CGNAT, but the
        // connection actually came in on this computer's plain LAN address —
        // refused.
        assert!(!l.on_accept(
            "100.112.147.20:5000".parse().unwrap_or_else(|_| panic!()),
            "192.168.1.10:3389".parse().unwrap_or_else(|_| panic!())
        ));
        // Same peer address, but arriving on the Tailscale interface this
        // time — accepted.
        assert!(l.on_accept(
            "100.112.147.20:5001".parse().unwrap_or_else(|_| panic!()),
            "100.64.0.5:3389".parse().unwrap_or_else(|_| panic!())
        ));
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap_or_else(|_| panic!("{s}"))
    }

    #[test]
    fn five_quick_failures_lock_that_device_only_and_the_lock_expires() {
        let (tx, _rx) = mpsc::channel();
        let mut l = Limiter::new(tx, Peer::default(), false, Arc::default());
        let (a, b) = (ip("100.64.0.1"), ip("100.64.0.2"));
        let t0 = Instant::now();
        let s1 = Duration::from_secs(1);
        for i in 0..4 {
            assert_eq!(
                l.record(a, t0 + Duration::from_secs(i), s1, true),
                Lock::None
            );
            assert!(!l.locked(a, t0 + Duration::from_secs(i)));
        }
        assert_eq!(
            l.record(a, t0 + Duration::from_secs(5), s1, true),
            Lock::Device
        );
        assert!(l.locked(a, t0 + Duration::from_secs(6)));
        // Another device is not blocked by the first one's mistakes.
        assert!(!l.locked(b, t0 + Duration::from_secs(6)));
        assert!(!l.locked(a, t0 + Duration::from_secs(5) + LOCKOUT + s1));
    }

    #[test]
    fn unblock_clears_device_and_everyone_blocks_once() {
        let (tx, _rx) = mpsc::channel();
        let unblock = Arc::new(AtomicBool::new(false));
        let mut l = Limiter::new(tx, Peer::default(), false, Arc::clone(&unblock));
        let t0 = Instant::now();
        let s1 = Duration::from_secs(1);
        let a = ip("100.64.0.1");
        for _ in 0..MAX_FAILURES {
            l.record(a, t0, s1, true);
        }
        for i in 0..MAX_FAILURES_ALL {
            l.record(ip(&format!("100.64.1.{i}")), t0, s1, true);
        }
        assert!(l.locked(a, t0 + s1));
        assert!(l.locked(ip("100.64.9.9"), t0 + s1));

        crate::Unblocker(Arc::clone(&unblock)).unblock();
        assert!(!l.locked(a, t0 + s1));
        assert!(!l.locked(ip("100.64.9.9"), t0 + s1));
        assert!(!unblock.load(Ordering::SeqCst), "the request is used up");

        // Counting starts again from zero: it takes a full set of new wrong
        // passwords to block that device again.
        for _ in 0..MAX_FAILURES - 1 {
            assert_eq!(l.record(a, t0 + s1, s1, true), Lock::None);
        }
        assert_eq!(l.record(a, t0 + s1, s1, true), Lock::Device);
    }

    #[test]
    fn many_devices_guessing_together_lock_everyone() {
        let (tx, _rx) = mpsc::channel();
        let mut l = Limiter::new(tx, Peer::default(), false, Arc::default());
        let t0 = Instant::now();
        let s1 = Duration::from_secs(1);
        let mut last = Lock::None;
        for i in 0..MAX_FAILURES_ALL {
            let device = ip(&format!("100.64.1.{i}"));
            last = l.record(device, t0, s1, true);
        }
        assert_eq!(last, Lock::All);
        assert!(l.locked(ip("100.64.9.9"), t0 + s1));
    }

    #[test]
    fn old_failures_and_long_sessions_do_not_count() {
        let (tx, _rx) = mpsc::channel();
        let mut l = Limiter::new(tx, Peer::default(), false, Arc::default());
        let a = ip("100.64.0.1");
        let t0 = Instant::now();
        let s1 = Duration::from_secs(1);
        for i in 0..4 {
            l.record(a, t0 + Duration::from_secs(i), s1, true);
        }
        // Eleven minutes later the old failures have expired.
        assert_eq!(
            l.record(a, t0 + Duration::from_secs(11 * 60), s1, true),
            Lock::None
        );
        // A long session that ends with a network error is not a failed sign-in.
        assert_eq!(
            l.record(
                a,
                t0 + Duration::from_secs(700),
                Duration::from_secs(600),
                true
            ),
            Lock::None
        );
        // A successful session clears that device's count.
        l.record(
            a,
            t0 + Duration::from_secs(800),
            Duration::from_secs(60),
            false,
        );
        assert!(!l.devices.contains_key(&a));
    }

    /// Thousands of different devices hammering the limiter over a short
    /// time: memory must stay bounded (eviction kicks in) and nothing
    /// should panic or loop forever. This is the kind of flood a scanner
    /// or a botnet would produce, not a real user.
    #[test]
    fn a_flood_of_devices_does_not_grow_without_bound_or_panic() {
        let (tx, _rx) = mpsc::channel();
        let mut l = Limiter::new(tx, Peer::default(), false, Arc::default());
        let t0 = Instant::now();
        let s1 = Duration::from_secs(1);
        for i in 0..20_000u32 {
            let device = ip(&format!(
                "{}.{}.{}.{}",
                (i >> 24) as u8 | 1,
                (i >> 16) as u8,
                (i >> 8) as u8,
                i as u8
            ));
            // Spread the clock out so the global 20-in-10-min safety net
            // doesn't dominate the test; each device gets one failure.
            let now = t0 + Duration::from_millis(u64::from(i) * 50);
            l.record(device, now, s1, true);
            assert!(l.devices.len() <= MAX_DEVICES, "device map grew unbounded");
        }
    }

    /// One device that alternates between a quick wrong password and a
    /// real (successful, longer) session many times in a row: the lockout
    /// state must not leak or get stuck across that back-and-forth.
    #[test]
    fn repeated_reconnect_cycles_do_not_leave_a_stuck_lock() {
        let (tx, _rx) = mpsc::channel();
        let mut l = Limiter::new(tx, Peer::default(), false, Arc::default());
        let a = ip("100.64.5.5");
        let mut t = Instant::now();
        let s1 = Duration::from_secs(1);
        for _ in 0..500 {
            // One wrong password, then immediately a real, successful
            // session: the device should never end up locked out just
            // from this pattern.
            l.record(a, t, s1, true);
            t += Duration::from_secs(1);
            l.record(a, t, Duration::from_secs(30), false);
            t += Duration::from_secs(1);
            assert!(!l.locked(a, t), "device got stuck locked after cycle");
        }
    }
}
