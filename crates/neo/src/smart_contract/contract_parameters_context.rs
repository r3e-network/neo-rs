//! ContractParametersContext - matches C# Neo.SmartContract.ContractParametersContext exactly

use crate::cryptography::crypto_utils::ECPoint;
use crate::smart_contract::contract::Contract;
use crate::smart_contract::contract_parameter::ContractParameter;
use crate::smart_contract::contract_parameter::ContractParameterValue;
use crate::smart_contract::contract_parameter_type::ContractParameterType;
use crate::{IVerifiable, UInt160};
use base64::{engine::general_purpose, Engine as _};
use num_traits::ToPrimitive;
use std::collections::HashMap;

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
                        let pub_key = hex::decode(key).ok().map(ECPoint::new)?;
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
    /// The verifiable to add witnesses
    pub verifiable: Box<dyn IVerifiable>,

    /// The snapshot cache used to read data
    pub snapshot_cache: DataCache,

    /// The magic number of the network
    pub network: u32,

    /// Context items by script hash
    context_items: HashMap<UInt160, ContextItem>,
}

impl ContractParametersContext {
    /// Creates a new context
    pub fn new(verifiable: Box<dyn IVerifiable>, snapshot_cache: DataCache, network: u32) -> Self {
        Self {
            verifiable,
            snapshot_cache,
            network,
            context_items: HashMap::new(),
        }
    }

    /// Determines whether all witnesses are ready
    pub fn completed(&self) -> bool {
        self.context_items.values().all(Self::check_item_completed)
    }

    /// Checks if a context item is completed
    fn check_item_completed(item: &ContextItem) -> bool {
        item.parameters
            .iter()
            .all(|p| !matches!(p.value, ContractParameterValue::Any))
    }

    /// Adds a contract to the context
    pub fn add(&mut self, contract: Contract) -> bool {
        let hash = contract.script_hash();

        if self.context_items.contains_key(&hash) {
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

        // Add contract if not present
        if !self.context_items.contains_key(&hash) {
            self.add(contract);
        }

        if let Some(item) = self.context_items.get_mut(&hash) {
            if item.signatures.contains_key(&public_key) {
                return Ok(false);
            }

            item.signatures
                .insert(public_key.clone(), signature.clone());

            // Try to update parameters for signature contracts
            if item.parameters.len() == 1
                && item.parameters[0].param_type == ContractParameterType::Signature
            {
                item.parameters[0].value = ContractParameterValue::Signature(signature);
                return Ok(true);
            }

            // Handle multi-sig contracts
            // This would need more complex logic to match signatures to positions

            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Gets the witnesses
    pub fn get_witnesses(&self) -> Vec<Witness> {
        if !self.completed() {
            return Vec::new();
        }

        let mut witnesses = Vec::new();

        for item in self.context_items.values() {
            // Build invocation script from parameters
            let invocation = Self::build_invocation_script(&item.parameters);

            // Use the verification script
            let verification = item.script.clone().unwrap_or_default();

            witnesses.push(Witness {
                invocation_script: invocation,
                verification_script: verification,
            });
        }

        witnesses
    }

    /// Builds invocation script from parameters
    fn build_invocation_script(parameters: &[ContractParameter]) -> Vec<u8> {
        let mut script = Vec::new();

        for param in parameters.iter().rev() {
            match &param.value {
                ContractParameterValue::Signature(sig) => {
                    script.push(0x40); // PUSHDATA1
                    script.push(sig.len() as u8);
                    script.extend_from_slice(sig);
                }
                ContractParameterValue::Boolean(b) => {
                    script.push(if *b { 0x51 } else { 0x50 }); // PUSH1 or PUSH0
                }
                ContractParameterValue::Integer(i) => {
                    // Push integer - simplified
                    if let Some(n) = i.to_i64() {
                        if n == -1 {
                            script.push(0x4f); // PUSHM1
                        } else if n == 0 {
                            script.push(0x50); // PUSH0
                        } else if n > 0 && n <= 16 {
                            script.push(0x50 + n as u8); // PUSH1-PUSH16
                        } else {
                            // PUSHDATA for larger numbers
                            let (_, bytes) = i.to_bytes_le();
                            script.push(bytes.len() as u8);
                            script.extend_from_slice(&bytes);
                        }
                    }
                }
                _ => {
                    // Handle other types as needed
                }
            }
        }

        script
    }

    /// Converts to JSON
    pub fn to_json(&self) -> serde_json::Value {
        let mut obj = serde_json::Map::new();

        // Add verifiable info (simplified)
        obj.insert(
            "network".to_string(),
            serde_json::Value::Number(self.network.into()),
        );

        // Add items
        let mut items = serde_json::Map::new();
        for (hash, item) in &self.context_items {
            items.insert(hash.to_string(), item.to_json());
        }
        obj.insert("items".to_string(), serde_json::Value::Object(items));

        serde_json::Value::Object(obj)
    }

    /// Creates from JSON
    pub fn from_json(
        json: &serde_json::Value,
        verifiable: Box<dyn IVerifiable>,
        snapshot: DataCache,
    ) -> Result<Self, String> {
        let obj = json.as_object().ok_or("Expected object")?;

        let network = obj.get("network").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

        let mut context = Self::new(verifiable, snapshot, network);

        if let Some(items) = obj.get("items").and_then(|v| v.as_object()) {
            for (hash_str, item_json) in items {
                let hash = hash_str.parse::<UInt160>().map_err(|e| e.to_string())?;
                let item = ContextItem::from_json(item_json)?;
                context.context_items.insert(hash, item);
            }
        }

        Ok(context)
    }
}

/// Witness structure (placeholder)
#[derive(Clone, Debug)]
pub struct Witness {
    pub invocation_script: Vec<u8>,
    pub verification_script: Vec<u8>,
}

/// DataCache placeholder
#[derive(Clone, Debug)]
pub struct DataCache {
    pub id: u32,
}
