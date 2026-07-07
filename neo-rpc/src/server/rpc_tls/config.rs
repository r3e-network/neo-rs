//! RPC TLS `ServerConfig` construction from server settings.

use super::authorities::load_trusted_authorities;
use super::certificate::load_tls_identity;
use crate::server::rpc_server_settings::RpcServerConfig;
use neo_error::{CoreError, CoreResult};
use rustls::ServerConfig;
use rustls::server::AllowAnyAuthenticatedClient;
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

    let identity = load_tls_identity(cert_path, settings.ssl_cert_password.as_str()).await?;
    let builder = ServerConfig::builder().with_safe_defaults();
    let builder = if settings.trusted_authorities.is_empty() {
        builder.with_no_client_auth()
    } else {
        let roots = load_trusted_authorities(&settings.trusted_authorities)?;
        builder.with_client_cert_verifier(Arc::new(AllowAnyAuthenticatedClient::new(roots)))
    };
    let config = builder
        .with_single_cert(identity.certs, identity.key)
        .map_err(|err| {
            CoreError::other(format!("failed to configure TLS for {cert_path}: {err}"))
        })?;

    Ok(Some(Arc::new(config)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn tls_config_without_certificate_is_disabled() {
        let mut settings = RpcServerConfig::default();
        settings.ssl_cert_password = "ignored".to_string();
        settings.trusted_authorities = vec!["aa:bb".to_string()];

        let config = build_tls_config_from_settings(&settings)
            .await
            .expect("config result");

        assert!(config.is_none());
    }
}
