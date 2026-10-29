//! Plain-language error and status text for the UI (Stage 3).

use rdpv_core::status::ConnectionError;

/// Text for one kind of connection error.
#[derive(Debug, Clone)]
pub struct ErrorInfo {
    /// Stable identifier, e.g. "WrongCredentials".
    pub id: String,
    pub title: String,
    pub advice: String,
    /// Show with warning colours and an icon.
    pub security_warning: bool,
}

/// Converts an error to its UI text (used by the session stream).
pub(crate) fn error_info(e: ConnectionError) -> ErrorInfo {
    ErrorInfo {
        id: format!("{e:?}"),
        title: e.title().to_owned(),
        advice: e.advice().to_owned(),
        security_warning: e.is_security_warning(),
    }
}

/// Every error the app can show, for the help screen and for looking up by id.
#[flutter_rust_bridge::frb(sync)]
pub fn all_connection_errors() -> Vec<ErrorInfo> {
    ConnectionError::ALL
        .iter()
        .copied()
        .map(error_info)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_text_present() {
        let all = all_connection_errors();
        assert_eq!(all.len(), ConnectionError::ALL.len());
        let mut ids: Vec<&str> = all.iter().map(|e| e.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), all.len());
        assert!(all
            .iter()
            .any(|e| e.id == "CertificateChanged" && e.security_warning));
    }
}
