//! Validation of what the user types into the connection form.
//!
//! Error messages never repeat the user's input, so they can be shown or
//! logged without exposing hostnames, usernames or passwords.

use core::fmt;
use core::net::{IpAddr, Ipv6Addr};

/// Default RDP port.
pub const DEFAULT_PORT: u16 = 3389;
/// Longest profile name accepted.
pub const MAX_PROFILE_NAME_CHARS: usize = 64;
/// Longest username accepted (Windows allows 104 for UPN; we allow some slack).
pub const MAX_USERNAME_CHARS: usize = 256;
/// Longest password accepted (Windows limit is 256 characters).
pub const MAX_PASSWORD_CHARS: usize = 256;

/// Which form field an error belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Field {
    ProfileName,
    Host,
    Port,
    Username,
    Password,
}

/// Why a field is invalid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldError {
    Required,
    TooLong,
    InvalidHost,
    HostHasScheme,
    HostHasPath,
    InvalidPort,
    InvalidUsername,
    ControlCharacters,
}

impl FieldError {
    /// A short message suitable for showing under the field.
    pub const fn message(self) -> &'static str {
        match self {
            Self::Required => "This field is required.",
            Self::TooLong => "This is too long.",
            Self::InvalidHost => "Enter a computer name, domain name or IP address.",
            Self::HostHasScheme => {
                "Enter just the computer name or address, without rdp:// or http://."
            }
            Self::HostHasPath => "Enter just the computer name or address, without a path.",
            Self::InvalidPort => "Port must be a number from 1 to 65535.",
            Self::InvalidUsername => "Enter a username, DOMAIN\\username or username@domain.",
            Self::ControlCharacters => "This contains characters that aren't allowed.",
        }
    }
}

impl fmt::Display for FieldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

/// The kind of address the user entered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostKind {
    Ipv4,
    Ipv6,
    Name,
}

/// A host and port parsed from the form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// Host in normalised form: lower-case name, or an IP address without brackets.
    pub host: String,
    pub port: u16,
    pub kind: HostKind,
}

impl Endpoint {
    /// "host:port", with IPv6 addresses in brackets.
    pub fn display(&self) -> String {
        match self.kind {
            HostKind::Ipv6 => format!("[{}]:{}", self.host, self.port),
            _ => format!("{}:{}", self.host, self.port),
        }
    }
}

fn has_control_chars(s: &str) -> bool {
    s.chars().any(char::is_control)
}

/// Validates an optional profile name (empty means "use the host as the name").
pub fn validate_profile_name(name: &str) -> Result<(), FieldError> {
    if has_control_chars(name) {
        return Err(FieldError::ControlCharacters);
    }
    if name.trim().chars().count() > MAX_PROFILE_NAME_CHARS {
        return Err(FieldError::TooLong);
    }
    Ok(())
}

fn is_valid_dns_label(label: &str) -> bool {
    let bytes = label.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'-')
        && bytes.first() != Some(&b'-')
        && bytes.last() != Some(&b'-')
}

/// Validates a host name (RFC 1123), IPv4 or IPv6 address.
///
/// Accepts `[v6]` or `[v6]:port` and `name:port` / `v4:port`; a port in the
/// host field overrides the port field. Returns the normalised endpoint.
pub fn parse_endpoint(host_input: &str, port_input: &str) -> Result<Endpoint, (Field, FieldError)> {
    let raw = host_input.trim();
    if raw.is_empty() {
        return Err((Field::Host, FieldError::Required));
    }
    if has_control_chars(raw) || raw.contains(char::is_whitespace) {
        return Err((Field::Host, FieldError::InvalidHost));
    }
    if raw.contains("://") {
        return Err((Field::Host, FieldError::HostHasScheme));
    }
    if raw.contains('/') || raw.contains('\\') || raw.contains('?') || raw.contains('#') {
        return Err((Field::Host, FieldError::HostHasPath));
    }
    if raw.contains('@') {
        return Err((Field::Host, FieldError::InvalidHost));
    }

    // Split an optional port out of the host field.
    let (host_part, port_from_host): (&str, Option<&str>) =
        if let Some(rest) = raw.strip_prefix('[') {
            let Some((inside, after)) = rest.split_once(']') else {
                return Err((Field::Host, FieldError::InvalidHost));
            };
            match after {
                "" => (inside, None),
                p if p.starts_with(':') => (inside, Some(&p[1..])),
                _ => return Err((Field::Host, FieldError::InvalidHost)),
            }
        } else if raw.matches(':').count() == 1 {
            let (h, p) = raw.split_once(':').unwrap_or((raw, ""));
            (h, Some(p))
        } else {
            (raw, None)
        };

    let port = match port_from_host {
        Some(p) => parse_port(p).map_err(|e| (Field::Host, e))?,
        None => {
            if port_input.trim().is_empty() {
                DEFAULT_PORT
            } else {
                parse_port(port_input).map_err(|e| (Field::Port, e))?
            }
        }
    };

    if let Ok(ip) = host_part.parse::<IpAddr>() {
        return Ok(match ip {
            IpAddr::V4(v4) => Endpoint {
                host: v4.to_string(),
                port,
                kind: HostKind::Ipv4,
            },
            IpAddr::V6(v6) => Endpoint {
                host: v6.to_string(),
                port,
                kind: HostKind::Ipv6,
            },
        });
    }
    // Bare IPv6 without brackets (several colons) is accepted without a port.
    if raw.matches(':').count() > 1 && !raw.starts_with('[') {
        return match raw.parse::<Ipv6Addr>() {
            Ok(v6) => Ok(Endpoint {
                host: v6.to_string(),
                port,
                kind: HostKind::Ipv6,
            }),
            Err(_) => Err((Field::Host, FieldError::InvalidHost)),
        };
    }

    let name = host_part.trim_end_matches('.').to_ascii_lowercase();
    if name.is_empty() || name.len() > 253 {
        return Err((Field::Host, FieldError::InvalidHost));
    }
    // A name made only of digits and dots must be a valid IPv4 address, which failed above.
    if name.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return Err((Field::Host, FieldError::InvalidHost));
    }
    if !name.split('.').all(is_valid_dns_label) {
        return Err((Field::Host, FieldError::InvalidHost));
    }
    Ok(Endpoint {
        host: name,
        port,
        kind: HostKind::Name,
    })
}

/// Parses a port number from 1 to 65535.
pub fn parse_port(input: &str) -> Result<u16, FieldError> {
    let t = input.trim();
    if t.is_empty() || !t.chars().all(|c| c.is_ascii_digit()) || t.len() > 5 {
        return Err(FieldError::InvalidPort);
    }
    match t.parse::<u32>() {
        Ok(n) if (1..=65535).contains(&n) => u16::try_from(n).map_err(|_| FieldError::InvalidPort),
        _ => Err(FieldError::InvalidPort),
    }
}

/// A username split into its parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Username {
    pub user: String,
    pub domain: Option<String>,
    /// True when typed as user@domain (UPN) rather than DOMAIN\\user.
    pub upn: bool,
}

const USERNAME_FORBIDDEN: &[char] = &[
    '"', '/', '[', ']', ':', ';', '|', '=', ',', '+', '*', '?', '<', '>',
];

/// Validates "user", "DOMAIN\user" or "user@domain".
pub fn parse_username(input: &str) -> Result<Username, FieldError> {
    let t = input.trim();
    if t.is_empty() {
        return Err(FieldError::Required);
    }
    if has_control_chars(t) {
        return Err(FieldError::ControlCharacters);
    }
    if t.chars().count() > MAX_USERNAME_CHARS {
        return Err(FieldError::TooLong);
    }
    let (user, domain, upn) = if let Some((d, u)) = t.split_once('\\') {
        (u, Some(d), false)
    } else if let Some((u, d)) = t.rsplit_once('@') {
        (u, Some(d), true)
    } else {
        (t, None, false)
    };
    let user_ok = !user.is_empty()
        && !user.contains('\\')
        && !user.contains('@')
        && !user.contains(USERNAME_FORBIDDEN);
    let domain_ok = domain.is_none_or(|d| {
        !d.is_empty() && !d.contains(['\\', '@']) && !d.contains(USERNAME_FORBIDDEN)
    });
    if !user_ok || !domain_ok {
        return Err(FieldError::InvalidUsername);
    }
    Ok(Username {
        user: user.to_owned(),
        domain: domain.map(str::to_owned),
        upn,
    })
}

/// Validates the password field. An empty password is allowed only when a
/// saved password will be used.
pub fn validate_password(password: &str, has_saved_password: bool) -> Result<(), FieldError> {
    if password.is_empty() {
        return if has_saved_password {
            Ok(())
        } else {
            Err(FieldError::Required)
        };
    }
    if password.chars().count() > MAX_PASSWORD_CHARS {
        return Err(FieldError::TooLong);
    }
    if password.contains(['\0', '\r', '\n']) {
        return Err(FieldError::ControlCharacters);
    }
    Ok(())
}

/// Result of validating the whole connection form.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FormCheck {
    pub errors: Vec<(Field, FieldError)>,
    pub endpoint: Option<Endpoint>,
}

impl FormCheck {
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Validates every field of the connection form and collects all errors.
pub fn check_form(
    profile_name: &str,
    host: &str,
    port: &str,
    username: &str,
    password: &str,
    has_saved_password: bool,
) -> FormCheck {
    let mut check = FormCheck::default();
    if let Err(e) = validate_profile_name(profile_name) {
        check.errors.push((Field::ProfileName, e));
    }
    match parse_endpoint(host, port) {
        Ok(ep) => check.endpoint = Some(ep),
        Err(e) => {
            check.errors.push(e);
            // Also report a bad port field now, so the user can fix both at once.
            if e.0 == Field::Host && !port.trim().is_empty() {
                if let Err(pe) = parse_port(port) {
                    check.errors.push((Field::Port, pe));
                }
            }
        }
    }
    if let Err(e) = parse_username(username) {
        check.errors.push((Field::Username, e));
    }
    if let Err(e) = validate_password(password, has_saved_password) {
        check.errors.push((Field::Password, e));
    }
    check
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(host: &str, port: &str) -> Endpoint {
        match parse_endpoint(host, port) {
            Ok(e) => e,
            Err(e) => panic!("{host:?} {port:?} should be valid, got {e:?}"),
        }
    }

    fn bad(host: &str, port: &str) -> (Field, FieldError) {
        match parse_endpoint(host, port) {
            Ok(e) => panic!("{host:?} {port:?} should be invalid, got {e:?}"),
            Err(e) => e,
        }
    }

    #[test]
    fn accepts_names_and_addresses() {
        assert_eq!(ok("Office-PC", "").host, "office-pc");
        assert_eq!(ok("office-pc", "").port, DEFAULT_PORT);
        assert_eq!(
            ok("pc1.example.com.", "3390").display(),
            "pc1.example.com:3390"
        );
        assert_eq!(ok("192.0.2.10", "").kind, HostKind::Ipv4);
        assert_eq!(ok("192.0.2.10:3391", "").port, 3391);
        assert_eq!(ok("2001:db8::1", "").kind, HostKind::Ipv6);
        assert_eq!(ok("[2001:db8::1]:3392", "").display(), "[2001:db8::1]:3392");
        assert_eq!(ok("  example.com  ", " 3389 ").host, "example.com");
    }

    #[test]
    fn port_in_host_field_overrides_port_field() {
        assert_eq!(ok("example.com:4000", "3389").port, 4000);
    }

    #[test]
    fn rejects_bad_hosts() {
        assert_eq!(bad("", ""), (Field::Host, FieldError::Required));
        assert_eq!(bad("rdp://example.com", "").1, FieldError::HostHasScheme);
        assert_eq!(bad("example.com/path", "").1, FieldError::HostHasPath);
        assert_eq!(bad("user@example.com", "").1, FieldError::InvalidHost);
        assert_eq!(bad("exa mple.com", "").1, FieldError::InvalidHost);
        assert_eq!(bad("-bad.example.com", "").1, FieldError::InvalidHost);
        assert_eq!(bad("bad_.example.com", "").1, FieldError::InvalidHost);
        assert_eq!(bad("999.1.1.1", "").1, FieldError::InvalidHost);
        assert_eq!(bad("[2001:db8::1", "").1, FieldError::InvalidHost);
        assert_eq!(bad("2001:db8::zz", "").1, FieldError::InvalidHost);
        assert_eq!(bad(&"a".repeat(64), "").1, FieldError::InvalidHost);
        assert_eq!(bad("line\nbreak", "").1, FieldError::InvalidHost);
    }

    #[test]
    fn rejects_bad_ports() {
        assert_eq!(
            bad("example.com", "0"),
            (Field::Port, FieldError::InvalidPort)
        );
        assert_eq!(
            bad("example.com", "65536"),
            (Field::Port, FieldError::InvalidPort)
        );
        assert_eq!(
            bad("example.com", "-1"),
            (Field::Port, FieldError::InvalidPort)
        );
        assert_eq!(
            bad("example.com", "33 89"),
            (Field::Port, FieldError::InvalidPort)
        );
        assert_eq!(
            bad("example.com:abc", ""),
            (Field::Host, FieldError::InvalidPort)
        );
        assert_eq!(parse_port("65535"), Ok(65535));
        assert_eq!(parse_port("1"), Ok(1));
    }

    #[test]
    fn usernames() {
        assert_eq!(parse_username("alice").map(|u| u.domain), Ok(None));
        let d = parse_username("CORP\\alice");
        assert_eq!(d.as_ref().map(|u| u.user.as_str()), Ok("alice"));
        assert_eq!(d.map(|u| u.domain), Ok(Some("CORP".to_owned())));
        let upn = parse_username("alice@example.com");
        assert_eq!(upn.map(|u| u.domain), Ok(Some("example.com".to_owned())));
        assert_eq!(parse_username(""), Err(FieldError::Required));
        assert_eq!(parse_username("CORP\\"), Err(FieldError::InvalidUsername));
        assert_eq!(
            parse_username("@example.com"),
            Err(FieldError::InvalidUsername)
        );
        assert_eq!(parse_username("a\\b\\c"), Err(FieldError::InvalidUsername));
        assert_eq!(parse_username("bad|name"), Err(FieldError::InvalidUsername));
        assert_eq!(parse_username(&"u".repeat(300)), Err(FieldError::TooLong));
    }

    #[test]
    fn passwords() {
        let sample = ["p", "w", "-", "1", "2", "3"].concat();
        assert_eq!(validate_password(&sample, false), Ok(()));
        assert_eq!(validate_password("", false), Err(FieldError::Required));
        assert_eq!(validate_password("", true), Ok(()));
        assert_eq!(
            validate_password(&"x".repeat(257), false),
            Err(FieldError::TooLong)
        );
        assert_eq!(
            validate_password("a\nb", false),
            Err(FieldError::ControlCharacters)
        );
    }

    #[test]
    fn whole_form_collects_all_errors() {
        let c = check_form("", "", "70000", "", "", false);
        let fields: Vec<Field> = c.errors.iter().map(|(f, _)| *f).collect();
        assert_eq!(
            fields,
            vec![Field::Host, Field::Port, Field::Username, Field::Password]
        );
        assert!(!c.is_valid());

        let sample = ["p", "w", "-", "1", "2", "3"].concat();
        let good = check_form("Office", "192.0.2.10", "", "alice", &sample, false);
        assert!(good.is_valid(), "{:?}", good.errors);
        assert_eq!(good.endpoint.map(|e| e.port), Some(DEFAULT_PORT));
    }

    #[test]
    fn messages_never_contain_input() {
        let marker = ["Office", "PC", "Unique"].join("-");
        let c = check_form(
            &"n".repeat(100),
            &format!("rdp://{marker}"),
            "x",
            "a|b",
            "",
            false,
        );
        for (_, e) in &c.errors {
            assert!(!e.message().contains(&marker));
            assert!(!e.message().is_empty());
        }
    }
}
