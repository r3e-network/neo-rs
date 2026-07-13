//! ContractParametersContext - matches C# Neo.SmartContract.ContractParametersContext exactly

use crate::contract::Contract;
use crate::contract_parameter::ContractParameter;
use crate::contract_parameter::ContractParameterValue;
use crate::helper::Helper as ContractHelper;
use base64::{Engine as _, engine::general_purpose};
use neo_crypto::ECPoint;
use neo_error::{CoreError, CoreResult};
use neo_io::{MemoryReader, Serializable};
use neo_payloads::VerifiableExt;
use neo_payloads::{transaction::Transaction, witness::Witness};
use neo_primitives::ContractParameterType;
use neo_primitives::hex_util;
use neo_primitives::{UInt160, UInt256};
use neo_storage::{CacheRead, DataCache};
use neo_vm::OpCode;
use neo_vm::script_builder::ScriptBuilder;
use num_traits::ToPrimitive;
use std::collections::HashMap;
use std::sync::Arc;

/// Context item for managing signatures and parameters
#[derive(Clone, Debug)]
struct ContextItem {
    /// The script of the contract
    pub script: Option<Vec<u8>>,
    /// The parameters for the contract
    pub parameters: Vec<ContractParameter>,
    /// The collected signatures
    pub signatures: HashMap<ECPoint, Vec<u8>>,
}

impl ContextItem {
    /// Creates from a Contract
    pub fn from_contract(contract: &Contract) -> Self {
        let parameters = contract
            .parameter_list
            .iter()
            .map(|&p| ContractParameter::new(p))
            .collect();

        Self {
            script: Some(contract.script.clone()),
            parameters,
            signatures: HashMap::new(),
        }
    }

    /// Creates from JSON
    pub fn from_json(json: &serde_json::Value) -> CoreResult<Self> {
        let obj = json
            .as_object()
            .ok_or_else(|| CoreError::other("Expected object"))?;

        let script = match obj.get("script") {
            Some(value) if value.is_null() => None,
            Some(value) => {
                let text = value
                    .as_str()
                    .ok_or_else(|| CoreError::other("script must be a base64 string or null"))?;
                Some(
                    general_purpose::STANDARD
                        .decode(text)
                        .map_err(|err| CoreError::other(format!("invalid script base64: {err}")))?,
                )
            }
            None => None,
        };

        let parameters = match obj.get("parameters") {
            Some(value) => {
                let arr = value
                    .as_array()
                    .ok_or_else(|| CoreError::other("parameters must be an array"))?;
                arr.iter()
                    .enumerate()
                    .map(|(index, parameter)| {
                        ContractParameter::from_json(parameter).map_err(|err| {
                            CoreError::other(format!("invalid parameters[{index}]: {err}"))
                        })
                    })
                    .collect::<CoreResult<Vec<_>>>()?
            }
            None => Vec::new(),
        };

        let signatures = match obj.get("signatures") {
            Some(value) => {
                let sigs = value
                    .as_object()
                    .ok_or_else(|| CoreError::other("signatures must be an object"))?;
                let mut signatures = HashMap::new();
                for (key, value) in sigs {
                    let key_bytes = hex_util::decode_hex(key).map_err(|err| {
                        CoreError::other(format!("invalid signatures[{key}] public key hex: {err}"))
                    })?;
                    let pub_key = ECPoint::from_bytes(&key_bytes).map_err(|err| {
                        CoreError::other(format!("invalid signatures[{key}] public key: {err}"))
                    })?;
                    let signature_text = value.as_str().ok_or_else(|| {
                        CoreError::other(format!("signatures[{key}] must be a base64 string"))
                    })?;
                    let signature =
                        general_purpose::STANDARD
                            .decode(signature_text)
                            .map_err(|err| {
                                CoreError::other(format!(
                                    "invalid signatures[{key}] signature base64: {err}"
                                ))
                            })?;
                    signatures.insert(pub_key, signature);
                }
                signatures
            }
            None => HashMap::new(),
        };

        Ok(Self {
            script,
            parameters,
            signatures,
        })
    }

    /// Converts to JSON
    pub fn to_json(&self) -> serde_json::Value {
        let mut obj = serde_json::Map::new();

        if let Some(ref script) = self.script {
            obj.insert(
                "script".to_string(),
                serde_json::Value::String(general_purpose::STANDARD.encode(script)),
            );
        } else {
            obj.insert("script".to_string(), serde_json::Value::Null);
        }

        let params: Vec<serde_json::Value> = self.parameters.iter().map(|p| p.to_json()).collect();
        obj.insert("parameters".to_string(), serde_json::Value::Array(params));

        let mut sigs = serde_json::Map::new();
        for (key, value) in &self.signatures {
            sigs.insert(
                hex_util::encode_hex(&key.encoded()),
                serde_json::Value::String(general_purpose::STANDARD.encode(value)),
            );
        }
        obj.insert("signatures".to_string(), serde_json::Value::Object(sigs));

        serde_json::Value::Object(obj)
    }
}

/// The context used to add witnesses for Verifiable (matches C# ContractParametersContext)
pub struct ContractParametersContext {
    /// Serialized verifiable payload (unsigned, no witnesses).
    ///
    /// C# `ContractParametersContext.ToJson` writes `SerializeUnsigned` into the
    /// `data` field (ContractParametersContext.cs:414), and `FromJson` reconstructs
    /// via `DeserializeUnsigned` (line 298). The witness-less form is what makes a
    /// partially-signed context portable across wallets.
    verifiable_bytes: Vec<u8>,
    /// Hash of the verifiable (C# `Verifiable.Hash`), i.e. `Sha256(SerializeUnsigned)`.
    verifiable_hash: UInt256,
    /// Informational type name
    verifiable_type: String,
    /// The magic number of the network
    pub network: u32,

    /// Context items by script hash
    context_items: HashMap<UInt160, ContextItem>,

    /// Script hashes to be verified for the container
    script_hashes: Vec<UInt160>,
}

impl ContractParametersContext {
    /// Creates a new context
    pub fn new<B: CacheRead>(
        snapshot_cache: Arc<DataCache<B>>,
        verifiable: impl VerifiableExt + Serializable + 'static,
        network: u32,
    ) -> Self {
        Self::new_with_type(snapshot_cache, verifiable, network, None)
    }

    /// Creates a new context with an explicit type name (for parity with C# ToJson()).
    pub fn new_with_type<B: CacheRead>(
        snapshot_cache: Arc<DataCache<B>>,
        verifiable: impl VerifiableExt + Serializable + 'static,
        network: u32,
        verifiable_type: Option<String>,
    ) -> Self {
        let script_hashes = verifiable.script_hashes_for_verifying(snapshot_cache.as_ref());
        let verifiable_type =
            verifiable_type.unwrap_or_else(|| "Neo.Network.P2P.Payloads.Verifiable".to_string());
        // C# serializes the UNSIGNED verifiable (no witnesses) into `data`
        // (ContractParametersContext.cs:414). `Verifiable::hash_data()` returns exactly
        // that unsigned form, matching `IVerifiable.SerializeUnsigned`.
        let verifiable_bytes = verifiable.hash_data();
        // C# stores `Verifiable.Hash` in the context (line 408) = Sha256(SerializeUnsigned).
        let verifiable_hash = verifiable.hash().unwrap_or_default();
        Self {
            verifiable_bytes,
            verifiable_hash,
            verifiable_type,
            network,
            context_items: HashMap::new(),
            script_hashes,
        }
    }

    /// Determines whether all witnesses are ready
    pub fn completed(&self) -> bool {
        if self.context_items.len() < self.script_hashes.len() {
            return false;
        }
        self.context_items.values().all(Self::check_item_completed)
    }

    /// Checks if a context item is completed
    fn check_item_completed(item: &ContextItem) -> bool {
        item.parameters
            .iter()
            .all(|p| !matches!(p.value, ContractParameterValue::Any))
    }

    /// Adds a contract to the context
    pub fn add_contract(&mut self, contract: Contract) -> bool {
        self.add(contract)
    }

    /// Adds a contract to the context
    pub fn add(&mut self, contract: Contract) -> bool {
        let hash = contract.script_hash();

        if self.context_items.contains_key(&hash) {
            return false;
        }
        if !self.script_hashes.contains(&hash) {
            return false;
        }

        let item = ContextItem::from_contract(&contract);
        self.context_items.insert(hash, item);
        true
    }

    /// Adds a signature for a contract
    pub fn add_signature(
        &mut self,
        contract: Contract,
        public_key: ECPoint,
        signature: Vec<u8>,
    ) -> CoreResult<bool> {
        if signature.len() != 64 {
            return Err(CoreError::other("Invalid signature length"));
        }

        let hash = contract.script_hash();

        // Multi-signature contract path
        if let Some((_m, public_keys)) = ContractHelper::parse_multi_sig_contract(&contract.script)
        {
            let encoded = public_key
                .encode_point(true)
                .map_err(|e| CoreError::other(e.to_string()))?;
            if !public_keys
                .iter()
                .any(|key| key.as_slice() == encoded.as_slice())
            {
                return Ok(false);
            }

            if !self.context_items.contains_key(&hash) && !self.add(contract.clone()) {
                return Ok(false);
            }

            let item = match self.context_items.get_mut(&hash) {
                Some(item) => item,
                None => return Ok(false),
            };

            if item
                .parameters
                .iter()
                .all(|p| !matches!(p.value, ContractParameterValue::Any))
            {
                return Ok(false);
            }

            if item.signatures.contains_key(&public_key) {
                return Ok(false);
            }
            item.signatures
                .insert(public_key.clone(), signature.clone());

            if item.signatures.len() == contract.parameter_list.len() {
                let mut indexed: Vec<(usize, Vec<u8>)> = item
                    .signatures
                    .iter()
                    .filter_map(|(key, sig)| {
                        let key_bytes = key.encode_point(true).ok()?;
                        public_keys
                            .iter()
                            .position(|pk| pk.as_slice() == key_bytes.as_slice())
                            .map(|index| (index, sig.clone()))
                    })
                    .collect();
                indexed.sort_by_key(|entry| std::cmp::Reverse(entry.0));

                for (idx, (_, sig)) in indexed.into_iter().enumerate() {
                    if let Some(param) = item.parameters.get_mut(idx) {
                        param.value = ContractParameterValue::Signature(sig);
                    }
                }
            }

            return Ok(true);
        }

        // Single-signature contract path
        let mut index = None;
        for (i, param) in contract.parameter_list.iter().enumerate() {
            if *param == ContractParameterType::Signature {
                if index.is_some() {
                    return Err(CoreError::other("more than one signature parameter"));
                }
                index = Some(i);
            }
        }

        let Some(index) = index else {
            return Ok(false);
        };

        if !self.context_items.contains_key(&hash) && !self.add(contract.clone()) {
            return Ok(false);
        }

        let item = match self.context_items.get_mut(&hash) {
            Some(item) => item,
            None => return Ok(false),
        };

        if item.signatures.contains_key(&public_key) {
            return Ok(false);
        }

        item.signatures
            .insert(public_key.clone(), signature.clone());
        if let Some(param) = item.parameters.get_mut(index) {
            param.value = ContractParameterValue::Signature(signature);
        }

        Ok(true)
    }

    /// Gets the witnesses
    pub fn witnesses(&self) -> Option<Vec<Witness>> {
        if !self.completed() {
            return None;
        }

        let mut witnesses = Vec::new();

        for hash in &self.script_hashes {
            if let Some(item) = self.context_items.get(hash) {
                let invocation = Self::build_invocation_script(&item.parameters);
                let verification = item.script.clone().unwrap_or_default();
                witnesses.push(Witness::new_with_scripts(invocation, verification));
            }
        }

        Some(witnesses)
    }

    /// Builds invocation script from parameters
    fn build_invocation_script(parameters: &[ContractParameter]) -> Vec<u8> {
        let mut builder = ScriptBuilder::new();

        for param in parameters.iter().rev() {
            match &param.value {
                ContractParameterValue::Signature(sig) => {
                    builder.emit_opcode(OpCode::PUSHDATA1);
                    builder.emit(sig.len() as u8);
                    builder.emit_raw(sig);
                }
                ContractParameterValue::Boolean(b) => {
                    builder.emit_opcode(if *b { OpCode::PUSH1 } else { OpCode::PUSH0 });
                }
                ContractParameterValue::Integer(i) => {
                    if let Some(n) = i.to_i64() {
                        builder.emit_push_int(n);
                    }
                }
                ContractParameterValue::Hash160(h) => {
                    builder.emit_push(&h.to_bytes());
                }
                ContractParameterValue::Hash256(h) => {
                    builder.emit_push(&h.to_bytes());
                }
                ContractParameterValue::ByteArray(data) => {
                    builder.emit_push(data);
                }
                ContractParameterValue::String(s) => {
                    builder.emit_push(s.as_bytes());
                }
                ContractParameterValue::PublicKey(pk) => {
                    builder.emit_push(&pk.encoded());
                }
                ContractParameterValue::Array(array) => {
                    for item in array.iter().rev() {
                        builder.emit_push(Self::parameter_to_bytes(item).as_slice());
                    }
                    builder.emit_push_int(array.len() as i64);
                    builder.emit_opcode(OpCode::PACK);
                }
                _ => {}
            }
        }

        builder.to_array()
    }

    fn parameter_to_bytes(param: &ContractParameter) -> Vec<u8> {
        match &param.value {
            ContractParameterValue::Signature(sig) => sig.clone(),
            ContractParameterValue::Boolean(b) => vec![*b as u8],
            ContractParameterValue::Integer(i) => i.to_bytes_le().1,
            ContractParameterValue::Hash160(h) => h.to_bytes(),
            ContractParameterValue::Hash256(h) => h.to_bytes(),
            ContractParameterValue::ByteArray(data) => data.clone(),
            ContractParameterValue::PublicKey(pk) => pk.encoded(),
            ContractParameterValue::String(s) => s.as_bytes().to_vec(),
            ContractParameterValue::Array(arr) => {
                arr.iter().flat_map(Self::parameter_to_bytes).collect()
            }
            _ => Vec::new(),
        }
    }

    /// Converts to JSON
    pub fn to_json(&self) -> serde_json::Value {
        let mut obj = serde_json::Map::new();

        obj.insert(
            "type".to_string(),
            serde_json::Value::String(self.verifiable_type.clone()),
        );

        // C# writes `Verifiable.Hash` (Sha256 of the unsigned form), not a re-hash of
        // `data` here (ContractParametersContext.cs:408).
        obj.insert(
            "hash".to_string(),
            serde_json::Value::String(self.verifiable_hash.to_string()),
        );
        // `data` is the UNSIGNED serialization (no witnesses), Base64-encoded, matching
        // C# `SerializeUnsigned` (line 414).
        obj.insert(
            "data".to_string(),
            serde_json::Value::String(general_purpose::STANDARD.encode(self.hash_data())),
        );

        let mut items = serde_json::Map::new();
        for (hash, item) in &self.context_items {
            items.insert(hash.to_string(), item.to_json());
        }
        obj.insert("items".to_string(), serde_json::Value::Object(items));
        obj.insert(
            "network".to_string(),
            serde_json::Value::Number(self.network.into()),
        );

        serde_json::Value::Object(obj)
    }

    /// Creates from JSON
    pub fn from_json<B: CacheRead>(
        json: &serde_json::Value,
        verifiable: impl VerifiableExt + Serializable + 'static,
        snapshot: Arc<DataCache<B>>,
    ) -> CoreResult<Self> {
        let obj = json
            .as_object()
            .ok_or_else(|| CoreError::other("Expected object"))?;

        let network = obj.get("network").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

        let mut context = Self::new(snapshot, verifiable, network);

        if let Some(items) = obj.get("items").and_then(|v| v.as_object()) {
            for (hash_str, item_json) in items {
                let hash = hash_str
                    .parse::<UInt160>()
                    .map_err(|e| CoreError::other(e.to_string()))?;
                let item = ContextItem::from_json(item_json)?;
                context.context_items.insert(hash, item);
            }
        }

        Ok(context)
    }

    /// Parses a transaction signing context from JSON, returning the hydrated context and transaction.
    pub fn from_transaction_json<B: CacheRead>(
        json: &serde_json::Value,
        snapshot: Arc<DataCache<B>>,
    ) -> CoreResult<(Self, Transaction)> {
        let obj = json
            .as_object()
            .ok_or_else(|| CoreError::other("Expected object"))?;
        let type_name = obj
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CoreError::other("Missing context type"))?;
        if type_name != "Neo.Network.P2P.Payloads.Transaction" {
            return Err(CoreError::other(format!(
                "Unsupported context type: {}",
                type_name
            )));
        }

        let data_field = obj
            .get("data")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CoreError::other("Missing context data"))?;
        let data_bytes = general_purpose::STANDARD
            .decode(data_field)
            .map_err(|err| CoreError::other(format!("Invalid context data: {}", err)))?;
        // C# reconstructs the verifiable from the witness-less `data` via
        // `DeserializeUnsigned` (ContractParametersContext.cs:298). Using the full
        // `deserialize` here would (incorrectly) demand a witness array in `data`.
        let mut reader = MemoryReader::new(&data_bytes);
        let transaction = Transaction::deserialize_unsigned(&mut reader)
            .map_err(|err| CoreError::other(err.to_string()))?;

        // C# verifies `json["hash"] == verifiable.Hash` when present (line 299-304).
        if let Some(hash_str) = obj.get("hash").and_then(|v| v.as_str()) {
            let expected = hash_str
                .parse::<UInt256>()
                .map_err(|e| CoreError::other(format!("Invalid context hash: {e}")))?;
            let actual = transaction
                .try_hash()
                .map_err(|e| CoreError::other(e.to_string()))?;
            if expected != actual {
                return Err(CoreError::other(format!(
                    "context hash {expected} does not match transaction hash {actual}"
                )));
            }
        }

        let context = Self::from_json(json, transaction.clone(), snapshot)?;
        Ok((context, transaction))
    }

    /// Helper that parses from a JSON string payload.
    pub fn parse_transaction_context<B: CacheRead>(
        json_text: &str,
        snapshot: Arc<DataCache<B>>,
    ) -> CoreResult<(Self, Transaction)> {
        let value: serde_json::Value =
            serde_json::from_str(json_text).map_err(|err| CoreError::other(err.to_string()))?;
        Self::from_transaction_json(&value, snapshot)
    }

    fn hash_data(&self) -> &[u8] {
        &self.verifiable_bytes
    }

    /// Exposes script hashes for verification (C# ScriptHashes)
    pub fn script_hashes(&self) -> &[UInt160] {
        &self.script_hashes
    }
}

#[cfg(test)]
#[path = "../tests/contracts/contract_parameters_context.rs"]
mod tests;
