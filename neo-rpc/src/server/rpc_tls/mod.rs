//! # neo-rpc::server::rpc_tls
//!
//! TLS configuration helpers for RPC transports.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `rpc_tls`: TLS configuration loading for RPC listeners.

use super::rpc_server_settings::RpcServerConfig;
use neo_error::{CoreError, CoreResult};
use neo_primitives::hex_util;
use p12::PFX;
use rustls::server::AllowAnyAuthenticatedClient;
use rustls::{Certificate, PrivateKey, RootCertStore, ServerConfig};
use sha1::{Digest, Sha1};
use std::collections::HashSet;
use std::sync::Arc;
use tracing::warn;

/// Builds TLS configuration from RPC server settings asynchronously.
pub async fn build_tls_config_from_settings(
    settings: &RpcServerConfig,
) -> CoreResult<Option<Arc<ServerConfig>>> {
    build_tls_config(settings).await
}

async fn build_tls_config(settings: &RpcServerConfig) -> CoreResult<Option<Arc<ServerConfig>>> {
    let cert_path = settings.ssl_cert.trim();
    if cert_path.is_empty() {
        if !settings.ssl_cert_password.is_empty() || !settings.trusted_authorities.is_empty() {
            warn!(
                "RPC TLS settings provided without SslCert; TLS remains disabled (network {}).",
                settings.network
            );
        }
        return Ok(None);
    }

    let cert_bytes = tokio::fs::read(cert_path).await.map_err(|err| {
        CoreError::other(format!("failed to read TLS certificate {cert_path}: {err}"))
    })?;
    let pfx = PFX::parse(&cert_bytes)
        .map_err(|err| CoreError::other(format!("invalid PKCS#12 {cert_path}: {err:?}")))?;
    if !pfx.verify_mac(settings.ssl_cert_password.as_str()) {
        return Err(CoreError::other(format!(
            "invalid TLS certificate password for {cert_path}"
        )));
    }

    let certs_der = pfx
        .cert_x509_bags(settings.ssl_cert_password.as_str())
        .map_err(|err| {
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

    let mut keys = pfx
        .key_bags(settings.ssl_cert_password.as_str())
        .map_err(|err| {
            CoreError::other(format!(
                "failed to read TLS private key from {cert_path}: {err:?}"
            ))
        })?;
    let key_der = keys
        .pop()
        .ok_or_else(|| CoreError::other(format!("no TLS private key found in {cert_path}")))?;
    let key = PrivateKey(key_der);

    let builder = ServerConfig::builder().with_safe_defaults();
    let builder = if settings.trusted_authorities.is_empty() {
        builder.with_no_client_auth()
    } else {
        let roots = load_trusted_authorities(&settings.trusted_authorities)?;
        builder.with_client_cert_verifier(Arc::new(AllowAnyAuthenticatedClient::new(roots)))
    };
    let config = builder.with_single_cert(certs, key).map_err(|err| {
        CoreError::other(format!("failed to configure TLS for {cert_path}: {err}"))
    })?;

    Ok(Some(Arc::new(config)))
}

fn load_trusted_authorities(thumbprints: &[String]) -> CoreResult<RootCertStore> {
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
