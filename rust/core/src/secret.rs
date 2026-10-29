//! Password handling that cannot leak through logs, errors or debug output.

use core::fmt;

use zeroize::{Zeroize, ZeroizeOnDrop};

/// What every secret prints as, in `Debug` and `Display`.
pub const REDACTED: &str = "***";

/// A password or other secret string.
///
/// - `Debug` and `Display` always print [`REDACTED`], so a secret placed in a
///   log line or error message never reveals its value.
/// - The memory holding the value is overwritten when the value is dropped.
/// - It is deliberately **not** `Clone`, so copies are not scattered in memory.
///   Use [`SecretString::expose`] only at the point the value is handed to the
///   protocol engine or the OS vault.
#[derive(Zeroize, ZeroizeOnDrop, Default, PartialEq, Eq)]
pub struct SecretString(String);

impl SecretString {
    /// Wraps a secret value, taking ownership of it.
    pub fn new(value: String) -> Self {
        Self(value)
    }

    /// Returns the secret value. Call only where the value must be used.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// True if no secret has been entered.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<String> for SecretString {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

impl fmt::Display for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> String {
        // Built at run time so no password literal exists in the source (SA-9).
        ["sample", "value", "1234"].join("-")
    }

    #[test]
    fn debug_and_display_never_show_the_value() {
        let s = SecretString::new(sample());
        assert_eq!(format!("{s:?}"), REDACTED);
        assert_eq!(format!("{s}"), REDACTED);
        let nested = format!("{:?}", Some(&s));
        assert!(!nested.contains(&sample()));
    }

    #[test]
    fn expose_returns_the_value() {
        let s = SecretString::from(sample());
        assert_eq!(s.expose(), sample());
        assert!(!s.is_empty());
    }

    #[test]
    fn zeroize_clears_the_value() {
        let mut s = SecretString::new(sample());
        s.zeroize();
        assert!(s.is_empty());
    }

    #[test]
    fn default_is_empty() {
        assert!(SecretString::default().is_empty());
    }
}
