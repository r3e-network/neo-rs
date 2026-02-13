// Copyright (C) 2015-2025 The Neo Project.
//
// state_api.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::models::{RpcFoundStates, RpcStateRoot};
use crate::{RpcClient, RpcError};
use base64::{engine::general_purpose, Engine as _};
use neo_json::JToken;
use neo_primitives::{UInt160, UInt256};
use std::sync::Arc;

/// State service API
/// Matches C# `StateAPI`
pub struct StateApi {
    /// The RPC client instance
    rpc_client: Arc<RpcClient>,
}

impl StateApi {
    /// `StateAPI` Constructor
    /// Matches C# constructor
    #[must_use]
    pub const fn new(rpc_client: Arc<RpcClient>) -> Self {
        Self { rpc_client }
    }

    /// Get state root by index
    /// Matches C# `GetStateRootAsync`
    pub async fn get_state_root(&self, index: u32) -> Result<RpcStateRoot, RpcError> {
        let result = self
            .rpc_client
            .rpc_send_async("getstateroot", vec![JToken::Number(f64::from(index))])
            .await?;

        let obj = result.as_object().ok_or("Invalid response format")?;

        RpcStateRoot::from_json(obj).map_err(std::convert::Into::into)
    }

    /// Get proof for a storage key
    /// Matches C# `GetProofAsync`
    pub async fn get_proof(
        &self,
        root_hash: &UInt256,
        script_hash: &UInt160,
        key: &[u8],
    ) -> Result<Vec<u8>, RpcError> {
        let result = self
            .rpc_client
            .rpc_send_async(
                "getproof",
                vec![
                    JToken::String(root_hash.to_string()),
                    JToken::String(script_hash.to_string()),
                    JToken::String(general_purpose::STANDARD.encode(key)),
                ],
            )
            .await?;

        let proof_str = result.as_string().ok_or("Invalid response format")?;

        general_purpose::STANDARD
            .decode(proof_str)
            .map_err(std::convert::Into::into)
    }

    /// Verify a proof
    /// Matches C# `VerifyProofAsync`
    pub async fn verify_proof(
        &self,
        root_hash: &UInt256,
        proof_bytes: &[u8],
    ) -> Result<Vec<u8>, RpcError> {
        let result = self
            .rpc_client
            .rpc_send_async(
                "verifyproof",
                vec![
                    JToken::String(root_hash.to_string()),
                    JToken::String(general_purpose::STANDARD.encode(proof_bytes)),
                ],
            )
            .await?;

        let value_str = result.as_string().ok_or("Invalid response format")?;

        general_purpose::STANDARD
            .decode(value_str)
            .map_err(std::convert::Into::into)
    }

    /// Get state height information
    /// Matches C# `GetStateHeightAsync`
    pub async fn get_state_height(&self) -> Result<(Option<u32>, Option<u32>), RpcError> {
        let result = self
            .rpc_client
            .rpc_send_async("getstateheight", vec![])
            .await?;

        let obj = result.as_object().ok_or("Invalid response format")?;

        let local_root_index = obj
            .get("localrootindex")
            .and_then(neo_json::JToken::as_number)
            .map(|n| n as u32);

        let validated_root_index = obj
            .get("validatedrootindex")
            .and_then(neo_json::JToken::as_number)
            .map(|n| n as u32);

        Ok((local_root_index, validated_root_index))
    }

    /// Make parameters for find states call
    /// Matches C# `MakeFindStatesParams`
    #[must_use]
    pub fn make_find_states_params(
        root_hash: &UInt256,
        script_hash: &UInt160,
        prefix: &[u8],
        from: Option<&[u8]>,
        count: Option<i32>,
    ) -> Vec<JToken> {
        let mut params = vec![
            JToken::String(root_hash.to_string()),
            JToken::String(script_hash.to_string()),
            JToken::String(general_purpose::STANDARD.encode(prefix)),
            JToken::String(general_purpose::STANDARD.encode(from.unwrap_or(&[]))),
        ];

        if let Some(c) = count {
            params.push(JToken::Number(f64::from(c)));
        }

        params
    }

    /// Find states with prefix
    /// Matches C# `FindStatesAsync`
    pub async fn find_states(
        &self,
        root_hash: &UInt256,
        script_hash: &UInt160,
        prefix: &[u8],
        from: Option<&[u8]>,
        count: Option<i32>,
    ) -> Result<RpcFoundStates, RpcError> {
        let params = Self::make_find_states_params(root_hash, script_hash, prefix, from, count);

        let result = self.rpc_client.rpc_send_async("findstates", params).await?;

        let obj = result.as_object().ok_or("Invalid response format")?;

        RpcFoundStates::from_json(obj).map_err(std::convert::Into::into)
    }

    /// Get state value
    /// Matches C# `GetStateAsync`
    pub async fn get_state(
        &self,
        root_hash: &UInt256,
        script_hash: &UInt160,
        key: &[u8],
    ) -> Result<Vec<u8>, RpcError> {
        let result = self
            .rpc_client
            .rpc_send_async(
                "getstate",
                vec![
                    JToken::String(root_hash.to_string()),
                    JToken::String(script_hash.to_string()),
                    JToken::String(general_purpose::STANDARD.encode(key)),
                ],
            )
            .await?;

        let value_str = result.as_string().ok_or("Invalid response format")?;

        general_purpose::STANDARD
            .decode(value_str)
            .map_err(std::convert::Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RpcClient;
    use mockito::{Matcher, Server};
    use regex::escape;
    use reqwest::Url;
    use std::net::TcpListener;
    use std::sync::Arc;

    fn localhost_binding_permitted() -> bool {
        TcpListener::bind("127.0.0.1:0").is_ok()
    }

    #[tokio::test]
    async fn state_api_get_state_root_parses_response() {
        if !localhost_binding_permitted() {
            return;
        }
        let mut server = Server::new_async().await;
        let root_hash = UInt256::zero();
        let body = format!(
            r#"{{"jsonrpc":"2.0","id":1,"result":{{"version":0,"index":1,"roothash":"{root_hash}"}}}}"#
        );
        let _m = server
            .mock("POST", "/")
            .match_body(Matcher::Regex(
                r#""method"\s*:\s*"getstateroot".*"params"\s*:\s*\[\s*1\s*\]"#.to_string(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let url = Url::parse(&server.url()).expect("server url");
        let client = RpcClient::builder(url).build().expect("client");
        let api = StateApi::new(Arc::new(client));

        let parsed = api.get_state_root(1).await.expect("state root");
        assert_eq!(parsed.version, 0);
        assert_eq!(parsed.index, 1);
        assert_eq!(parsed.root_hash, root_hash);
        assert!(parsed.witness.is_none());
    }

    #[tokio::test]
    async fn state_api_get_state_height_handles_nullable_indices() {
        if !localhost_binding_permitted() {
            return;
        }
        let mut server = Server::new_async().await;
        let body =
            r#"{"jsonrpc":"2.0","id":1,"result":{"localrootindex":2,"validatedrootindex":null}}"#;
        let _m = server
            .mock("POST", "/")
            .match_body(Matcher::Regex(
                r#""method"\s*:\s*"getstateheight".*"params"\s*:\s*\[\s*\]"#.to_string(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let url = Url::parse(&server.url()).expect("server url");
        let client = RpcClient::builder(url).build().expect("client");
        let api = StateApi::new(Arc::new(client));

        let (local, validated) = api.get_state_height().await.expect("state height");
        assert_eq!(local, Some(2));
        assert_eq!(validated, None);
    }

    #[tokio::test]
    async fn state_api_get_state_and_proof_round_trip_bytes() {
        if !localhost_binding_permitted() {
            return;
        }
        let mut server = Server::new_async().await;
        let root_hash = UInt256::zero();
        let script_hash = UInt160::zero();
        let key = b"state-key";
        let value = b"value";
        let key_b64 = general_purpose::STANDARD.encode(key);
        let value_b64 = general_purpose::STANDARD.encode(value);

        let _m = server
            .mock("POST", "/")
            .match_body(Matcher::Regex(format!(
                r#""method"\s*:\s*"getstate".*"params"\s*:\s*\[\s*"{root}".*"{script}".*"{key}".*\]"#,
                root = escape(&root_hash.to_string()),
                script = escape(&script_hash.to_string()),
                key = escape(&key_b64),
            )))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(format!(
                r#"{{"jsonrpc":"2.0","id":1,"result":"{value_b64}"}}"#
            ))
            .create();

        let url = Url::parse(&server.url()).expect("server url");
        let client = RpcClient::builder(url).build().expect("client");
        let api = StateApi::new(Arc::new(client));

        let parsed = api
            .get_state(&root_hash, &script_hash, key)
            .await
            .expect("state value");
        assert_eq!(parsed, value.to_vec());
    }

    #[tokio::test]
    async fn state_api_get_and_verify_proof_round_trip_bytes() {
        if !localhost_binding_permitted() {
            return;
        }
        let mut server = Server::new_async().await;
        let root_hash = UInt256::zero();
        let script_hash = UInt160::zero();
        let key = b"proof-key";
        let proof = b"proof-data";
        let key_b64 = general_purpose::STANDARD.encode(key);
        let proof_b64 = general_purpose::STANDARD.encode(proof);
        let value = b"verified";
        let value_b64 = general_purpose::STANDARD.encode(value);

        let _m_get = server
            .mock("POST", "/")
            .match_body(Matcher::Regex(format!(
                r#""method"\s*:\s*"getproof".*"params"\s*:\s*\[\s*"{root}".*"{script}".*"{key}".*\]"#,
                root = escape(&root_hash.to_string()),
                script = escape(&script_hash.to_string()),
                key = escape(&key_b64),
            )))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(format!(
                r#"{{"jsonrpc":"2.0","id":1,"result":"{proof_b64}"}}"#
            ))
            .create();

        let _m_verify = server
            .mock("POST", "/")
            .match_body(Matcher::Regex(format!(
                r#""method"\s*:\s*"verifyproof".*"params"\s*:\s*\[\s*"{root}".*"{proof}".*\]"#,
                root = escape(&root_hash.to_string()),
                proof = escape(&proof_b64),
            )))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(format!(
                r#"{{"jsonrpc":"2.0","id":1,"result":"{value_b64}"}}"#
            ))
            .create();

        let url = Url::parse(&server.url()).expect("server url");
        let client = RpcClient::builder(url).build().expect("client");
        let api = StateApi::new(Arc::new(client));

        let parsed_proof = api
            .get_proof(&root_hash, &script_hash, key)
            .await
            .expect("proof");
        assert_eq!(parsed_proof, proof.to_vec());

        let parsed_value = api
            .verify_proof(&root_hash, &parsed_proof)
            .await
            .expect("verified value");
        assert_eq!(parsed_value, value.to_vec());
    }

    #[tokio::test]
    async fn state_api_find_states_parses_results_and_proofs() {
        if !localhost_binding_permitted() {
            return;
        }
        let mut server = Server::new_async().await;
        let root_hash = UInt256::zero();
        let script_hash = UInt160::zero();
        let prefix = b"pre";
        let from = b"from";
        let prefix_b64 = general_purpose::STANDARD.encode(prefix);
        let from_b64 = general_purpose::STANDARD.encode(from);

        let response = format!(
            r#"{{"jsonrpc":"2.0","id":1,"result":{{"truncated":false,"results":[{{"key":"{key}","value":"{value}"}}],"firstProof":"{first}","lastProof":"{last}"}}}}"#,
            key = general_purpose::STANDARD.encode(b"k"),
            value = general_purpose::STANDARD.encode(b"v"),
            first = general_purpose::STANDARD.encode(b"first"),
            last = general_purpose::STANDARD.encode(b"last"),
        );

        let _m = server
            .mock("POST", "/")
            .match_body(Matcher::Regex(format!(
                r#""method"\s*:\s*"findstates".*"params"\s*:\s*\[\s*"{root}".*"{script}".*"{prefix}".*"{from}".*2\s*\]"#,
                root = escape(&root_hash.to_string()),
                script = escape(&script_hash.to_string()),
                prefix = escape(&prefix_b64),
                from = escape(&from_b64),
            )))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(response)
            .create();

        let url = Url::parse(&server.url()).expect("server url");
        let client = RpcClient::builder(url).build().expect("client");
        let api = StateApi::new(Arc::new(client));

        let parsed = api
            .find_states(&root_hash, &script_hash, prefix, Some(from), Some(2))
            .await
            .expect("found states");
        assert!(!parsed.truncated);
        assert_eq!(parsed.results.len(), 1);
        assert_eq!(parsed.results[0].0, b"k".to_vec());
        assert_eq!(parsed.results[0].1, b"v".to_vec());
        assert_eq!(parsed.first_proof.as_deref(), Some(b"first".as_slice()));
        assert_eq!(parsed.last_proof.as_deref(), Some(b"last".as_slice()));
    }

    #[test]
    fn state_api_make_find_states_params_handles_defaults() {
        let root_hash = UInt256::zero();
        let script_hash = UInt160::zero();
        let params =
            StateApi::make_find_states_params(&root_hash, &script_hash, b"prefix", None, None);

        assert_eq!(params.len(), 4);
        assert_eq!(params[0].as_string().unwrap(), root_hash.to_string());
        assert_eq!(params[1].as_string().unwrap(), script_hash.to_string());
        assert_eq!(
            params[2].as_string().unwrap(),
            general_purpose::STANDARD.encode(b"prefix")
        );
        assert_eq!(
            params[3].as_string().unwrap(),
            general_purpose::STANDARD.encode(b"")
        );
    }
}
