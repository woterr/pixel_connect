// Pixel Connect — rustls certificate verifiers for KDE Connect peers.
//
// KDE Connect uses *self-signed* device certificates on both desktop and
// Android, so ordinary PKI chain validation can never succeed. Disabling
// verification outright would be unacceptable, so these verifiers instead
// enforce the properties KDE Connect actually relies on:
//
//   1. the leaf certificate parses as a valid X.509 structure and is inside its
//      declared validity window, and
//   2. the subject CommonName equals the `deviceId` the peer announced in its
//      identity packet.
//
// (2) is checked by the caller immediately after the handshake, in
// `tls::verify_peer_identity*`, because rustls only exposes the ServerName we
// supplied and not the peer's certificate subject at that point. For already
// paired devices the full certificate DER is additionally pinned to the copy
// stored at pairing time, which is stricter than upstream KDE Connect.

use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::DistinguishedName;
use rustls::{DigitallySignedStruct, Error, SignatureScheme};

pub struct PinnedPeerVerifier {
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl std::fmt::Debug for PinnedPeerVerifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PinnedPeerVerifier")
    }
}

impl PinnedPeerVerifier {
    pub fn new() -> Self {
        Self::new_client()
    }

    pub fn new_client() -> Self {
        Self {
            provider: Arc::new(rustls::crypto::ring::default_provider()),
        }
    }

    fn check_leaf(&self, end_entity: &CertificateDer<'_>) -> Result<(), Error> {
        let (_, parsed) = x509_parser::parse_x509_certificate(end_entity.as_ref())
            .map_err(|_| Error::General("peer certificate could not be parsed".into()))?;

        if crate::crypto::subject_common_name(&parsed).is_none() {
            return Err(Error::General(
                "peer certificate has no CommonName to pin against".into(),
            ));
        }

        if !parsed.validity().is_valid() {
            return Err(Error::General(
                "peer certificate is outside its validity period".into(),
            ));
        }

        Ok(())
    }
}

impl ServerCertVerifier for PinnedPeerVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        self.check_leaf(end_entity)?;
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls12_signature(
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
        rustls::crypto::verify_tls13_signature(
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

impl rustls::server::danger::ClientCertVerifier for PinnedPeerVerifier {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn offer_client_auth(&self) -> bool {
        true
    }

    fn client_auth_mandatory(&self) -> bool {
        // KDE Connect clients always present a device certificate; requiring
        // one lets us pin the peer even when we opened the TCP connection.
        true
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<rustls::server::danger::ClientCertVerified, Error> {
        self.check_leaf(end_entity)?;
        Ok(rustls::server::danger::ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        ServerCertVerifier::verify_tls12_signature(self, message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        ServerCertVerifier::verify_tls13_signature(self, message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        ServerCertVerifier::supported_verify_schemes(self)
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use std::collections::HashMap;

    #[test]
    fn generated_certificates_are_in_their_validity_window() {
        let id = crate::crypto::generate_device_id();
        let ident = crate::crypto::generate_identity(&id, None).unwrap();
        let (_, parsed) = x509_parser::parse_x509_certificate(&ident.cert_der).unwrap();
        assert!(parsed.validity().is_valid());
        assert_eq!(
            crate::crypto::subject_common_name(&parsed).as_deref(),
            Some(id.as_str())
        );
    }
}