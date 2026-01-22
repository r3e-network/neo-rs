//! NeoFS oracle protocol implementation (parity with Neo.Plugins.OracleService).

mod auth;
#[cfg(feature = "neofs-grpc")]
mod grpc;
mod http;
mod json;
mod parse;
#[cfg(feature = "neofs-grpc")]
mod proto;

#[cfg(test)]
mod tests;

use super::OracleServiceSettings;
use crate::network::p2p::payloads::OracleResponseCode;
use crate::wallets::KeyPair;
use auth::build_neofs_auth;
use http::normalize_neofs_endpoint;
use parse::parse_neofs_request;

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
        let request = match parse_neofs_request(url) {
            Ok(request) => request,
            Err(_) => return (OracleResponseCode::Error, String::new()),
        };
        let endpoint = match normalize_neofs_endpoint(&settings.neofs_endpoint) {
            Ok(endpoint) => endpoint,
            Err(_) => return (OracleResponseCode::Error, String::new()),
        };
        let auth = build_neofs_auth(settings, oracle_key);

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
