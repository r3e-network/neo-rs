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
