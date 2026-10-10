// Pixel Connect — KDE Connect protocol v8, TLS transport.
//
// KDE Connect's LAN backend is TLS end to end with certificate pinning:
// the peer certificate's subject CommonName must equal the `deviceId` that the
// peer announced in its identity packet, and for already-paired devices the
// full certificate DER must equal the copy stored at pairing time.
//
// The peer certificate is *always* self-signed (KDE Connect generates
// self-signed device certificates on both desktop and Android), so ordinary PKI
// validation is meaningless here. Rather than disabling verification, we
// install an explicit verifier that enforces the properties KDE Connect
// actually relies on: a parseable certificate, a valid validity window, and a
// usable CommonName to pin against. The CommonName comparison itself happens in
// `verify_peer_identity*` right after the handshake.

use std::sync::Arc;

use anyhow::{anyhow, Result};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_rustls::TlsAcceptor;

use crate::crypto::certificate_common_name;
use crate::tls_verify::PinnedPeerVerifier;

/// Maximum time allowed for a TLS handshake.
pub const HANDSHAKE_TIMEOUT_SECS: u64 = 15;

pub struct TlsContext {
    client_config: Arc<rustls::ClientConfig>,
    server_config: Arc<rustls::ServerConfig>,
}

/// Which side of TLS we play.
///
/// KDE Connect inverts the TLS roles relative to TCP: whoever *opens* the TCP
/// connection plays the TLS **server**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsRole {
    Client,
    Server,
}

impl TlsContext {
    pub fn new(cert_pem: &str, key_pem: &str) -> Result<Self> {
        let certs: Vec<rustls::pki_types::CertificateDer<'static>> = {
            let mut cursor = std::io::Cursor::new(cert_pem.as_bytes());
            rustls_pemfile::certs(&mut cursor)
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| anyhow!("parse certificate PEM: {e}"))?
        };
        if certs.is_empty() {
            return Err(anyhow!("no certificate in PEM"));
        }
        let key = rustls_pemfile::private_key(&mut std::io::Cursor::new(key_pem.as_bytes()))
            .map_err(|e| anyhow!("parse private key PEM: {e}"))?
            .ok_or_else(|| anyhow!("no private key in PEM"))?;

        // TLS 1.2 is what KDE Connect for Android pins; 1.3 is offered as well
        // because the Qt-based desktop implementation allows it.
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let versions: &[&rustls::SupportedProtocolVersion] =
            &[&rustls::version::TLS13, &rustls::version::TLS12];

        // When we act as the TLS server (we opened the TCP connection) we ask
        // the peer for its device certificate. KDE Connect always sends one,
        // so this costs no interoperability while letting us pin the peer in
        // both directions.
        let server_config = rustls::ServerConfig::builder_with_provider(provider.clone())
            .with_protocol_versions(versions)
            .map_err(|e| anyhow!("tls server config: {e}"))?
            .with_client_cert_verifier(Arc::new(PinnedPeerVerifier::new_client()))
            .with_single_cert(certs.clone(), key.clone_key())
            .map_err(|e| anyhow!("tls certificate: {e}"))?;

        // The TLS client must present its device certificate too: the peer
        // (acting as the TLS server) requires one so it can pin us by
        // CommonName. KDE Connect clients always send a certificate, so this
        // costs no interoperability.
        let client_config = rustls::ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(versions)
            .map_err(|e| anyhow!("tls client config: {e}"))?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(PinnedPeerVerifier::new()))
            .with_client_auth_cert(certs.clone(), key.clone_key())
            .map_err(|e| anyhow!("tls client certificate: {e}"))?;

        Ok(Self {
            client_config: Arc::new(client_config),
            server_config: Arc::new(server_config),
        })
    }

    /// Perform the TLS handshake in the requested role.
    pub async fn handshake<S>(
        &self,
        role: TlsRole,
        stream: S,
    ) -> Result<TlsStream<S>>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let timeout = std::time::Duration::from_secs(HANDSHAKE_TIMEOUT_SECS);
        match role {
            TlsRole::Client => {
                let connector = tokio_rustls::TlsConnector::from(self.client_config.clone());
                let name = rustls::pki_types::ServerName::try_from(
                    crate::config::DEFAULT_TLS_SERVER_NAME,
                )
                .map_err(|e| anyhow!("invalid SNI name: {e}"))?;
                let s = tokio::time::timeout(timeout, connector.connect(name, stream))
                    .await
                    .map_err(|_| anyhow!("TLS handshake timed out"))?
                    .map_err(|e| anyhow!("TLS handshake failed: {e}"))?;
                Ok(TlsStream::Client(s))
            }
            TlsRole::Server => {
                let acceptor = TlsAcceptor::from(self.server_config.clone());
                let s = tokio::time::timeout(timeout, acceptor.accept(stream))
                    .await
                    .map_err(|_| anyhow!("TLS handshake timed out"))?
                    .map_err(|e| anyhow!("TLS handshake failed: {e}"))?;
                Ok(TlsStream::Server(s))
            }
        }
    }
}

/// KDE Connect's anti-spoofing check: the certificate CN must equal the device
/// id announced in the identity packet. Upstream logs a warning and aborts on
/// mismatch; we always abort. For paired devices the whole certificate DER is
/// additionally pinned to the copy stored at pairing time.
fn check_certificate(
    der: &[u8],
    announced_device_id: &str,
    pinned_der: Option<&[u8]>,
) -> Result<()> {
    let cn = certificate_common_name(der)
        .ok_or_else(|| anyhow!("peer certificate has no usable CommonName"))?;
    if cn != announced_device_id {
        return Err(anyhow!(
            "peer certificate CN {cn:?} does not match announced device id {announced_device_id:?}"
        ));
    }
    if let Some(pinned) = pinned_der {
        if pinned != der {
            return Err(anyhow!(
                "peer certificate changed since pairing with {announced_device_id}"
            ));
        }
    }
    Ok(())
}

pub fn verify_peer_identity<S>(
    stream: &tokio_rustls::client::TlsStream<S>,
    announced_device_id: &str,
    pinned_der: Option<&[u8]>,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (_, session) = stream.get_ref();
    let der = session
        .peer_certificates()
        .and_then(|c| c.first())
        .map(|c| c.as_ref())
        .ok_or_else(|| anyhow!("peer presented no certificate"))?;
    check_certificate(der, announced_device_id, pinned_der)
}

pub fn verify_peer_identity_server<S>(
    stream: &tokio_rustls::server::TlsStream<S>,
    announced_device_id: &str,
    pinned_der: Option<&[u8]>,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (_, conn) = stream.get_ref();
    let der = conn
        .peer_certificates()
        .and_then(|c| c.first())
        .map(|c| c.as_ref())
        .ok_or_else(|| anyhow!("peer presented no certificate"))?;
    check_certificate(der, announced_device_id, pinned_der)
}

pub async fn open_tcp(addr: &str) -> Result<TcpStream> {
    tokio::time::timeout(std::time::Duration::from_secs(5), TcpStream::connect(addr))
        .await
        .map_err(|_| anyhow!("TCP connect to {addr} timed out"))?
        .map_err(|e| anyhow!("TCP connect to {addr} failed: {e}"))
}

/// Either TLS role, unified so the rest of the crate can be written once.
///
/// KDE Connect inverts the TLS roles relative to TCP: whoever opens the TCP
/// connection plays the TLS server. Both variants expose the same session
/// inspection API.
pub enum TlsStream<S> {
    Client(tokio_rustls::client::TlsStream<S>),
    Server(tokio_rustls::server::TlsStream<S>),
}

impl<S> std::fmt::Debug for TlsStream<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TlsStream::Client(_) => f.write_str("TlsStream::Client"),
            TlsStream::Server(_) => f.write_str("TlsStream::Server"),
        }
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> TlsStream<S> {
    /// DER of the peer's leaf certificate.
    pub fn peer_cert_der(&self) -> Option<Vec<u8>> {
        match self {
            TlsStream::Client(s) => s
                .get_ref()
                .1
                .peer_certificates()
                .and_then(|c| c.first())
                .map(|c| c.as_ref().to_vec()),
            TlsStream::Server(s) => s
                .get_ref()
                .1
                .peer_certificates()
                .and_then(|c| c.first())
                .map(|c| c.as_ref().to_vec()),
        }
    }

    /// DER of the peer's SubjectPublicKeyInfo, used for the pairing
    /// verification key.
    pub fn peer_spki_der(&self) -> Vec<u8> {
        let Some(der) = self.peer_cert_der() else {
            return Vec::new();
        };
        x509_parser::parse_x509_certificate(&der)
            .ok()
            .map(|(_, c)| c.public_key().raw.to_vec())
            .unwrap_or_default()
    }

    /// KDE Connect's identity check: the certificate CN must equal the announced
    /// device id, and for paired devices the DER must equal the pinned copy.
    pub fn verify_peer(&self, announced_device_id: &str, pinned_der: Option<&[u8]>) -> Result<()> {
        let der = self
            .peer_cert_der()
            .ok_or_else(|| anyhow!("peer presented no certificate"))?;
        check_certificate(&der, announced_device_id, pinned_der)
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for TlsStream<S> {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            TlsStream::Client(s) => std::pin::Pin::new(s).poll_read(cx, buf),
            TlsStream::Server(s) => std::pin::Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for TlsStream<S> {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match self.get_mut() {
            TlsStream::Client(s) => std::pin::Pin::new(s).poll_write(cx, buf),
            TlsStream::Server(s) => std::pin::Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            TlsStream::Client(s) => std::pin::Pin::new(s).poll_flush(cx),
            TlsStream::Server(s) => std::pin::Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            TlsStream::Client(s) => std::pin::Pin::new(s).poll_shutdown(cx),
            TlsStream::Server(s) => std::pin::Pin::new(s).poll_shutdown(cx),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn certificate_mismatch_is_rejected() {
        let id = crate::crypto::generate_device_id();
        let ident = crate::crypto::generate_identity(&id, None).unwrap();
        check_certificate(&ident.cert_der, &id, None).unwrap();
        assert!(check_certificate(&ident.cert_der, "someone-else", None).is_err());
    }

    #[test]
    fn pinned_der_mismatch_is_rejected() {
        let id = crate::crypto::generate_device_id();
        let ident = crate::crypto::generate_identity(&id, None).unwrap();
        check_certificate(&ident.cert_der, &id, Some(&ident.cert_der)).unwrap();
        assert!(check_certificate(&ident.cert_der, &id, Some(b"not-a-cert")).is_err());
    }
}