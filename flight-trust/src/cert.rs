// SPDX-License-Identifier: MIT

use crate::{Fingerprint, TrustError};
use x509_parser::prelude::{FromDer, X509Certificate};

/// The identity of the key a certificate carries.
pub fn fingerprint_of_cert(der: &[u8]) -> Result<Fingerprint, TrustError> {
    let (_, cert) = X509Certificate::from_der(der)
        .map_err(|e| TrustError::Invalid(format!("not an X.509 certificate: {e}")))?;
    Ok(Fingerprint::of_spki(cert.tbs_certificate.subject_pki.raw))
}
