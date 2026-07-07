//! PKCS#12 server identity loading for RPC TLS listeners.

use neo_error::{CoreError, CoreResult};
use p12::PFX;
use rustls::{Certificate, PrivateKey};

pub(super) struct TlsIdentity {
    pub(super) certs: Vec<Certificate>,
    pub(super) key: PrivateKey,
}

pub(super) async fn load_tls_identity(cert_path: &str, password: &str) -> CoreResult<TlsIdentity> {
    let cert_bytes = tokio::fs::read(cert_path).await.map_err(|err| {
        CoreError::other(format!("failed to read TLS certificate {cert_path}: {err}"))
    })?;
    parse_tls_identity(cert_path, password, &cert_bytes)
}

fn parse_tls_identity(
    cert_path: &str,
    password: &str,
    cert_bytes: &[u8],
) -> CoreResult<TlsIdentity> {
    let pfx = PFX::parse(cert_bytes)
        .map_err(|err| CoreError::other(format!("invalid PKCS#12 {cert_path}: {err:?}")))?;
    if !pfx.verify_mac(password) {
        return Err(CoreError::other(format!(
            "invalid TLS certificate password for {cert_path}"
        )));
    }

    let certs_der = pfx.cert_x509_bags(password).map_err(|err| {
        CoreError::other(format!(
            "failed to read TLS certificate chain from {cert_path}: {err:?}"
        ))
    })?;
    if certs_der.is_empty() {
        return Err(CoreError::other(format!(
            "no TLS certificates found in {cert_path}"
        )));
    }
    let certs = certs_der.into_iter().map(Certificate).collect::<Vec<_>>();

    let mut keys = pfx.key_bags(password).map_err(|err| {
        CoreError::other(format!(
            "failed to read TLS private key from {cert_path}: {err:?}"
        ))
    })?;
    let key_der = keys
        .pop()
        .ok_or_else(|| CoreError::other(format!("no TLS private key found in {cert_path}")))?;
    let key = PrivateKey(key_der);

    Ok(TlsIdentity { certs, key })
}
