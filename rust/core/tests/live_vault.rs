//! Live tests of the OS vault (Linux Secret Service here; Windows Credential
//! Manager in CI). Ignored by default.
//!
//! With a keyring:     RDPV_TEST_VAULT=available  cargo test -p rdpv-core --test live_vault -- --ignored
//! Without a keyring:  RDPV_TEST_VAULT=missing    (run with no Secret Service on the session bus)

use rdpv_core::secret::SecretString;
use rdpv_core::vault::{self, VaultError};

fn mode() -> Option<String> {
    std::env::var("RDPV_TEST_VAULT").ok()
}

fn sample() -> SecretString {
    // Different every run, so a stale entry can never make the test pass.
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    SecretString::new(format!("live-vault-{n}"))
}

#[test]
#[ignore = "needs an OS vault (see module docs)"]
fn real_vault_round_trip() -> Result<(), String> {
    if mode().as_deref() != Some("available") {
        return Err("set RDPV_TEST_VAULT=available".into());
    }
    let id = "live-test-entry";
    let pw = sample();
    vault::save(id, &pw).map_err(|e| e.to_string())?;
    let back = vault::load(id).map_err(|e| e.to_string())?;
    assert_eq!(back.expose(), pw.expose());
    // Replacing works.
    let pw2 = sample();
    vault::save(id, &pw2).map_err(|e| e.to_string())?;
    assert_eq!(
        vault::load(id).map_err(|e| e.to_string())?.expose(),
        pw2.expose()
    );
    // Deleted means gone.
    vault::delete(id).map_err(|e| e.to_string())?;
    assert_eq!(vault::load(id).err(), Some(VaultError::NotFound));
    assert_eq!(vault::exists(id), Ok(false));
    Ok(())
}

#[test]
#[ignore = "needs a session without an OS vault (see module docs)"]
fn missing_vault_refuses_to_save() -> Result<(), String> {
    if mode().as_deref() != Some("missing") {
        return Err("set RDPV_TEST_VAULT=missing".into());
    }
    match vault::save("live-test-missing", &sample()) {
        Err(VaultError::Unavailable | VaultError::Locked) => Ok(()),
        other => Err(format!("expected Unavailable, got {other:?}")),
    }
}
