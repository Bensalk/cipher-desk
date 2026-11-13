//! The Host's own TLS certificate.
//!
//! Created once (self-signed, ECDSA P-256) and kept in the app's private data
//! folder, so its fingerprint stays the same and the viewer can pin it like any
//! Remote Desktop computer. The fingerprint is shown on the "Share this screen"
//! page for the user to compare on first connection.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context as _;
use ironrdp_server::tokio_rustls::rustls::pki_types::pem::PemObject as _;
use ironrdp_server::tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
use ironrdp_server::tokio_rustls::rustls::{self, ServerConfig};
use ironrdp_server::tokio_rustls::TlsAcceptor;

const CERT_FILE: &str = "host_certificate.pem";
const KEY_FILE: &str = "host_private_key.pem";

/// The certificate and key the Host uses.
pub struct Identity {
    pub cert_der: Vec<u8>,
    key_der: PrivateKeyDer<'static>,
    /// Subject public key, needed for Network Level Authentication.
    pub pub_key: Vec<u8>,
}

impl Identity {
    /// SHA-256 fingerprint in the same form the viewer shows ("AB:12:…").
    pub fn fingerprint(&self) -> String {
        rdpv_core::pins::fingerprint(&self.cert_der)
    }

    /// TLS acceptor with an explicit crypto provider and no key log.
    pub fn acceptor(&self) -> anyhow::Result<TlsAcceptor> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .context("TLS versions")?
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(self.cert_der.clone())],
                self.key_der.clone_key(),
            )
            .context("certificate and key")?;
        Ok(TlsAcceptor::from(Arc::new(config)))
    }
}

fn paths(dir: &Path) -> (PathBuf, PathBuf) {
    (dir.join(CERT_FILE), dir.join(KEY_FILE))
}

/// Loads the Host certificate from `dir`, creating it the first time.
pub fn load_or_create(dir: &Path) -> anyhow::Result<Identity> {
    let (cert_path, key_path) = paths(dir);
    if !cert_path.exists() || !key_path.exists() {
        create(&cert_path, &key_path)?;
    }
    load(&cert_path, &key_path)
}

/// Makes a new certificate (the fingerprint changes; viewers must check it again).
pub fn recreate(dir: &Path) -> anyhow::Result<Identity> {
    let (cert_path, key_path) = paths(dir);
    create(&cert_path, &key_path)?;
    load(&cert_path, &key_path)
}

fn create(cert_path: &Path, key_path: &Path) -> anyhow::Result<()> {
    let key = rcgen::KeyPair::generate().context("new key")?;
    let mut params = rcgen::CertificateParams::new(vec!["cipher-desk-host".to_owned()])
        .context("certificate parameters")?;
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "Cipher Desk Host");
    // Long-lived on purpose: the fingerprint is what the viewer trusts.
    params.not_before = rcgen::date_time_ymd(2026, 1, 1);
    params.not_after = rcgen::date_time_ymd(2046, 1, 1);
    let cert = params
        .self_signed(&key)
        .context("self-signed certificate")?;
    if let Some(dir) = cert_path.parent() {
        std::fs::create_dir_all(dir).context("data folder")?;
    }
    rdpv_core::fsutil::write_atomically(key_path, key.serialize_pem().as_bytes())
        .context("saving the private key")?;
    rdpv_core::fsutil::write_atomically(cert_path, cert.pem().as_bytes())
        .context("saving the certificate")?;
    Ok(())
}

fn load(cert_path: &Path, key_path: &Path) -> anyhow::Result<Identity> {
    let cert = CertificateDer::from_pem_file(cert_path).context("reading the certificate")?;
    let key_pem = zeroize::Zeroizing::new(
        std::fs::read_to_string(key_path).context("reading the private key")?,
    );
    let key_der = PrivateKeyDer::from_pem_slice(key_pem.as_bytes()).context("private key")?;
    // For NLA the server proves it owns the key whose public part is in the
    // certificate ("subject public key", the raw key bytes).
    let pub_key = rcgen::KeyPair::from_pem(&key_pem)
        .context("private key")?
        .public_key_raw()
        .to_vec();
    Ok(Identity {
        cert_der: cert.to_vec(),
        key_der,
        pub_key,
    })
}
