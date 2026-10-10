// Pixel Connect — KDE Connect protocol v8, TLS identity and pairing secrets.
//
// KDE Connect secures the LAN backend entirely with TLS plus certificate
// pinning: every device holds a self-signed EC (P-256 / secp256r1) certificate
// whose subject CommonName is the device id, and the peer's certificate CN is
// compared against the device id announced in the identity packet
// (kdeconnect-kde `core/backends/lan/lanlinkprovider.cpp`, ~line 650).
//
// We reproduce that model and additionally pin the full certificate DER for
// already-trusted devices, which is strictly stronger than upstream behaviour.

use anyhow::{anyhow, Context, Result};
use rcgen::{
    CertificateParams, DistinguishedName, DnType, IsCa, KeyPair, KeyUsagePurpose, SerialNumber,
};
use sha2::{Digest, Sha256};

/// Generate a device id that satisfies KDE Connect's `^[a-zA-Z0-9_-]{32,38}$`.
pub fn generate_device_id() -> String {
    use rand::Rng;
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::rng();
    (0..32)
        .map(|_| ALPHABET[rng.random_range(0..ALPHABET.len())] as char)
        .collect()
}

pub struct DeviceIdentity {
    pub cert_pem: String,
    pub key_pem: String,
    pub cert_der: Vec<u8>,
    /// DER of the SubjectPublicKeyInfo, used to compute the pairing
    /// verification key exactly like `PairingHandler::verificationKey()`.
    pub public_key_der: Vec<u8>,
}

/// Build the self-signed device certificate.
///
/// Subject: `CN=<device id>`, issuer identical (self-signed), validity
/// `now-1y .. now+10y` — the same window KDE Connect uses.
pub fn generate_identity(device_id: &str, common_name_override: Option<&str>) -> Result<DeviceIdentity> {
    let key_pair = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)
        .context("generate EC P-256 key pair")?;

    let mut dn = DistinguishedName::new();
    let cn = common_name_override.unwrap_or(device_id);
    // KDE Connect puts the *device id* in the CN; some older Android builds put
    // the device name there instead. The caller decides which by passing an
    // override, but the CN must stay an Ia5String of at most 64 bytes.
    let cn: String = cn.chars().take(64).collect();
    dn.push(DnType::CommonName, cn);
    dn.push(DnType::OrganizationName, "KDE");
    dn.push(DnType::OrganizationalUnitName, "KDE Connect");

    let now = time::OffsetDateTime::now_utc();
    let mut params = CertificateParams::new(Vec::new()).context("certificate params")?;
    params.distinguished_name = dn;
    params.not_before = now - time::Duration::days(365);
    params.not_after = now + time::Duration::days(365 * 10);
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyEncipherment,
    ];
    params.is_ca = IsCa::NoCa;

    // RFC 5280 caps serial numbers at 20 octets; use 20 random bytes.
    let mut serial = [0u8; 20];
    use rand::RngCore;
    rand::rng().fill_bytes(&mut serial);
    params.serial_number = Some(SerialNumber::from_slice(&serial));

    let cert = params.self_signed(&key_pair).context("self-sign certificate")?;

    let cert_pem = cert.pem();
    let key_pem = key_pair.serialize_pem();
    let cert_der = cert.der().to_vec();

    let public_key_der = key_pair.public_key_der();
    debug_assert!(!public_key_der.is_empty());

    Ok(DeviceIdentity {
        cert_pem,
        key_pem,
        cert_der,
        public_key_der,
    })
}

/// `PairingHandler::verificationKey()`: sort the two DER SubjectPublicKeyInfos
/// lexicographically, concatenate them, append the decimal pairing timestamp,
/// SHA-256, take the first 8 hex characters, uppercase.
///
/// Both KDE Connect desktop and KDE Connect Android compute it this way, so the
/// numbers shown to the user on either side match.
pub fn pairing_verification_key(
    our_public_key_der: &[u8],
    their_public_key_der: &[u8],
    timestamp_secs: i64,
) -> String {
    let (a, b) = if our_public_key_der < their_public_key_der {
        (our_public_key_der, their_public_key_der)
    } else {
        (their_public_key_der, our_public_key_der)
    };
    let mut hasher = Sha256::new();
    hasher.update(a);
    hasher.update(b);
    hasher.update(timestamp_secs.to_string().as_bytes());
    let digest = hasher.finalize();
    digest
        .iter()
        .take(4)
        .map(|b| format!("{b:02X}"))
        .collect::<String>()
}

/// Wrap DER certificate bytes in a PEM armour block, matching the format KDE
/// Connect stores in its `trusted_devices` file.
pub fn der_to_pem(der: &[u8]) -> String {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;
    format!(
        "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n",
        STANDARD.encode(der)
    )
}

/// Extract the CommonName from a parsed X.509 certificate.
pub fn subject_common_name(cert: &x509_parser::certificate::X509Certificate<'_>) -> Option<String> {
    cert.subject()
        .iter_common_name()
        .next()
        .and_then(|cn| cn.as_str().ok())
        .map(|s| s.to_string())
}

/// Extract the CN from a DER certificate without a caller-visible parser.
pub fn certificate_common_name(der: &[u8]) -> Option<String> {
    let (_, cert) = x509_parser::parse_x509_certificate(der).ok()?;
    subject_common_name(&cert)
}

/// Load a certificate chain from PEM, returning the leaf certificate DER.
pub fn certificate_der_from_pem(pem: &str) -> Result<Vec<u8>> {
    let mut cursor = std::io::Cursor::new(pem.as_bytes());
    let certs = rustls_pemfile::certs(&mut cursor)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| anyhow!("parse certificate PEM: {e}"))?;
    certs
        .into_iter()
        .next()
        .map(|c| c.to_vec())
        .ok_or_else(|| anyhow!("no certificate found in PEM"))
}

/// Sanitised device name for use in file names.
pub fn safe_file_stem(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .take(32)
        .collect();
    if cleaned.trim().is_empty() {
        "device".to_string()
    } else {
        cleaned.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_generates_and_round_trips() {
        let id = generate_device_id();
        let ident = generate_identity(&id, None).unwrap();
        assert!(ident.cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(ident.key_pem.contains("PRIVATE KEY"));
        let der = certificate_der_from_pem(&ident.cert_pem).unwrap();
        assert_eq!(der, ident.cert_der);
        assert_eq!(certificate_common_name(&ident.cert_der).unwrap(), id);
    }

    #[test]
    fn verification_key_is_order_independent_and_eight_chars() {
        let a = b"\x01\x02\x03";
        let b = b"\x09\x08\x07";
        assert_eq!(
            pairing_verification_key(a, b, 1_700_000_000),
            pairing_verification_key(b, a, 1_700_000_000)
        );
        let k = pairing_verification_key(a, b, 1_700_000_000);
        assert_eq!(k.len(), 8);
        assert!(k.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn der_to_pem_round_trips() {
        let id = generate_device_id();
        let ident = generate_identity(&id, None).unwrap();
        let pem = der_to_pem(&ident.cert_der);
        assert!(pem.starts_with("-----BEGIN CERTIFICATE-----\n"));
        assert_eq!(certificate_der_from_pem(&pem).unwrap(), ident.cert_der);
        assert_eq!(certificate_common_name(&certificate_der_from_pem(&pem).unwrap()).unwrap(), id);
    }

    #[test]
    fn pairing_timestamp_changes_the_key() {
        let a = b"key-one";
        let b = b"key-two";
        assert_ne!(
            pairing_verification_key(a, b, 1),
            pairing_verification_key(a, b, 2)
        );
    }
}