//! StateService proof RPC handlers and proof payload codec.
//!
//! `getproof` and `verifyproof` share the C# StateService proof payload
//! format. Keeping that logic here leaves the root state module focused on
//! handler registration while support and query modules own runtime mechanics.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use neo_io::MemoryReader;
use neo_state_service::{StateProof, StateProviderFactory, StateView, verify_state_proof};
use serde_json::Value;

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_server::RpcServer;

use super::RpcServerState;
use super::request::{StateKeyRequest, VerifyProofRequest};
use super::response::{base64_state_value_to_json, proof_payload_to_json};

/// Upper bound on a proof storage key (mirrors C#
/// `StateService.MaxKeyLength`: 64 key bytes + the i32 contract-id
/// prefix).
const MAX_PROOF_KEY_LENGTH: usize = 64 + std::mem::size_of::<i32>();

/// Upper bound on a single proof node (an MPT node never exceeds the
/// 1 KiB C# `Node.MaxLength` by far; allow ample slack).
const MAX_PROOF_NODE_LENGTH: usize = 4096;

impl RpcServerState {
    /// `getproof(roothash, scripthash, key)` — C#
    /// `StatePlugin.GetProof`: resolves the contract id under the
    /// requested root, builds the Merkle proof for the storage key and
    /// returns the Base64 proof payload (`VarBytes(storage_key)` +
    /// `VarInt(count)` + `VarBytes` per node).
    pub(super) fn get_proof(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let factory = Self::state_provider_factory(server)?;
        let request = StateKeyRequest::parse_get_proof(params)?;

        // The factory binds the pruning-mode gate and all lookups to one frozen
        // generation, so a concurrent commit cannot prune nodes mid-request.
        let mut state = factory
            .state_by_root(request.root_hash)
            .map_err(|error| Self::state_provider_error("getproof", error))?;
        let contract_id = Self::historical_contract_id(&mut state, &request.script_hash)?;
        let payload = Self::proof_payload(&mut state, contract_id, &request.key)?;
        Ok(proof_payload_to_json(payload))
    }

    pub(super) fn verify_proof(
        _server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let request = VerifyProofRequest::parse(params)?;
        let (key, nodes) = Self::decode_proof_payload(&request.proof_bytes).ok_or_else(|| {
            RpcException::from(RpcError::invalid_params().with_data("invalid proof payload"))
        })?;
        let proof: StateProof = nodes.into_iter().collect();
        let value = verify_state_proof(request.root_hash, &key, &proof).map_err(|_| {
            RpcException::from(
                RpcError::verification_failed()
                    .with_data("failed to verify state proof against supplied root"),
            )
        })?;
        Ok(base64_state_value_to_json(&value))
    }

    /// C# `StatePlugin.GetProof(Trie, int, byte[])`: builds the proof
    /// for `(contract_id, key)` and serializes the payload
    /// (`UnknownStorageItem` when the key is not in the trie).
    pub(super) fn proof_payload<V>(
        state: &mut V,
        contract_id: i32,
        key: &[u8],
    ) -> Result<String, RpcException>
    where
        V: StateView,
    {
        let storage_key = Self::storage_key_bytes(contract_id, key);
        let proof = state
            .proof(&storage_key)
            .map_err(|error| Self::state_provider_error("proof query", error))?
            .ok_or_else(|| RpcException::from(RpcError::unknown_storage_item()))?;
        let nodes: Vec<Vec<u8>> = proof.into_iter().collect();
        Ok(BASE64_STANDARD.encode(Self::encode_proof_payload(&storage_key, &nodes)?))
    }

    /// Decodes the C# StateService proof payload: `VarBytes(key)` then
    /// `VarInt(count)` proof nodes, each `VarBytes`.
    pub(super) fn decode_proof_payload(bytes: &[u8]) -> Option<(Vec<u8>, Vec<Vec<u8>>)> {
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
    ///
    /// Returns `Err` if the underlying `BinaryWriter` fails. The in-memory
    /// `BinaryWriter` cannot fail in practice, but propagating the `?` here
    /// ensures that any future writer change surfaces an internal error
    /// instead of silently producing a truncated/malformed proof payload.
    pub(super) fn encode_proof_payload(
        key: &[u8],
        nodes: &[Vec<u8>],
    ) -> Result<Vec<u8>, RpcException> {
        let mut writer = neo_io::BinaryWriter::new();
        writer
            .write_var_bytes(key)
            .map_err(Self::writer_error_to_rpc)?;
        writer
            .write_var_int(nodes.len() as u64)
            .map_err(Self::writer_error_to_rpc)?;
        for node in nodes {
            writer
                .write_var_bytes(node)
                .map_err(Self::writer_error_to_rpc)?;
        }
        Ok(writer.into_bytes())
    }

    /// Converts a `neo_io::IoError` from the in-memory `BinaryWriter` into
    /// an internal RPC error. The current writer writes to a `Vec<u8>` and
    /// cannot fail, but keeping the conversion in one place lets
    /// `encode_proof_payload` use `?` and stay resilient to a future
    /// streaming writer.
    fn writer_error_to_rpc(err: neo_io::IoError) -> RpcException {
        RpcException::from(
            RpcError::internal_server_error()
                .with_data(format!("proof payload encoding failed: {err}")),
        )
    }
}
