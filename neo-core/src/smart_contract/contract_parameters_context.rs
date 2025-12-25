//! ContractParametersContext - matches C# Neo.SmartContract.ContractParametersContext exactly

use crate::cryptography::{ECPoint, NeoHash};
use crate::neo_io::{BinaryWriter, MemoryReader, Serializable};
use crate::network::p2p::payloads::{transaction::Transaction, witness::Witness};
use crate::persistence::DataCache;
use crate::smart_contract::contract::Contract;
use crate::smart_contract::contract_parameter::ContractParameter;
use crate::smart_contract::contract_parameter::ContractParameterValue;
use crate::smart_contract::contract_parameter_type::ContractParameterType;
use crate::smart_contract::helper::Helper as ContractHelper;
use crate::{IVerifiable, UInt160, UInt256};
use base64::{engine::general_purpose, Engine as _};
use neo_vm::{op_code::OpCode, ScriptBuilder};
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
    pub fn from_json(json: &serde_json::Value) -> Result<Self, String> {
        let obj = json.as_object().ok_or("Expected object")?;

        let script = obj
            .get("script")
            .and_then(|v| v.as_str())
            .and_then(|s| general_purpose::STANDARD.decode(s).ok());

        let parameters = obj
            .get("parameters")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|p| ContractParameter::from_json(p).ok())
                    .collect()
            })
            .unwrap_or_default();

        let signatures = obj
            .get("signatures")
            .and_then(|v| v.as_object())
            .map(|sigs| {
                sigs.iter()
                    .filter_map(|(key, value)| {
                        let key_bytes = hex::decode(key).ok()?;
                        let pub_key = ECPoint::from_bytes(&key_bytes).ok()?;
                        let sig = value
                            .as_str()
                            .and_then(|s| general_purpose::STANDARD.decode(s).ok())?;
                        Some((pub_key, sig))
                    })
                    .collect()
            })
            .unwrap_or_default();

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
                hex::encode(key.encoded()),
                serde_json::Value::String(general_purpose::STANDARD.encode(value)),
            );
        }
        obj.insert("signatures".to_string(), serde_json::Value::Object(sigs));

        serde_json::Value::Object(obj)
    }
}

/// The context used to add witnesses for IVerifiable (matches C# ContractParametersContext)
pub struct ContractParametersContext {
    /// Serialized verifiable payload (unsigned)
    verifiable_bytes: Vec<u8>,
    /// Informational type name
    verifiable_type: String,
    /// The snapshot cache used to read data
    pub snapshot_cache: Arc<DataCache>,

    /// The magic number of the network
    pub network: u32,

    /// Context items by script hash
    context_items: HashMap<UInt160, ContextItem>,

    /// Script hashes to be verified for the container
    script_hashes: Vec<UInt160>,
}

impl ContractParametersContext {
    /// Creates a new context
    pub fn new(
        snapshot_cache: Arc<DataCache>,
        verifiable: impl IVerifiable + Serializable + 'static,
        network: u32,
    ) -> Self {
        Self::new_with_type(snapshot_cache, verifiable, network, None)
    }

    /// Creates a new context with an explicit type name (for parity with C# ToJson()).
    pub fn new_with_type(
        snapshot_cache: Arc<DataCache>,
        verifiable: impl IVerifiable + Serializable + 'static,
        network: u32,
        verifiable_type: Option<String>,
    ) -> Self {
        let script_hashes = verifiable.get_script_hashes_for_verifying(snapshot_cache.as_ref());
        let verifiable_type =
            verifiable_type.unwrap_or_else(|| "Neo.Network.P2P.Payloads.IVerifiable".to_string());
        let mut writer = BinaryWriter::new();
        let _ = verifiable.serialize(&mut writer);
        let verifiable_bytes = writer.into_bytes();
        Self {
            verifiable_bytes,
            verifiable_type,
            snapshot_cache,
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
    ) -> Result<bool, String> {
        if signature.len() != 64 {
            return Err("Invalid signature length".to_string());
        }

        let hash = contract.script_hash();

        // Multi-signature contract path
        if let Some((_m, public_keys)) = ContractHelper::parse_multi_sig_contract(&contract.script) {
            let encoded = public_key
                .encode_point(true)
                .map_err(|e| e.to_string())?;
            if !public_keys.iter().any(|key| key.as_slice() == encoded.as_slice()) {
                return Ok(false);
            }

            if !self.context_items.contains_key(&hash) {
                if !self.add(contract.clone()) {
                    return Ok(false);
                }
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
                indexed.sort_by(|a, b| b.0.cmp(&a.0));

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
                    return Err("more than one signature parameter".to_string());
                }
                index = Some(i);
            }
        }

        let Some(index) = index else {
            return Ok(false);
        };

        if !self.context_items.contains_key(&hash) {
            if !self.add(contract.clone()) {
                return Ok(false);
            }
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
    pub fn get_witnesses(&self) -> Option<Vec<Witness>> {
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

        let data_bytes = self.get_hash_data();
        let hash_bytes = NeoHash::hash256(&data_bytes);
        let hash = UInt256::from_bytes(&hash_bytes).unwrap_or_else(|_| UInt256::default());
        obj.insert(
            "hash".to_string(),
            serde_json::Value::String(hash.to_string()),
        );
        obj.insert(
            "data".to_string(),
            serde_json::Value::String(general_purpose::STANDARD.encode(data_bytes)),
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
    pub fn from_json(
        json: &serde_json::Value,
        verifiable: impl IVerifiable + Serializable + 'static,
        snapshot: Arc<DataCache>,
    ) -> Result<Self, String> {
        let obj = json.as_object().ok_or("Expected object")?;

        let network = obj.get("network").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

        let mut context = Self::new(snapshot, verifiable, network);

        if let Some(items) = obj.get("items").and_then(|v| v.as_object()) {
            for (hash_str, item_json) in items {
                let hash = hash_str.parse::<UInt160>().map_err(|e| e.to_string())?;
                let item = ContextItem::from_json(item_json)?;
                context.context_items.insert(hash, item);
            }
        }

        Ok(context)
    }

    /// Parses a transaction signing context from JSON, returning the hydrated context and transaction.
    pub fn from_transaction_json(
        json: &serde_json::Value,
        snapshot: Arc<DataCache>,
    ) -> Result<(Self, Transaction), String> {
        let obj = json.as_object().ok_or("Expected object")?;
        let type_name = obj
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or("Missing context type")?;
        if type_name != "Neo.Network.P2P.Payloads.Transaction" {
            return Err(format!("Unsupported context type: {}", type_name));
        }

        let data_field = obj
            .get("data")
            .and_then(|v| v.as_str())
            .ok_or("Missing context data")?;
        let data_bytes = general_purpose::STANDARD
            .decode(data_field)
            .map_err(|err| format!("Invalid context data: {}", err))?;
        let mut reader = MemoryReader::new(&data_bytes);
        let transaction = <Transaction as Serializable>::deserialize(&mut reader)
            .map_err(|err| err.to_string())?;

        let context = Self::from_json(json, transaction.clone(), snapshot)?;
        Ok((context, transaction))
    }

    /// Helper that parses from a JSON string payload.
    pub fn parse_transaction_context(
        json_text: &str,
        snapshot: Arc<DataCache>,
    ) -> Result<(Self, Transaction), String> {
        let value: serde_json::Value =
            serde_json::from_str(json_text).map_err(|err| err.to_string())?;
        Self::from_transaction_json(&value, snapshot)
    }

    fn get_hash_data(&self) -> Vec<u8> {
        self.verifiable_bytes.clone()
    }

    /// Exposes script hashes for verification (C# ScriptHashes)
    pub fn script_hashes(&self) -> &[UInt160] {
        &self.script_hashes
    }
}
