use super::super::ClientRpcError;
use super::super::models::{
    RpcBlock, RpcBlockHeader, RpcContractState, RpcInvokeResult, RpcPeers, RpcRawMemPool,
    RpcValidator, RpcVersion,
};
use super::helpers::{
    parse_object_array_result, token_as_number, token_as_object, token_as_string,
};
use super::hooks::RpcObserver;
use super::{MAX_JSON_NESTING, RpcClient};
use crate::client::utility::cloned_token_array;
use base64::{Engine as _, engine::general_purpose};
use neo_payloads::Signer;
use neo_serialization::json::JToken;
use num_bigint::BigInt;
use std::str::FromStr;

impl<O> RpcClient<O>
where
    O: RpcObserver,
{
    /// Returns the hash of the tallest block in the main chain
    /// Matches C# `GetBestBlockHashAsync`
    pub async fn get_best_block_hash(&self) -> Result<String, ClientRpcError> {
        let result = self.rpc_send_async("getbestblockhash", vec![]).await?;
        token_as_string(result, "getbestblockhash")
    }

    /// Internal helper for sending requests by hash or index
    /// Matches C# `RpcSendByHashOrIndexAsync`
    async fn rpc_send_by_hash_or_index(
        &self,
        rpc_name: &str,
        hash_or_index: &str,
        arguments: Vec<JToken>,
    ) -> Result<JToken, ClientRpcError> {
        let mut params = vec![];

        if let Ok(index) = hash_or_index.trim().parse::<i32>() {
            params.push(JToken::Number(f64::from(index)));
        } else {
            params.push(JToken::String(hash_or_index.to_string()));
        }

        params.extend(arguments);
        self.rpc_send_async(rpc_name, params).await
    }

    /// Invokes a VM script without affecting blockchain state.
    pub async fn invoke_script(&self, script: &[u8]) -> Result<RpcInvokeResult, ClientRpcError> {
        self.invoke_script_with_signers(script, &[]).await
    }

    /// Invokes a VM script with optional signer context.
    pub async fn invoke_script_with_signers(
        &self,
        script: &[u8],
        signers: &[Signer],
    ) -> Result<RpcInvokeResult, ClientRpcError> {
        let mut parameters = Vec::with_capacity(2);
        parameters.push(JToken::String(general_purpose::STANDARD.encode(script)));

        if !signers.is_empty() {
            let mut signer_tokens = Vec::with_capacity(signers.len());
            for signer in signers {
                let serialized = serde_json::to_string(signer).map_err(|err| {
                    ClientRpcError::new(
                        -32603,
                        format!("Failed to serialize signer for invokescript: {err}"),
                    )
                })?;
                let token = JToken::parse(&serialized, MAX_JSON_NESTING).map_err(|err| {
                    ClientRpcError::new(
                        -32603,
                        format!("Failed to parse signer for invokescript: {err}"),
                    )
                })?;
                signer_tokens.push(token);
            }
            parameters.push(cloned_token_array(&signer_tokens));
        }

        let result = self.rpc_send_async("invokescript", parameters).await?;
        let obj = token_as_object(result, "invokescript")?;
        RpcInvokeResult::from_json(&obj).map_err(|err| ClientRpcError::new(-32603, err.to_string()))
    }

    /// Invokes a function on a contract.
    pub async fn invoke_function(
        &self,
        contract_hash: &str,
        operation: &str,
        params: &[JToken],
    ) -> Result<RpcInvokeResult, ClientRpcError> {
        let result = self
            .rpc_send_async(
                "invokefunction",
                vec![
                    JToken::String(contract_hash.to_string()),
                    JToken::String(operation.to_string()),
                    cloned_token_array(params),
                ],
            )
            .await?;
        let obj = token_as_object(result, "invokefunction")?;
        RpcInvokeResult::from_json(&obj).map_err(|err| ClientRpcError::new(-32603, err.to_string()))
    }

    /// Gets a block by hash or index (verbose).
    ///
    /// This matches the default behaviour of the C# client where `verbose = true`.
    pub async fn get_block(&self, hash_or_index: &str) -> Result<JToken, ClientRpcError> {
        self.get_block_with_verbosity(hash_or_index, true).await
    }

    /// Gets a block by hash or index with explicit verbosity control.
    ///
    /// - `verbose = true` returns a JSON block object
    /// - `verbose = false` returns a raw hex string
    pub async fn get_block_with_verbosity(
        &self,
        hash_or_index: &str,
        verbose: bool,
    ) -> Result<JToken, ClientRpcError> {
        let params = if verbose {
            vec![JToken::Boolean(true)]
        } else {
            Vec::new()
        };
        self.rpc_send_by_hash_or_index("getblock", hash_or_index, params)
            .await
    }

    /// Gets a raw block (hex) by hash or index.
    pub async fn get_block_hex(&self, hash_or_index: &str) -> Result<String, ClientRpcError> {
        let result = self.get_block_with_verbosity(hash_or_index, false).await?;
        token_as_string(result, "getblock")
    }

    /// Gets a block count
    /// Matches C# `GetBlockCountAsync`
    pub async fn get_block_count(&self) -> Result<u32, ClientRpcError> {
        let result = self.rpc_send_async("getblockcount", vec![]).await?;
        token_as_number(result, "getblockcount").map(|n| n as u32)
    }

    /// Gets a block hash by index.
    /// Matches C# `GetBlockHashAsync`
    pub async fn get_block_hash(&self, index: u32) -> Result<String, ClientRpcError> {
        let result = self
            .rpc_send_async("getblockhash", vec![JToken::Number(f64::from(index))])
            .await?;
        token_as_string(result, "getblockhash")
    }

    /// Gets a block header count.
    /// Matches C# `GetBlockHeaderCountAsync`
    pub async fn get_block_header_count(&self) -> Result<u32, ClientRpcError> {
        let result = self.rpc_send_async("getblockheadercount", vec![]).await?;
        token_as_number(result, "getblockheadercount").map(|n| n as u32)
    }

    /// Gets the system fee amount for a block.
    pub async fn get_block_sys_fee(&self, height: u32) -> Result<BigInt, ClientRpcError> {
        let result = self
            .rpc_send_async("getblocksysfee", vec![JToken::Number(f64::from(height))])
            .await?;
        match result {
            JToken::String(value) => BigInt::from_str(&value)
                .map_err(|_| ClientRpcError::new(-32603, format!("Invalid sysfee value: {value}"))),
            JToken::Number(value) => Ok(BigInt::from(value as i64)),
            _ => Err(ClientRpcError::new(
                -32603,
                "getblocksysfee returned invalid token",
            )),
        }
    }

    /// Gets a block by hash or index (verbose)
    pub async fn get_block_verbose(&self, hash_or_index: &str) -> Result<RpcBlock, ClientRpcError> {
        let result = self.get_block(hash_or_index).await?;
        let obj = token_as_object(result, "getblock")?;
        RpcBlock::from_json(&obj, &self.protocol_settings)
            .map_err(|err| ClientRpcError::new(-32603, err.to_string()))
    }

    /// Gets a block header by hash or index (verbose).
    pub async fn get_block_header(&self, hash_or_index: &str) -> Result<JToken, ClientRpcError> {
        self.rpc_send_by_hash_or_index("getblockheader", hash_or_index, vec![JToken::Boolean(true)])
            .await
    }

    /// Gets a raw block header (hex) by hash or index.
    pub async fn get_block_header_hex(
        &self,
        hash_or_index: &str,
    ) -> Result<String, ClientRpcError> {
        let result = self
            .rpc_send_by_hash_or_index("getblockheader", hash_or_index, Vec::new())
            .await?;
        token_as_string(result, "getblockheader")
    }

    /// Gets a block header by hash or index (verbose)
    pub async fn get_block_header_verbose(
        &self,
        hash_or_index: &str,
    ) -> Result<RpcBlockHeader, ClientRpcError> {
        let result = self.get_block_header(hash_or_index).await?;
        let obj = token_as_object(result, "getblockheader")?;
        RpcBlockHeader::from_json(&obj, &self.protocol_settings)
            .map_err(|err| ClientRpcError::new(-32603, err.to_string()))
    }

    /// Obtains the number of connections for the node.
    /// Matches C# `GetConnectionCountAsync`
    pub async fn get_connection_count(&self) -> Result<u32, ClientRpcError> {
        let result = self.rpc_send_async("getconnectioncount", vec![]).await?;
        token_as_number(result, "getconnectioncount").map(|n| n as u32)
    }

    /// Returns the currently connected peers.
    /// Matches C# `GetPeersAsync`
    pub async fn get_peers(&self) -> Result<RpcPeers, ClientRpcError> {
        let result = self.rpc_send_async("getpeers", vec![]).await?;
        let obj = token_as_object(result, "getpeers")?;
        RpcPeers::from_json(&obj).map_err(|err| ClientRpcError::new(-32603, err.to_string()))
    }

    /// Returns the node version details.
    /// Matches C# `GetVersionAsync`
    pub async fn get_version(&self) -> Result<RpcVersion, ClientRpcError> {
        let result = self.rpc_send_async("getversion", vec![]).await?;
        let obj = token_as_object(result, "getversion")?;
        RpcVersion::from_json(&obj).map_err(|err| ClientRpcError::new(-32603, err.to_string()))
    }

    /// Returns the current committee members.
    /// Matches C# `GetCommitteeAsync`
    pub async fn get_committee(&self) -> Result<Vec<String>, ClientRpcError> {
        let result = self.rpc_send_async("getcommittee", vec![]).await?;
        let array = result
            .as_array()
            .ok_or_else(|| ClientRpcError::new(-32603, "getcommittee returned non-array"))?;
        let mut members = Vec::with_capacity(array.len());
        for item in array.iter() {
            let token = item
                .as_ref()
                .ok_or_else(|| ClientRpcError::new(-32603, "getcommittee returned null entry"))?;
            let key = token
                .as_string()
                .ok_or_else(|| ClientRpcError::new(-32603, "getcommittee returned non-string"))?;
            members.push(key);
        }
        Ok(members)
    }

    /// Returns the next block validators.
    /// Matches C# `GetNextBlockValidatorsAsync`
    pub async fn get_next_block_validators(&self) -> Result<Vec<RpcValidator>, ClientRpcError> {
        let result = self
            .rpc_send_async("getnextblockvalidators", vec![])
            .await?;
        parse_object_array_result(
            &result,
            "getnextblockvalidators returned non-array",
            "getnextblockvalidators returned null entry",
            "getnextblockvalidators returned non-object",
            RpcValidator::from_json,
        )
    }

    /// Gets a storage item by contract hash and key.
    /// Matches C# `GetStorageAsync`
    pub async fn get_storage(&self, hash: &str, key: &str) -> Result<String, ClientRpcError> {
        let result = self
            .rpc_send_by_hash_or_index("getstorage", hash, vec![JToken::String(key.to_string())])
            .await?;
        token_as_string(result, "getstorage")
    }

    /// Gets a storage item by numeric contract ID and key.
    pub async fn get_storage_by_id(&self, id: i32, key: &str) -> Result<String, ClientRpcError> {
        let result = self
            .rpc_send_async(
                "getstorage",
                vec![
                    JToken::Number(f64::from(id)),
                    JToken::String(key.to_string()),
                ],
            )
            .await?;
        token_as_string(result, "getstorage")
    }

    /// Returns the block index in which the transaction is found.
    /// Matches C# `GetTransactionHeightAsync`
    pub async fn get_transaction_height(&self, tx_hash: &str) -> Result<u32, ClientRpcError> {
        let result = self
            .rpc_send_async(
                "gettransactionheight",
                vec![JToken::String(tx_hash.to_string())],
            )
            .await?;
        let value = token_as_string(result, "gettransactionheight")?;
        value
            .parse::<u32>()
            .map_err(|_| ClientRpcError::new(-32603, format!("Invalid height value: {value}")))
    }

    /// Returns the list of native contracts.
    /// Matches C# `GetNativeContractsAsync`
    pub async fn get_native_contracts(&self) -> Result<Vec<RpcContractState>, ClientRpcError> {
        let result = self.rpc_send_async("getnativecontracts", vec![]).await?;
        parse_object_array_result(
            &result,
            "getnativecontracts returned non-array",
            "getnativecontracts returned null entry",
            "getnativecontracts returned non-object",
            RpcContractState::from_json,
        )
    }

    /// Obtains the list of unconfirmed transactions in memory.
    /// Matches C# `GetRawMempoolAsync`
    pub async fn get_raw_mempool(&self) -> Result<Vec<String>, ClientRpcError> {
        let result = self.rpc_send_async("getrawmempool", vec![]).await?;
        let array = result
            .as_array()
            .ok_or_else(|| ClientRpcError::new(-32603, "getrawmempool returned non-array"))?;
        let mut hashes = Vec::with_capacity(array.len());
        for item in array.iter() {
            let token = item
                .as_ref()
                .ok_or_else(|| ClientRpcError::new(-32603, "getrawmempool returned null entry"))?;
            let hash = token.as_string().ok_or_else(|| {
                ClientRpcError::new(-32603, "getrawmempool returned non-string entry")
            })?;
            hashes.push(hash);
        }
        Ok(hashes)
    }

    /// Obtains the list of unconfirmed transactions in memory (verified + unverified).
    /// Matches C# `GetRawMempoolBothAsync`
    pub async fn get_raw_mempool_both(&self) -> Result<RpcRawMemPool, ClientRpcError> {
        let result = self
            .rpc_send_async("getrawmempool", vec![JToken::Boolean(true)])
            .await?;
        let obj = token_as_object(result, "getrawmempool")?;
        RpcRawMemPool::from_json(&obj).map_err(|err| ClientRpcError::new(-32603, err.to_string()))
    }
}
