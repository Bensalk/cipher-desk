//! TLS for RDP with certificate pinning (S1, S2, SA-6).
//!
//! RDP hosts normally use self-signed certificates, so normal web-style chain
//! validation does not apply. Instead the leaf certificate's SHA-256
//! fingerprint must equal the pinned one ([`crate::pins`]). There is no
//! "accept any certificate" mode.
//!
//! Handshake signatures are always verified with the presented certificate,
//! so a copied certificate without its private key is rejected.
//! Only TLS 1.2 and 1.3 are possible (rustls does not implement older versions).

use std::sync::{Arc, Mutex};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{CertificateError, DigitallySignedStruct, Error, SignatureScheme};

use crate::pins::fingerprint;

/// What the verifier saw during the handshake.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Observed {
    /// Fingerprint of the certificate the server presented.
    pub fingerprint: Option<String>,
}

/// Accepts the server only if its certificate matches `expected`.
///
/// With `expected = None` (host never seen) the handshake is always refused:
/// the caller shows the fingerprint to the user, and reconnects with it as
/// `expected` once the user approves. Nothing is sent to an unapproved host.
#[derive(Debug)]
pub struct PinnedCertVerifier {
    expected: Option<String>,
    observed: Arc<Mutex<Observed>>,
    provider: Arc<CryptoProvider>,
}

impl PinnedCertVerifier {
    pub fn new(expected: Option<String>, provider: Arc<CryptoProvider>) -> Self {
        Self {
            expected,
            observed: Arc::new(Mutex::new(Observed::default())),
            provider,
        }
    }

    /// Shared handle to what was observed, readable after the handshake.
    pub fn observed(&self) -> Arc<Mutex<Observed>> {
        Arc::clone(&self.observed)
    }
}

impl ServerCertVerifier for PinnedCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        let presented = fingerprint(end_entity.as_ref());
        if let Ok(mut o) = self.observed.lock() {
            o.fingerprint = Some(presented.clone());
        }
        match &self.expected {
            Some(pinned) if *pinned == presented => Ok(ServerCertVerified::assertion()),
            Some(_) => Err(Error::InvalidCertificate(
                CertificateError::ApplicationVerificationFailure,
            )),
            None => Err(Error::InvalidCertificate(CertificateError::UnknownIssuer)),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// The crypto provider used for RDP TLS (explicit, so it never depends on
/// which provider another library installed as the process default).
pub fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// Builds a client configuration that pins `expected`.
pub fn client_config(
    expected: Option<String>,
) -> Result<(rustls::ClientConfig, Arc<Mutex<Observed>>), Error> {
    let provider = provider();
    let verifier = PinnedCertVerifier::new(expected, Arc::clone(&provider));
    let observed = verifier.observed();
    let mut config = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    // CredSSP does not support TLS session resumption (MS-CSSP).
    config.resumption = rustls::client::Resumption::disabled();
    Ok((config, observed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verify(expected: Option<&str>, cert: &[u8]) -> (Result<(), Error>, Observed) {
        let v = PinnedCertVerifier::new(expected.map(str::to_owned), provider());
        let name = ServerName::try_from("example.com").map_err(|_| Error::General("name".into()));
        let r = name.and_then(|n| {
            v.verify_server_cert(
                &CertificateDer::from(cert.to_vec()),
                &[],
                &n,
                &[],
                UnixTime::now(),
            )
            .map(|_| ())
        });
        let observed = v.observed().lock().map(|o| o.clone()).unwrap_or_default();
        (r, observed)
    }

    #[test]
    fn unknown_host_is_refused_but_fingerprint_is_reported() {
        let (r, o) = verify(None, b"cert");
        assert!(matches!(
            r,
            Err(Error::InvalidCertificate(CertificateError::UnknownIssuer))
        ));
        assert_eq!(o.fingerprint, Some(fingerprint(b"cert")));
    }

    #[test]
    fn pinned_certificate_is_accepted() {
        let fp = fingerprint(b"cert");
        let (r, _) = verify(Some(&fp), b"cert");
        assert!(r.is_ok());
    }

    #[test]
    fn changed_certificate_is_refused() {
        let fp = fingerprint(b"old cert");
        let (r, o) = verify(Some(&fp), b"new cert");
        assert!(matches!(
            r,
            Err(Error::InvalidCertificate(
                CertificateError::ApplicationVerificationFailure
            ))
        ));
        assert_eq!(o.fingerprint, Some(fingerprint(b"new cert")));
    }

    #[test]
    fn config_builds_with_tls12_and_13_only() {
        assert!(client_config(None).is_ok());
        let schemes = PinnedCertVerifier::new(None, provider()).supported_verify_schemes();
        assert!(!schemes.is_empty());
    }
}
