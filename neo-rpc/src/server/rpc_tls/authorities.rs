//! Trusted client-certificate authority loading for RPC TLS.

use neo_error::{CoreError, CoreResult};
use neo_primitives::hex_util;
use rustls::{Certificate, RootCertStore};
use sha1::{Digest, Sha1};
use std::collections::HashSet;
use tracing::warn;

pub(super) fn load_trusted_authorities(thumbprints: &[String]) -> CoreResult<RootCertStore> {
    let allowed: HashSet<String> = thumbprints
        .iter()
        .map(|value| normalize_thumbprint(value))
        .filter(|value| !value.is_empty())
        .collect();
    let native_certs = rustls_native_certs::load_native_certs()
        .map_err(|err| CoreError::other(format!("failed to load native TLS roots: {err:?}")))?;

    let mut roots = RootCertStore::empty();
    let mut matched = 0usize;
    for cert in native_certs {
        let cert_der = cert.0;
        let thumbprint = thumbprint_hex(&cert_der);
        if allowed.contains(&thumbprint) {
            let rustls_cert = Certificate(cert_der);
            roots.add(&rustls_cert).map_err(|err| {
                CoreError::other(format!(
                    "failed to add trusted authority {thumbprint}: {err}"
                ))
            })?;
            matched += 1;
        }
    }

    if matched == 0 {
        warn!("RPC TLS configured with TrustedAuthorities, but no matching roots were found.");
    }

    Ok(roots)
}

fn thumbprint_hex(cert_der: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(cert_der);
    let digest = hasher.finalize();
    hex_util::encode_hex_upper(&digest)
}

fn normalize_thumbprint(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .replace(':', "")
        .to_ascii_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_thumbprint_removes_quotes_colons_and_case() {
        assert_eq!(normalize_thumbprint(" \"aa:bb:CC:00\" "), "AABBCC00");
    }

    #[test]
    fn thumbprint_hex_returns_uppercase_sha1_digest() {
        assert_eq!(
            thumbprint_hex(b"neo-rs"),
            "09CB02E47C9637FDAC0B09C44B23EC7D7D4525EA"
        );
    }
}
