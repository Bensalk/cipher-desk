//! Connection-form validation for the UI (Stage 3).

use rdpv_core::input::{self, Field, FieldError};

use crate::api::store::{method_to_core, ConnectionMethod};

/// One problem with one field, ready to show under that field.
pub struct FieldProblem {
    /// "profileName", "host", "port", "username" or "password".
    pub field: String,
    pub message: String,
}

/// Result of checking the connection form.
pub struct FormCheckResult {
    pub valid: bool,
    pub problems: Vec<FieldProblem>,
    /// Normalised "host:port" when the address is valid, e.g. "office-pc:3389".
    pub endpoint: Option<String>,
    /// Normalised host without port, when valid.
    pub host: Option<String>,
    pub port: Option<u16>,
}

fn field_id(field: Field) -> &'static str {
    match field {
        Field::ProfileName => "profileName",
        Field::Host => "host",
        Field::Port => "port",
        Field::Username => "username",
        Field::Password => "password",
    }
}

/// Checks every field. The password is only checked for length and
/// forbidden characters; it is not stored or logged.
///
/// An empty port means the method's default port. VNC has no username, so an
/// empty username is accepted for it.
#[flutter_rust_bridge::frb(sync)]
pub fn check_connection_form(
    method: ConnectionMethod,
    profile_name: String,
    host: String,
    port: String,
    username: String,
    password: String,
    has_saved_password: bool,
) -> FormCheckResult {
    let method = method_to_core(method);
    let password = rdpv_core::secret::SecretString::new(password);
    let port = if port.trim().is_empty() {
        method.default_port().to_string()
    } else {
        port
    };
    let mut check = input::check_form(
        &profile_name,
        &host,
        &port,
        &username,
        password.expose(),
        has_saved_password,
    );
    if !method.needs_username() && username.trim().is_empty() {
        check
            .errors
            .retain(|(f, e)| !(*f == Field::Username && *e == FieldError::Required));
    }
    FormCheckResult {
        valid: check.is_valid(),
        problems: check
            .errors
            .iter()
            .map(|(f, e)| FieldProblem {
                field: field_id(*f).to_owned(),
                message: e.message().to_owned(),
            })
            .collect(),
        endpoint: check.endpoint.as_ref().map(input::Endpoint::display),
        host: check.endpoint.as_ref().map(|e| e.host.clone()),
        port: check.endpoint.as_ref().map(|e| e.port),
    }
}

/// The default RDP port, for the port field hint.
#[flutter_rust_bridge::frb(sync)]
pub fn default_port() -> u16 {
    input::DEFAULT_PORT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_form() {
        let pw = ["x", "y", "z", "9"].concat();
        let r = check_connection_form(
            ConnectionMethod::PrivateRemoteDesktop,
            "Office".into(),
            "192.0.2.10".into(),
            String::new(),
            "alice".into(),
            pw,
            false,
        );
        assert!(r.valid);
        assert_eq!(r.endpoint.as_deref(), Some("192.0.2.10:3389"));
        assert_eq!(r.port, Some(3389));
    }

    #[test]
    fn problems_name_their_fields() {
        let r = check_connection_form(
            ConnectionMethod::PrivateRemoteDesktop,
            String::new(),
            String::new(),
            "0".into(),
            String::new(),
            String::new(),
            false,
        );
        assert!(!r.valid);
        let fields: Vec<&str> = r.problems.iter().map(|p| p.field.as_str()).collect();
        assert_eq!(fields, vec!["host", "port", "username", "password"]);
        assert!(r.endpoint.is_none());
    }

    #[test]
    fn each_method_has_its_own_default_port() {
        let check = |m| {
            check_connection_form(
                m,
                String::new(),
                "pc.example.com".into(),
                String::new(),
                "alice".into(),
                String::new(),
                true,
            )
            .port
        };
        assert_eq!(check(ConnectionMethod::PrivateRemoteDesktop), Some(3389));
        assert_eq!(check(ConnectionMethod::ShareScreen), Some(3395));
        assert_eq!(check(ConnectionMethod::Vnc), Some(5900));
    }

    #[test]
    fn vnc_needs_no_username_but_the_others_do() {
        let r = |m| {
            check_connection_form(
                m,
                String::new(),
                "192.168.1.20".into(),
                String::new(),
                String::new(),
                String::new(),
                true,
            )
        };
        assert!(r(ConnectionMethod::Vnc).valid);
        assert!(!r(ConnectionMethod::ShareScreen).valid);
        assert!(!r(ConnectionMethod::PrivateRemoteDesktop).valid);
    }
}
