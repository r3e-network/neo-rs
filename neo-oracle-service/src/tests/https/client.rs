use super::*;
use crate::service::OracleServiceError;

#[test]
fn invalid_http_client_builder_returns_typed_startup_error() {
    let result =
        OracleHttpsProtocol::from_builder(reqwest::Client::builder().user_agent("bad\nagent"));

    let Err(OracleServiceError::HttpClientInitialization(message)) = result else {
        panic!("invalid builder must return an HTTP client initialization error");
    };
    assert!(message.contains("builder error"));
}
