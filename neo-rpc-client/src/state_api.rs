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

use crate::models::{RpcFoundStates, RpcStateRoot};
use crate::RpcClient;
use base64::{engine::general_purpose, Engine as _};
use neo_core::{UInt160, UInt256};
use neo_json::JToken;
use std::sync::Arc;

/// State service API
/// Matches C# StateAPI
pub struct StateApi {
    /// The RPC client instance
    rpc_client: Arc<RpcClient>,
}

impl StateApi {
    /// StateAPI Constructor
    /// Matches C# constructor
    pub fn new(rpc_client: Arc<RpcClient>) -> Self {
        Self { rpc_client }
    }

    /// Get state root by index
    /// Matches C# GetStateRootAsync
    pub async fn get_state_root(
        &self,
        index: u32,
    ) -> Result<RpcStateRoot, Box<dyn std::error::Error>> {
        let result = self
            .rpc_client
            .rpc_send_async("getstateroot", vec![JToken::Number(index as f64)])
            .await?;

        let obj = result.as_object().ok_or("Invalid response format")?;

        RpcStateRoot::from_json(obj).map_err(|e| e.into())
    }

    /// Get proof for a storage key
    /// Matches C# GetProofAsync
    pub async fn get_proof(
        &self,
        root_hash: &UInt256,
        script_hash: &UInt160,
        key: &[u8],
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
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
            .map_err(|e| e.into())
    }

    /// Verify a proof
    /// Matches C# VerifyProofAsync
    pub async fn verify_proof(
        &self,
        root_hash: &UInt256,
        proof_bytes: &[u8],
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
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
            .map_err(|e| e.into())
    }

    /// Get state height information
    /// Matches C# GetStateHeightAsync
    pub async fn get_state_height(
        &self,
    ) -> Result<(Option<u32>, Option<u32>), Box<dyn std::error::Error>> {
        let result = self
            .rpc_client
            .rpc_send_async("getstateheight", vec![])
            .await?;

        let obj = result.as_object().ok_or("Invalid response format")?;

        let local_root_index = obj
            .get("localrootindex")
            .and_then(|v| v.as_number())
            .map(|n| n as u32);

        let validated_root_index = obj
            .get("validatedrootindex")
            .and_then(|v| v.as_number())
            .map(|n| n as u32);

        Ok((local_root_index, validated_root_index))
    }

    /// Make parameters for find states call
    /// Matches C# MakeFindStatesParams
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
            params.push(JToken::Number(c as f64));
        }

        params
    }

    /// Find states with prefix
    /// Matches C# FindStatesAsync
    pub async fn find_states(
        &self,
        root_hash: &UInt256,
        script_hash: &UInt160,
        prefix: &[u8],
        from: Option<&[u8]>,
        count: Option<i32>,
    ) -> Result<RpcFoundStates, Box<dyn std::error::Error>> {
        let params = Self::make_find_states_params(root_hash, script_hash, prefix, from, count);

        let result = self.rpc_client.rpc_send_async("findstates", params).await?;

        let obj = result.as_object().ok_or("Invalid response format")?;

        RpcFoundStates::from_json(obj).map_err(|e| e.into())
    }

    /// Get state value
    /// Matches C# GetStateAsync
    pub async fn get_state(
        &self,
        root_hash: &UInt256,
        script_hash: &UInt160,
        key: &[u8],
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
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
            .map_err(|e| e.into())
    }
}
