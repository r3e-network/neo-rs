use super::models::{RpcFoundStates, RpcStateRoot};
use super::utility::base64_string_token;
use crate::client::RpcClientError;
use crate::{RpcClient, RpcObserver, TracingRpcObserver};
use base64::{Engine as _, engine::general_purpose};
use neo_primitives::{UInt160, UInt256};
use neo_serialization::json::JToken;
use std::sync::Arc;

/// State service API
/// Matches C# `StateAPI`
pub struct StateApi<O = TracingRpcObserver> {
    /// The RPC client instance
    rpc_client: Arc<RpcClient<O>>,
}

fn decode_base64_rpc_result(result: &JToken) -> Result<Vec<u8>, RpcClientError> {
    let value = result.as_string().ok_or("Invalid response format")?;
    general_purpose::STANDARD
        .decode(value)
        .map_err(std::convert::Into::into)
}

fn make_find_states_params_impl(
    root_hash: &UInt256,
    script_hash: &UInt160,
    prefix: &[u8],
    from: Option<&[u8]>,
    count: Option<i32>,
) -> Vec<JToken> {
    let mut params = vec![
        JToken::String(root_hash.to_string()),
        JToken::String(script_hash.to_string()),
        base64_string_token(prefix),
        base64_string_token(from.unwrap_or(&[])),
    ];

    if let Some(c) = count {
        params.push(JToken::Number(f64::from(c)));
    }

    params
}

impl StateApi<TracingRpcObserver> {
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
        make_find_states_params_impl(root_hash, script_hash, prefix, from, count)
    }
}

impl<O> StateApi<O>
where
    O: RpcObserver,
{
    /// `StateAPI` Constructor
    /// Matches C# constructor
    #[must_use]
    pub const fn new(rpc_client: Arc<RpcClient<O>>) -> Self {
        Self { rpc_client }
    }

    /// Get state root by index
    /// Matches C# `GetStateRootAsync`
    pub async fn get_state_root(&self, index: u32) -> Result<RpcStateRoot, RpcClientError> {
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
    ) -> Result<Vec<u8>, RpcClientError> {
        let result = self
            .rpc_client
            .rpc_send_async(
                "getproof",
                vec![
                    JToken::String(root_hash.to_string()),
                    JToken::String(script_hash.to_string()),
                    base64_string_token(key),
                ],
            )
            .await?;

        decode_base64_rpc_result(&result)
    }

    /// Verify a proof
    /// Matches C# `VerifyProofAsync`
    pub async fn verify_proof(
        &self,
        root_hash: &UInt256,
        proof_bytes: &[u8],
    ) -> Result<Vec<u8>, RpcClientError> {
        let result = self
            .rpc_client
            .rpc_send_async(
                "verifyproof",
                vec![
                    JToken::String(root_hash.to_string()),
                    base64_string_token(proof_bytes),
                ],
            )
            .await?;

        decode_base64_rpc_result(&result)
    }

    /// Get state height information
    /// Matches C# `GetStateHeightAsync`
    pub async fn get_state_height(&self) -> Result<(Option<u32>, Option<u32>), RpcClientError> {
        let result = self
            .rpc_client
            .rpc_send_async("getstateheight", vec![])
            .await?;

        let obj = result.as_object().ok_or("Invalid response format")?;

        let local_root_index = obj
            .get("localrootindex")
            .and_then(neo_serialization::json::JToken::as_number)
            .map(|n| n as u32);

        let validated_root_index = obj
            .get("validatedrootindex")
            .and_then(neo_serialization::json::JToken::as_number)
            .map(|n| n as u32);

        Ok((local_root_index, validated_root_index))
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
    ) -> Result<RpcFoundStates, RpcClientError> {
        let params = make_find_states_params_impl(root_hash, script_hash, prefix, from, count);

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
    ) -> Result<Vec<u8>, RpcClientError> {
        let result = self
            .rpc_client
            .rpc_send_async(
                "getstate",
                vec![
                    JToken::String(root_hash.to_string()),
                    JToken::String(script_hash.to_string()),
                    base64_string_token(key),
                ],
            )
            .await?;

        decode_base64_rpc_result(&result)
    }
}

#[cfg(test)]
#[path = "../../tests/client/state_api.rs"]
mod tests;
