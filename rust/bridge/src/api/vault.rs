//! Saved passwords for the UI (Stage 5). Windows and Linux only; Android uses
//! the Keystore from Kotlin.

use rdpv_core::secret::SecretString;
use rdpv_core::vault::{self, VaultError};

/// Result of a vault operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaultResult {
    Ok,
    /// No password is saved for this connection.
    NotFound,
    /// No secure password storage on this device; nothing was saved.
    Unavailable,
    /// The storage is locked or access was refused.
    Locked,
    InvalidId,
}

impl From<Result<(), VaultError>> for VaultResult {
    fn from(r: Result<(), VaultError>) -> Self {
        match r {
            Ok(()) => Self::Ok,
            Err(VaultError::NotFound) => Self::NotFound,
            Err(VaultError::Unavailable) => Self::Unavailable,
            Err(VaultError::Locked) => Self::Locked,
            Err(VaultError::InvalidId) => Self::InvalidId,
        }
    }
}

/// Saves the password for a connection in the OS vault.
#[flutter_rust_bridge::frb(sync)]
pub fn vault_save_password(connection_id: String, password: String) -> VaultResult {
    let password = SecretString::new(password);
    vault::save(&connection_id, &password).into()
}

/// Whether a password is saved for this connection (`Ok` = yes, `NotFound` = no).
#[flutter_rust_bridge::frb(sync)]
pub fn vault_check_password(connection_id: String) -> VaultResult {
    vault::exists(&connection_id)
        .and_then(|found| {
            if found {
                Ok(())
            } else {
                Err(VaultError::NotFound)
            }
        })
        .into()
}

/// Deletes the saved password (no error if there was none).
#[flutter_rust_bridge::frb(sync)]
pub fn vault_delete_password(connection_id: String) -> VaultResult {
    vault::delete(&connection_id).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_results() {
        assert_eq!(VaultResult::from(Ok(())), VaultResult::Ok);
        assert_eq!(
            VaultResult::from(Err(VaultError::Locked)),
            VaultResult::Locked
        );
        assert_eq!(
            vault_save_password("bad id".into(), "x".into()),
            VaultResult::InvalidId
        );
    }
}
