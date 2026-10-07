// SPDX-License-Identifier: MIT

use crate::{fingerprint_of_cert, Fingerprint, TrustError};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use std::fs;
use std::path::Path;

const KEY_FILE: &str = "key.pem";
const CERT_FILE: &str = "cert.pem";

/// A long-lived keypair with a self-signed certificate. Its fingerprint is its id.
pub struct Identity {
    key_der: Vec<u8>,
    cert_der: Vec<u8>,
    key_pem: String,
    cert_pem: String,
    fingerprint: Fingerprint,
}

impl Identity {
    pub fn generate() -> Result<Self, TrustError> {
        let certified = rcgen::generate_simple_self_signed(vec!["flight".to_owned()])
            .map_err(|e| TrustError::Generate(e.to_string()))?;
        let cert_der = certified.cert.der().to_vec();
        let fingerprint = fingerprint_of_cert(&cert_der)?;
        Ok(Self {
            key_der: certified.signing_key.serialize_der(),
            key_pem: certified.signing_key.serialize_pem(),
            cert_pem: certified.cert.pem(),
            cert_der,
            fingerprint,
        })
    }

    pub fn fingerprint(&self) -> &Fingerprint {
        &self.fingerprint
    }

    pub fn cert_der(&self) -> CertificateDer<'static> {
        CertificateDer::from(self.cert_der.clone())
    }

    pub fn key_der(&self) -> PrivateKeyDer<'static> {
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key_der.clone()))
    }

    /// Write `key.pem` (owner-only) and `cert.pem` into `dir`.
    pub fn save(&self, dir: &Path) -> Result<(), TrustError> {
        fs::create_dir_all(dir)?;
        write_private(&dir.join(KEY_FILE), &self.key_pem)?;
        fs::write(dir.join(CERT_FILE), &self.cert_pem)?;
        Ok(())
    }

    pub fn load(dir: &Path) -> Result<Self, TrustError> {
        let key_pem = fs::read_to_string(dir.join(KEY_FILE))?;
        let cert_pem = fs::read_to_string(dir.join(CERT_FILE))?;
        let key_der = pem_body(&key_pem, "PRIVATE KEY")?;
        let cert_der = pem_body(&cert_pem, "CERTIFICATE")?;
        let fingerprint = fingerprint_of_cert(&cert_der)?;
        Ok(Self {
            key_der,
            cert_der,
            key_pem,
            cert_pem,
            fingerprint,
        })
    }

    /// Whether `dir` already holds an identity.
    pub fn exists(dir: &Path) -> bool {
        dir.join(KEY_FILE).exists() && dir.join(CERT_FILE).exists()
    }

    /// Remove the identity files from `dir` (and `dir` itself if that leaves it empty).
    pub fn remove(dir: &Path) {
        let _ = fs::remove_file(dir.join(KEY_FILE));
        let _ = fs::remove_file(dir.join(CERT_FILE));
        let _ = fs::remove_dir(dir);
    }

    /// The identity in `dir`, generated and saved first if there is none yet.
    pub fn load_or_create(dir: &Path) -> Result<Self, TrustError> {
        if Self::exists(dir) {
            return Self::load(dir);
        }
        let identity = Self::generate()?;
        identity.save(dir)?;
        Ok(identity)
    }
}

#[cfg(unix)]
fn write_private(path: &Path, contents: &str) -> Result<(), TrustError> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(contents.as_bytes())?;
    Ok(())
}

#[cfg(not(unix))]
fn write_private(path: &Path, contents: &str) -> Result<(), TrustError> {
    fs::write(path, contents)?;
    Ok(())
}

/// The DER bytes of the first PEM block whose label ends with `label`.
fn pem_body(pem: &str, label: &str) -> Result<Vec<u8>, TrustError> {
    use base64::Engine;
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    let start = pem
        .find(&begin)
        .ok_or_else(|| TrustError::Invalid(format!("no {label} block")))?
        + begin.len();
    let stop = pem[start..]
        .find(&end)
        .ok_or_else(|| TrustError::Invalid(format!("unterminated {label} block")))?;
    let body: String = pem[start..start + stop].split_whitespace().collect();
    base64::engine::general_purpose::STANDARD
        .decode(body)
        .map_err(|e| TrustError::Invalid(format!("bad base64 in {label}: {e}")))
}
