use std::collections::HashMap;
use std::convert::TryFrom;
use neo_json::json_convert_trait::JsonConvertibleTrait;
use neo_json::json_error::JsonError;
use neo_json::jtoken::JToken;
use crate::cryptography::ECPoint;
use crate::neo_contract::contract::Contract;
use crate::neo_contract::contract_error::ContractError;
use crate::neo_contract::contract_parameter::ContractParameter;
use crate::network::payloads::{IVerifiable, Transaction};
use crate::persistence::DataCache;
use neo_type::H160;

/// The context used to add witnesses for `IVerifiable`.
pub struct ContractParametersContext {
    /// The `IVerifiable` to add witnesses.
    pub verifiable: Box<dyn IVerifiable<Error=ContractError>>,

    /// The snapshotcache used to read data.
    pub snapshot_cache: dyn DataCache,

    /// The magic number of the network.
    pub network: u32,

    context_items: HashMap<H160, ContextItem>,
    script_hashes: Option<Vec<H160>>,
}

struct ContextItem {
    script: Vec<u8>,
    parameters: Vec<ContractParameter>,
    signatures: HashMap<ECPoint, Vec<u8>>,
}

impl ContextItem {
    fn new(contract: &dyn Contract) -> Self {
        Self {
            script: contract.script.clone(),
            parameters: contract.parameter_list.iter()
                .map(|&p| ContractParameter::new(p))
                .collect(),
            signatures: HashMap::new(),
        }
    }
}

impl JsonConvertibleTrait for ContextItem {
    fn to_json(&self) -> serde_json::Value {
        let mut json = JToken::new_object();
        json.insert("script", JValue::from(hex::encode(&self.script)));
        json.insert("parameters", JValue::from(self.parameters.iter().map(|p| p.to_json()).collect::<Vec<_>>()));
        let signatures = self.signatures.iter()
            .map(|(k, v)| (k.to_string(), JValue::from(hex::encode(v))))
            .collect::<HashMap<_, _>>();
        json.insert("signatures", JValue::from(signatures));
        json
    }

    fn from_json(json: &serde_json::Value) -> Result<Self, JsonError>
    where
        Self: Sized
    {
        let script = json.get("script")
            .and_then(|v| v.as_str())
            .ok_or(JsonError::InvalidFormat)?;
        let script = hex::decode(script).map_err(|_| JsonError::InvalidFormat)?;

        let parameters = json.get("parameters")
            .and_then(|v| v.as_array())
            .ok_or(JsonError::InvalidFormat)?;
        let parameters = parameters.iter()
            .map(|item| ContractParameter::from_json(item))
            .collect::<Result<Vec<_>, _>>()?;

        let signatures = json.get("signatures")
            .and_then(|v| v.as_object())
            .ok_or(JsonError::InvalidFormat)?;
        let signatures = signatures.iter()
            .map(|(k, v)| {
                let public_key = ECPoint::try_from(k.as_str()).map_err(|_| JsonError::InvalidFormat)?;
                let signature = hex::decode(v.as_str().ok_or(JsonError::InvalidFormat)?).map_err(|_| JsonError::InvalidFormat)?;
                Ok((public_key, signature))
            })
            .collect::<Result<HashMap<_, _>, JsonError>>()?;

        Ok(Self {
            script,
            parameters,
            signatures,
        })
    }
}

impl ContractParametersContext {
    pub fn new(snapshot_cache: Box<dyn DataCache>, verifiable: Box<dyn IVerifiable<Error=ContractError>>, network: u32) -> Self {
        Self {
            verifiable,
            snapshot_cache,
            network,
            context_items: HashMap::new(),
            script_hashes: None,
        }
    }

    pub fn completed(&self) -> bool {
        if self.context_items.len() < self.script_hashes().len() {
            return false;
        }
        self.context_items.values().all(|item| {
            item.parameters.iter().all(|p| p.value.is_some())
        })
    }

    pub fn script_hashes(&self) -> &[H160] {
        if self.script_hashes.is_none() {
            let hashes = self.verifiable.get_script_hashes_for_verifying(&self.snapshot_cache);
            self.script_hashes = Some(hashes);
        }
        self.script_hashes.as_ref().unwrap()
    }

    pub fn add(&mut self, contract: Contract, parameters: Vec<ContractParameter>) -> bool {
        let script_hash = contract.script_hash();
        if !self.script_hashes().contains(&script_hash) {
            return false;
        }
        let item = self.context_items.entry(script_hash).or_insert_with(|| ContextItem::new(&contract));
        if item.parameters.len() != parameters.len() {
            return false;
        }
        for (i, parameter) in parameters.into_iter().enumerate() {
            if parameter.type_ != item.parameters[i].type_ {
                return false;
            }
            item.parameters[i] = parameter;
        }
        true
    }

    pub fn add_signature(&mut self, contract: Contract, public_key: ECPoint, signature: Vec<u8>) -> bool {
        if signature.len() != 64 {
            return false;
        }
        let script_hash = contract.script_hash();
        let item = self.context_items.entry(script_hash).or_insert_with(|| ContextItem::new(&contract));
        item.signatures.insert(public_key, signature);
        true
    }

    pub fn create_item(&mut self, contract: Contract) -> bool {
        let script_hash = contract.script_hash();
        if !self.script_hashes().contains(&script_hash) {
            return false;
        }
        self.context_items.entry(script_hash).or_insert_with(|| ContextItem::new(&contract));
        true
    }

    pub fn get_parameters(&self, script_hash: &H160) -> Option<&[ContractParameter]> {
        self.context_items.get(script_hash).map(|item| item.parameters.as_slice())
    }

    pub fn get_signatures(&self, script_hash: &H160) -> Option<&HashMap<ECPoint, Vec<u8>>> {
        self.context_items.get(script_hash).map(|item| &item.signatures)
    }

    pub fn get_script(&self, script_hash: &H160) -> Option<&[u8]> {
        self.context_items.get(script_hash).map(|item| item.script.as_slice())
    }

    pub fn from_json(json: &JObject, snapshot: DataCache) -> Result<Self, Error> {
        let network = json.get("network")
            .and_then(|v| v.as_u64())
            .ok_or(Error::InvalidFormat)? as u32;

        let type_ = json.get("type")
            .and_then(|v| v.as_str())
            .ok_or(Error::InvalidFormat)?;

        let verifiable: Box<dyn IVerifiable> = match type_ {
            "Block" => Box::new(Block::from_json(json.get("verifiable").ok_or(Error::InvalidFormat)?)?),
            "Transaction" => Box::new(Transaction::from_json(json.get("verifiable").ok_or(Error::InvalidFormat)?)?),
            _ => return Err(Error::InvalidFormat),
        };

        let items = json.get("items")
            .and_then(|v| v.as_object())
            .ok_or(Error::InvalidFormat)?;
        let context_items = items.iter()
            .map(|(k, v)| {
                let script_hash = H160::try_from(k.as_str()).map_err(|_| Error::InvalidFormat)?;
                let item = ContextItem::from_json(v.as_object().ok_or(Error::InvalidFormat)?)?;
                Ok((script_hash, item))
            })
            .collect::<Result<HashMap<_, _>, Error>>()?;

        Ok(Self {
            verifiable,
            snapshot_cache: snapshot,
            network,
            context_items,
            script_hashes: None,
        })
    }

    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert("type", JValue::from(self.verifiable.type_name()));
        json.insert("network", JValue::from(self.network));
        json.insert("verifiable", self.verifiable.to_json());
        let items = self.context_items.iter()
            .map(|(k, v)| (k.to_string(), JValue::from(v.to_json())))
            .collect::<HashMap<_, _>>();
        json.insert("items", JValue::from(items));
        json
    }
}

// Additional implementations and trait bounds would be needed for full functionality

// Implement Clone trait for ContractParametersContext
impl Clone for ContractParametersContext {
    fn clone(&self) -> Self {
        Self {
            verifiable: self.verifiable.clone(),
            snapshot_cache: self.snapshot_cache.clone(),
            network: self.network,
            context_items: self.context_items.clone(),
            script_hashes: self.script_hashes.clone(),
        }
    }
}

// Implement Default trait for ContractParametersContext
impl Default for ContractParametersContext {
    fn default() -> Self {
        Self {
            verifiable: Box::new(Transaction::default()),
            snapshot_cache: DataCache::default(),
            network: 0,
            context_items: HashMap::new(),
            script_hashes: None,
        }
    }
}

// Add methods for interacting with other contracts
impl ContractParametersContext {
    pub fn call_contract(&self, script_hash: H160, method: &str, args: Vec<StackItem>) -> Result<StackItem, Error> {
        // Implementation for calling other contracts
        unimplemented!("Contract interaction not implemented in this context")
    }

    pub fn get_storage(&self, script_hash: H160, key: &[u8]) -> Option<Vec<u8>> {
        // Implementation for accessing contract storage
        unimplemented!("Storage access not implemented in this context")
    }
}