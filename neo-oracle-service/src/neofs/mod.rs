//! # neo-oracle-service::neofs
//!
//! NeoFS request signing, authentication, JSON, and response helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-oracle-service`. This service crate owns oracle
//! request handling and must not decide block import, consensus, or storage
//! backend policy.
//!
//! ## Contents
//!
//! - `auth`: NeoFS authentication and authorization helpers.
//! - `grpc`: NeoFS gRPC client helpers.
//! - `http`: NeoFS HTTP client helpers.
//! - `json`: JSON models and codecs for external service integration.
//! - `parse`: NeoFS response parsing helpers.
//! - `proto`: Protocol message definitions and network payload framing.
//! - `tests`: Module-local tests and regression coverage.

mod auth;
#[cfg(feature = "neofs-grpc")]
mod grpc;
mod http;
mod json;
mod parse;
#[cfg(feature = "neofs-grpc")]
mod proto;

#[cfg(test)]
#[path = "../tests/neofs/mod.rs"]
mod tests;

use super::OracleServiceSettings;
use http::normalize_neofs_endpoint;
use neo_payloads::OracleResponseCode;
use neo_wallets::KeyPair;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NeoFsRange {
    offset: u64,
    length: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NeoFsCommand {
    Payload,
    Range(NeoFsRange),
    Header,
    Hash(Option<NeoFsRange>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NeoFsRequest {
    container: String,
    object: String,
    command: NeoFsCommand,
}

#[derive(Clone, Debug, Default)]
struct NeoFsAuth {
    token: Option<String>,
    signature: Option<String>,
    signature_key: Option<String>,
    wallet_connect: bool,
}

fn decode_raw_base58(value: &str, expected_len: Option<usize>) -> Option<Vec<u8>> {
    let decoded = neo_crypto::Base58::decode(value).ok()?;
    if expected_len.is_some_and(|len| decoded.len() != len) {
        return None;
    }
    Some(decoded)
}

pub(crate) struct OracleNeoFsProtocol {
    client: reqwest::Client,
}

impl OracleNeoFsProtocol {
    pub(crate) fn new() -> Self {
        let version = env!("CARGO_PKG_VERSION");
        let client = reqwest::Client::builder()
            .user_agent(format!("NeoOracleService/{}", version))
            .build()
            .expect("failed to build neofs http client");
        Self { client }
    }

    pub(crate) async fn process(
        &self,
        settings: &OracleServiceSettings,
        url: &str,
        oracle_key: Option<&KeyPair>,
    ) -> (OracleResponseCode, String) {
        let request = match NeoFsRequest::parse_neofs_request(url) {
            Ok(request) => request,
            Err(_) => return (OracleResponseCode::Error, String::new()),
        };
        let endpoint = match normalize_neofs_endpoint(&settings.neofs_endpoint) {
            Ok(endpoint) => endpoint,
            Err(_) => return (OracleResponseCode::Error, String::new()),
        };
        let auth = NeoFsAuth::build_neofs_auth(settings, oracle_key);

        if settings.neofs_use_grpc {
            #[cfg(feature = "neofs-grpc")]
            {
                let Some(key) = oracle_key else {
                    return (OracleResponseCode::Error, String::new());
                };
                let fut = self.execute_grpc_request(&endpoint, request, &auth, key);
                return match tokio::time::timeout(settings.neofs_timeout, fut).await {
                    Ok(result) => result,
                    Err(_) => (OracleResponseCode::Error, String::new()),
                };
            }
            #[cfg(not(feature = "neofs-grpc"))]
            {
                return (OracleResponseCode::Error, String::new());
            }
        }

        let fut = self.execute_request(&endpoint, request, &auth);
        match tokio::time::timeout(settings.neofs_timeout, fut).await {
            Ok(result) => result,
            Err(_) => (OracleResponseCode::Timeout, String::new()),
        }
    }
}
