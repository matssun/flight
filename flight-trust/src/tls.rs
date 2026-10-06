// SPDX-License-Identifier: MIT

use crate::{fingerprint_of_cert, Fingerprint, Identity, TrustError};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{ring, verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{
    CertificateError, ClientConfig, DigitallySignedStruct, DistinguishedName, Error, ServerConfig,
    SignatureScheme,
};
use std::sync::Arc;

fn provider() -> Arc<CryptoProvider> {
    Arc::new(ring::default_provider())
}

/// Accepts exactly one server identity: the pinned fingerprint. No CA, no hostname.
#[derive(Debug)]
struct PinnedServer {
    expected: Fingerprint,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for PinnedServer {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        match fingerprint_of_cert(end_entity) {
            Ok(actual) if actual == self.expected => Ok(ServerCertVerified::assertion()),
            _ => Err(Error::InvalidCertificate(
                CertificateError::ApplicationVerificationFailure,
            )),
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

/// Requires a client certificate and proves the client holds its key (the handshake
/// signature is verified), but does not decide trust: the application checks the peer's
/// fingerprint against the trust store, because an unenrolled node must still be able to
/// connect to enroll.
#[derive(Debug)]
struct AnyClient {
    provider: Arc<CryptoProvider>,
}

impl ClientCertVerifier for AnyClient {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, Error> {
        Ok(ClientCertVerified::assertion())
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

/// A client config that trusts exactly `expected` and presents `identity` (mutual TLS).
pub fn client_config(
    identity: &Identity,
    expected: &Fingerprint,
) -> Result<ClientConfig, TrustError> {
    let provider = provider();
    ClientConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| TrustError::Invalid(format!("client tls: {e}")))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinnedServer {
            expected: expected.clone(),
            provider,
        }))
        .with_client_auth_cert(vec![identity.cert_der()], identity.key_der())
        .map_err(|e| TrustError::Invalid(format!("client tls: {e}")))
}

/// A server config that presents `identity` and requires (and proves possession of) a client
/// certificate. Authorization is the application's job; see [`peer_fingerprint`].
pub fn server_config(identity: &Identity) -> Result<ServerConfig, TrustError> {
    let provider = provider();
    ServerConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| TrustError::Invalid(format!("server tls: {e}")))?
        .with_client_cert_verifier(Arc::new(AnyClient { provider }))
        .with_single_cert(vec![identity.cert_der()], identity.key_der())
        .map_err(|e| TrustError::Invalid(format!("server tls: {e}")))
}

/// The authenticated identity of a TLS peer: the fingerprint of the leaf certificate it
/// proved possession of.
pub fn peer_fingerprint(
    peer_certs: Option<&[CertificateDer<'_>]>,
) -> Result<Fingerprint, TrustError> {
    let leaf = peer_certs
        .and_then(|c| c.first())
        .ok_or_else(|| TrustError::Invalid("peer presented no certificate".to_owned()))?;
    fingerprint_of_cert(leaf)
}
