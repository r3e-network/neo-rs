//! # neo-rpc::server::rpc_server_state
//!
//! State-service RPC endpoint handlers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `tests`: Module-local tests and regression coverage.

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::{expect_base64_param_with_message, expect_u32_param_with_message};
use crate::server::rpc_server::{RpcHandler, RpcServer};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use neo_crypto::mpt_trie::{MptError, MptResult, MptStoreSnapshot, Trie};
use neo_execution::contract_state::ContractState;
use neo_io::MemoryReader;
use neo_primitives::{UInt160, UInt256};
use neo_state_service::mpt_store::{MptReadSnapshot, MptStore};
use neo_state_service::state_store::StateStoreLookup;
use neo_state_service::{StateRoot, StateStore};
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use std::sync::Arc;

/// Upper bound on a proof storage key (mirrors C#
/// `StateService.MaxKeyLength`: 64 key bytes + the i32 contract-id
/// prefix).
const MAX_PROOF_KEY_LENGTH: usize = 64 + std::mem::size_of::<i32>();

/// Upper bound on a single proof node (an MPT node never exceeds the
/// 1 KiB C# `Node.MaxLength` by far; allow ample slack).
const MAX_PROOF_NODE_LENGTH: usize = 4096;

/// `ContractManagement::ID` (C# `NativeContract.ContractManagement.Id`).
const CONTRACT_MANAGEMENT_ID: i32 = -1;

/// `ContractManagement.Prefix_Contract` — the per-contract record
/// prefix the C# `StatePlugin.GetHistoricalContractState` reads
/// (`const byte prefix = 8`).
const PREFIX_CONTRACT: u8 = 8;

/// C# `StateServiceSettings.MaxFindResultItems` default (the plugin
/// caps every `findstates` page at this many results).
const MAX_FIND_RESULT_ITEMS: usize = 100;

/// Zero-state snapshot used purely to pin `Trie`'s type parameter for
/// the associated [`Trie::verify_proof`] call (which builds its own
/// internal proof store and never touches the parameter type).
struct ProofVerifySnapshot;

impl MptStoreSnapshot for ProofVerifySnapshot {
    fn try_get(&self, _key: &[u8]) -> MptResult<Option<Vec<u8>>> {
        Ok(None)
    }

    fn put(&self, _key: Vec<u8>, _value: Vec<u8>) -> MptResult<()> {
        Ok(())
    }

    fn delete(&self, _key: Vec<u8>) -> MptResult<()> {
        Ok(())
    }
}

/// RPC handler group for StateService methods.
pub struct RpcServerState;

impl RpcServerState {
    /// Register StateService RPC handlers.
    pub fn register_handlers() -> Vec<RpcHandler> {
        super::rpc_handlers![
            "getstateheight" => Self::get_state_height,
            "getstateroot" => Self::get_state_root,
            "getproof" => Self::get_proof,
            "verifyproof" => Self::verify_proof,
            "getstate" => Self::get_state,
            "findstates" => Self::find_states,
        ]
    }

    fn state_store(server: &RpcServer) -> Result<Arc<StateStore>, RpcException> {
        server.system().state_store().ok_or_else(|| {
            RpcException::from(
                RpcError::internal_server_error().with_data("StateService service not registered"),
            )
        })
    }

    /// Resolves the persisted MPT backend, or reports the same
    /// `UnsupportedState` error the MPT-less build always served.
    fn mpt_store(server: &RpcServer) -> Result<Arc<MptStore>, RpcException> {
        let state_store = Self::state_store(server)?;
        state_store.mpt().ok_or_else(Self::proofs_unsupported)
    }

    fn get_state_height(server: &RpcServer, _params: &[Value]) -> Result<Value, RpcException> {
        let state_store = Self::state_store(server)?;
        // The state-root cache records roots once they are validated, so the
        // local and validated indexes coincide in this build. The verification
        // StateStore is only populated when the (currently dormant) state-root
        // verification pipeline runs; fall back to the live MptStore, which is
        // written by the block-apply pipeline, so a running node reports a real
        // height instead of null.
        let index = state_store
            .current_local_index()
            .or_else(|| {
                Self::mpt_store(server)
                    .ok()
                    .and_then(|mpt| mpt.current_local_root_index())
            })
            .map_or(Value::Null, |index| json!(index));
        Ok(json!({
            "localrootindex": index,
            "validatedrootindex": index}))
    }

    fn get_state_root(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let index = Self::expect_u32(params, 0, "getstateroot")?;
        let state_store = Self::state_store(server)?;
        let state_root = state_store
            .get_state_root(StateStoreLookup::ByBlockIndex(index))
            .or_else(|| {
                // Fall back to the live MptStore (written by apply_block_changes)
                // when the verification StateStore cache is empty.
                Self::mpt_store(server)
                    .ok()
                    .and_then(|mpt| mpt.get_state_root(index))
            })
            .ok_or_else(|| RpcException::from(RpcError::unknown_state_root()))?;
        Ok(Self::state_root_to_json(&state_root))
    }

    /// `getproof(roothash, scripthash, key)` — C#
    /// `StatePlugin.GetProof`: resolves the contract id under the
    /// requested root, builds the Merkle proof for the storage key and
    /// returns the Base64 proof payload (`VarBytes(storage_key)` +
    /// `VarInt(count)` + `VarBytes` per node).
    fn get_proof(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let mpt = Self::mpt_store(server)?;
        let root_hash = Self::parse_uint256(params, 0, "getproof")?;
        let script_hash = Self::parse_uint160(params, 1, "getproof")?;
        let key = Self::parse_base64(params, 2, "getproof", "Base64 storage key")?;

        // One frozen view per request (C# `GetStoreSnapshot()`): the
        // root gate and the trie walk read the same generation, so a
        // concurrent block commit cannot prune nodes mid-walk.
        let snapshot = mpt.snapshot();
        Self::check_root_hash(&snapshot, &root_hash)?;
        let mut trie = snapshot.open_trie(Some(root_hash));
        let contract_id = Self::historical_contract_id(&mut trie, &script_hash)?;
        let payload = Self::proof_payload(&mut trie, contract_id, &key)?;
        Ok(Value::String(payload))
    }

    fn verify_proof(_server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let root_hash = Self::parse_uint256(params, 0, "verifyproof")?;
        let proof_bytes = Self::parse_base64(params, 1, "verifyproof", "Base64 proof payload")?;
        let (key, nodes) = Self::decode_proof_payload(&proof_bytes).ok_or_else(|| {
            RpcException::from(RpcError::invalid_params().with_data("invalid proof payload"))
        })?;
        let proof: HashSet<Vec<u8>> = nodes.into_iter().collect();
        let value =
            Trie::<ProofVerifySnapshot>::verify_proof(root_hash, &key, &proof).map_err(|_| {
                RpcException::from(
                    RpcError::verification_failed()
                        .with_data("failed to verify state proof against supplied root"),
                )
            })?;
        Ok(Value::String(BASE64_STANDARD.encode(value)))
    }

    /// `getstate(roothash, scripthash, key)` — C# `StatePlugin.GetState`:
    /// resolves the value stored under the historical root and returns
    /// it Base64-encoded.
    ///
    /// C# reads the value through the trie indexer, whose
    /// `KeyNotFoundException` escapes to the generic RPC handler as a
    /// raw .NET `HResult` custom error; this port reports the named
    /// `UnknownStorageItem` (-104) instead — the code the C# plugin
    /// itself uses for the identical condition in `getproof`.
    fn get_state(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let mpt = Self::mpt_store(server)?;
        let root_hash = Self::parse_uint256(params, 0, "getstate")?;
        let script_hash = Self::parse_uint160(params, 1, "getstate")?;
        let key = Self::parse_base64(params, 2, "getstate", "Base64 storage key")?;

        let snapshot = mpt.snapshot();
        Self::check_root_hash(&snapshot, &root_hash)?;
        let mut trie = snapshot.open_trie(Some(root_hash));
        let contract_id = Self::historical_contract_id(&mut trie, &script_hash)?;
        let storage_key = Self::storage_key_bytes(contract_id, &key);
        let value = trie
            .try_get_value(&storage_key)
            .map_err(|err| Self::trie_lookup_error("getstate", &err))?
            .ok_or_else(|| RpcException::from(RpcError::unknown_storage_item()))?;
        Ok(Value::String(BASE64_STANDARD.encode(value)))
    }

    /// `findstates(roothash, scripthash, prefix, [key], [count])` — C#
    /// `StatePlugin.FindStates`: enumerates storage entries under
    /// `prefix`, resuming strictly after the optional `key`, capped at
    /// `count` (default and maximum
    /// [`MAX_FIND_RESULT_ITEMS`]). The response carries the page,
    /// `truncated`, and Merkle proofs for the first (and, when the
    /// page has more than one entry, last) returned key.
    fn find_states(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let mpt = Self::mpt_store(server)?;
        let root_hash = Self::parse_uint256(params, 0, "findstates")?;
        let script_hash = Self::parse_uint160(params, 1, "findstates")?;
        let prefix = Self::parse_base64(params, 2, "findstates", "Base64 key prefix")?;
        let from_key = Self::parse_optional_base64(params, 3, "findstates", "Base64 from-key")?;
        let count = Self::parse_find_count(params, 4)?;

        let snapshot = mpt.snapshot();
        Self::check_root_hash(&snapshot, &root_hash)?;
        let mut trie = snapshot.open_trie(Some(root_hash));
        let contract_id = Self::historical_contract_id(&mut trie, &script_hash)?;

        let prefix_key = Self::storage_key_bytes(contract_id, &prefix);
        let from_storage_key = from_key
            .as_ref()
            .filter(|from| !from.is_empty())
            .map(|from| Self::storage_key_bytes(contract_id, from));

        // C# consumes the lazy `Trie.Find` enumerator and breaks once
        // it has seen `count` results plus one probe entry, so the
        // trie traversal never materializes the whole prefix range.
        // `find_limited` is that early break: request exactly
        // `count + 1` entries — the probe's only job is to tell us
        // whether the page is truncated.
        let mut entries = trie
            .find_limited(&prefix_key, from_storage_key.as_deref(), count + 1)
            .map_err(Self::find_error)?;
        let truncated = entries.len() > count;
        entries.truncate(count);
        let results: Vec<(Vec<u8>, Vec<u8>)> = entries
            .into_iter()
            .map(|entry| {
                let key_suffix = entry.key.get(std::mem::size_of::<i32>()..).unwrap_or(&[]);
                (key_suffix.to_vec(), entry.value)
            })
            .collect();

        let mut response = Map::new();
        if let Some((first_key, _)) = results.first() {
            response.insert(
                "firstProof".to_string(),
                Value::String(Self::proof_payload(&mut trie, contract_id, first_key)?),
            );
        }
        if results.len() > 1 {
            if let Some((last_key, _)) = results.last() {
                response.insert(
                    "lastProof".to_string(),
                    Value::String(Self::proof_payload(&mut trie, contract_id, last_key)?),
                );
            }
        }
        response.insert("truncated".to_string(), Value::Bool(truncated));
        response.insert(
            "results".to_string(),
            Value::Array(
                results
                    .into_iter()
                    .map(|(key, value)| {
                        json!({
                            "key": BASE64_STANDARD.encode(key),
                            "value": BASE64_STANDARD.encode(value),
                        })
                    })
                    .collect(),
            ),
        );
        Ok(Value::Object(response))
    }

    /// C# `StatePlugin.CheckRootHash`: without `FullState`, only the
    /// current local root may be queried (`UnsupportedState`
    /// otherwise, with the same diagnostic data string — C#
    /// interpolates `bool.ToString()`, so the flag reads
    /// `True`/`False`).
    ///
    /// The check runs against the request's own store snapshot (C#
    /// reads the live `CurrentLocalRootHash` just before opening the
    /// snapshot; gating on the snapshot's value closes that race
    /// window without changing the accepted set).
    fn check_root_hash(
        snapshot: &MptReadSnapshot,
        root_hash: &UInt256,
    ) -> Result<(), RpcException> {
        let full_state = snapshot.full_state();
        let current = snapshot.current_local_root_hash();
        if !full_state && current.as_ref() != Some(root_hash) {
            let full_state_text = if full_state { "True" } else { "False" };
            let current_text = current.map(|hash| hash.to_string()).unwrap_or_default();
            return Err(RpcException::from(RpcError::unsupported_state().with_data(
                format!("fullState:{full_state_text},current:{current_text},rootHash:{root_hash}"),
            )));
        }
        Ok(())
    }

    /// C# `StatePlugin.GetHistoricalContractState`: reads the
    /// `ContractManagement` per-contract record
    /// (`KeyBuilder(-1, 8).Add(scriptHash)`) out of the historical
    /// trie and decodes the interoperable `ContractState` to obtain
    /// the contract id (`UnknownContract` when absent).
    fn historical_contract_id(
        trie: &mut Trie<MptReadSnapshot>,
        script_hash: &UInt160,
    ) -> Result<i32, RpcException> {
        let mut key = Vec::with_capacity(std::mem::size_of::<i32>() + 1 + UInt160::LENGTH);
        key.extend_from_slice(&CONTRACT_MANAGEMENT_ID.to_le_bytes());
        key.push(PREFIX_CONTRACT);
        key.extend_from_slice(&script_hash.to_bytes());

        let record = trie
            .try_get_value(&key)
            .map_err(|err| Self::trie_lookup_error("contract lookup", &err))?
            .ok_or_else(|| RpcException::from(RpcError::unknown_contract()))?;
        let contract = ContractState::deserialize_contract_record(&record).map_err(|err| {
            RpcException::from(
                RpcError::internal_server_error()
                    .with_data(format!("malformed contract record in state trie: {err}")),
            )
        })?;
        Ok(contract.id)
    }

    /// C# `StatePlugin.GetProof(Trie, int, byte[])`: builds the proof
    /// for `(contract_id, key)` and serializes the payload
    /// (`UnknownStorageItem` when the key is not in the trie).
    fn proof_payload(
        trie: &mut Trie<MptReadSnapshot>,
        contract_id: i32,
        key: &[u8],
    ) -> Result<String, RpcException> {
        let storage_key = Self::storage_key_bytes(contract_id, key);
        let proof = trie
            .try_get_proof(&storage_key)
            .map_err(|err| Self::trie_lookup_error("proof query", &err))?
            .ok_or_else(|| RpcException::from(RpcError::unknown_storage_item()))?;
        let nodes: Vec<Vec<u8>> = proof.into_iter().collect();
        Ok(BASE64_STANDARD.encode(Self::encode_proof_payload(&storage_key, &nodes)))
    }

    /// Serializes `(contract_id, key)` as the C# `StorageKey.ToArray()`
    /// bytes the state trie is keyed by: little-endian `i32` id + key.
    fn storage_key_bytes(contract_id: i32, key: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(std::mem::size_of::<i32>() + key.len());
        bytes.extend_from_slice(&contract_id.to_le_bytes());
        bytes.extend_from_slice(key);
        bytes
    }

    /// Maps a trie resolution failure (e.g. a root hash this store has
    /// never persisted) to an internal error.
    ///
    /// C# surfaces the same condition as an uncaught
    /// `InvalidOperationException` converted to a raw `HResult` custom
    /// error; a named internal error with the MPT message is this
    /// port's equivalent.
    fn trie_lookup_error(context: &str, err: &MptError) -> RpcException {
        RpcException::from(
            RpcError::internal_server_error()
                .with_data(format!("{context}: MPT resolution failed: {err}")),
        )
    }

    /// Maps `Trie::find` argument failures (`from` not under the
    /// prefix, oversized keys) to `InvalidParams`; anything else is a
    /// resolution failure.
    fn find_error(err: MptError) -> RpcException {
        match err {
            MptError::InvalidOperation(message) | MptError::Key(message) => {
                RpcException::from(RpcError::invalid_params().with_data(message))
            }
            other => Self::trie_lookup_error("findstates", &other),
        }
    }

    /// The state-root cache does not persist the MPT trie, so queries
    /// that must walk historical tries cannot be answered.
    fn proofs_unsupported() -> RpcException {
        RpcException::from(RpcError::unsupported_state().with_data(
            "the state service in this build records validated state roots only and does not \
             persist the MPT trie required for state/proof queries",
        ))
    }

    /// Decodes the C# StateService proof payload: `VarBytes(key)` then
    /// `VarInt(count)` proof nodes, each `VarBytes`.
    fn decode_proof_payload(bytes: &[u8]) -> Option<(Vec<u8>, Vec<Vec<u8>>)> {
        let mut reader = MemoryReader::new(bytes);
        let key = reader.read_var_bytes(MAX_PROOF_KEY_LENGTH).ok()?;
        let count = reader.read_var_int(u32::MAX as u64).ok()?;
        let mut nodes = Vec::with_capacity(usize::try_from(count).ok()?);
        for _ in 0..count {
            nodes.push(reader.read_var_bytes(MAX_PROOF_NODE_LENGTH).ok()?);
        }
        Some((key, nodes))
    }

    /// Encodes the C# StateService proof payload (the inverse of
    /// [`Self::decode_proof_payload`]): `WriteVarBytes(storage_key)`,
    /// `WriteVarInt(count)`, then each node as `VarBytes` — the exact
    /// layout `StatePlugin.GetProof` emits.
    fn encode_proof_payload(key: &[u8], nodes: &[Vec<u8>]) -> Vec<u8> {
        let mut writer = neo_io::BinaryWriter::new();
        let _ = writer.write_var_bytes(key);
        let _ = writer.write_var_int(nodes.len() as u64);
        for node in nodes {
            let _ = writer.write_var_bytes(node);
        }
        writer.into_bytes()
    }

    fn parse_uint256(params: &[Value], idx: usize, method: &str) -> Result<UInt256, RpcException> {
        Self::parse_uint_parameter(params, idx, method, "UInt256", UInt256::parse)
    }

    fn parse_uint160(params: &[Value], idx: usize, method: &str) -> Result<UInt160, RpcException> {
        Self::parse_uint_parameter(params, idx, method, "UInt160", UInt160::parse)
    }

    fn parse_uint_parameter<T, E>(
        params: &[Value],
        idx: usize,
        method: &str,
        type_name: &str,
        parse: impl FnOnce(&str) -> Result<T, E>,
    ) -> Result<T, RpcException> {
        let value = params.get(idx).and_then(Value::as_str).ok_or_else(|| {
            RpcException::from(RpcError::invalid_params().with_data(format!(
                "{method} expects {type_name} parameter at index {idx}"
            )))
        })?;
        parse(value).map_err(|_| {
            RpcException::from(
                RpcError::invalid_params()
                    .with_data(format!("failed to parse {type_name} parameter")),
            )
        })
    }

    fn parse_base64(
        params: &[Value],
        idx: usize,
        method: &str,
        descriptor: &str,
    ) -> Result<Vec<u8>, RpcException> {
        expect_base64_param_with_message(
            params,
            idx,
            format!("{method} expects {descriptor} at index {idx}"),
        )
    }

    /// Parses an optional Base64 parameter: absent or `null` maps to
    /// `None` (the C# binder's `byte[] key = null` default).
    fn parse_optional_base64(
        params: &[Value],
        idx: usize,
        method: &str,
        descriptor: &str,
    ) -> Result<Option<Vec<u8>>, RpcException> {
        match params.get(idx) {
            None | Some(Value::Null) => Ok(None),
            Some(_) => Self::parse_base64(params, idx, method, descriptor).map(Some),
        }
    }

    /// Parses the optional `findstates` count with the C# binder's
    /// accepting behaviour: absent or `null` falls back to the C#
    /// parameter default (`int count = 0`, i.e. `MaxFindResultItems`);
    /// present tokens go through the `ParameterConverter.ToNumeric<int>`
    /// conversion; non-positive results select the default page size
    /// and explicit values are capped at [`MAX_FIND_RESULT_ITEMS`].
    fn parse_find_count(params: &[Value], idx: usize) -> Result<usize, RpcException> {
        let requested = match params.get(idx) {
            None | Some(Value::Null) => 0i32,
            Some(value) => Self::to_numeric_i32(value)?,
        };
        if requested <= 0 {
            return Ok(MAX_FIND_RESULT_ITEMS);
        }
        Ok((requested as usize).min(MAX_FIND_RESULT_ITEMS))
    }

    /// C# `ParameterConverter.ToNumeric<int>` (ParameterConverter.cs):
    /// funnels the token through `JToken.AsNumber()` and requires the
    /// result to be an integral value within `i32` range — so JSON
    /// strings ("2"), float-integral numbers (2.0), and booleans are
    /// accepted exactly as the C# binder accepts them, while
    /// fractional, out-of-range, and non-numeric tokens are rejected
    /// with the C# `InvalidParams` data string.
    fn to_numeric_i32(value: &Value) -> Result<i32, RpcException> {
        let number = Self::token_as_number(value);
        // C# checks the `int` range first and then
        // `IsValidInteger` (an exact integral remainder; NaN fails
        // it). Infinity fails the range check; both reject the same
        // way here.
        if !(number >= f64::from(i32::MIN) && number <= f64::from(i32::MAX)) || number % 1.0 != 0.0
        {
            return Err(RpcException::from(
                RpcError::invalid_params()
                    .with_data(format!("Invalid System.Int32 value: {value}")),
            ));
        }
        Ok(number as i32)
    }

    /// Neo.Json `JToken.AsNumber()`: numbers pass through, strings
    /// parse as invariant floating-point text (`JString.AsNumber` —
    /// the empty string is `0`, surrounding whitespace is allowed,
    /// unparseable text is NaN), booleans map to `1`/`0`
    /// (`JBoolean.AsNumber`), and every other token is NaN
    /// (`JToken.AsNumber` base).
    fn token_as_number(value: &Value) -> f64 {
        match value {
            Value::Number(number) => number.as_f64().unwrap_or(f64::NAN),
            Value::String(text) => {
                if text.is_empty() {
                    return 0.0;
                }
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    f64::NAN
                } else {
                    trimmed.parse::<f64>().unwrap_or(f64::NAN)
                }
            }
            Value::Bool(flag) => {
                if *flag {
                    1.0
                } else {
                    0.0
                }
            }
            _ => f64::NAN,
        }
    }

    fn expect_u32(params: &[Value], idx: usize, method: &str) -> Result<u32, RpcException> {
        expect_u32_param_with_message(
            params,
            idx,
            format!("{method} expects unsigned integer parameter"),
        )
    }

    fn state_root_to_json(root: &StateRoot) -> Value {
        let mut obj = Map::new();
        obj.insert("version".to_string(), json!(root.version));
        obj.insert("index".to_string(), json!(root.index));
        obj.insert(
            "roothash".to_string(),
            Value::String(root.root_hash.to_string()),
        );
        // C# `StateRoot.ToJson`: `witnesses = [] | [Witness.ToJson()]`, where a
        // witness is `{ invocation: base64, verification: base64 }`. Emitted when
        // the root is signed (an aggregated StateValidators witness).
        let witnesses = match root.witness() {
            None => Vec::new(),
            Some(witness) => {
                let mut w = Map::new();
                w.insert(
                    "invocation".to_string(),
                    Value::String(BASE64_STANDARD.encode(&witness.invocation_script)),
                );
                w.insert(
                    "verification".to_string(),
                    Value::String(BASE64_STANDARD.encode(&witness.verification_script)),
                );
                vec![Value::Object(w)]
            }
        };
        obj.insert("witnesses".to_string(), Value::Array(witnesses));
        Value::Object(obj)
    }
}

#[cfg(test)]
#[path = "../../tests/server/handlers/rpc_server_state.rs"]
mod tests;
