//! Oracle native contract implementation.
//!
//! The Oracle contract manages external data requests and responses,
//! enabling smart contracts to access off-chain data sources.

use crate::cryptography::crypto_utils::NeoHash;
use crate::error::{CoreError as Error, CoreResult as Result};
use crate::neo_config::{
    HASH_SIZE, MAX_SCRIPT_SIZE, MAX_TRANSACTIONS_PER_BLOCK, SECONDS_PER_BLOCK,
};
use crate::network::p2p::payloads::{
    oracle_response::OracleResponse as TxOracleResponse,
    transaction_attribute::TransactionAttribute,
};
use crate::persistence::{
    i_read_only_store::IReadOnlyStoreGeneric, seek_direction::SeekDirection, DataCache,
};
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::binary_serializer::BinarySerializer;
use crate::smart_contract::contract::Contract;
use crate::smart_contract::native::{
    oracle_request::OracleRequest, GasToken, NativeContract, NativeMethod, Role, RoleManagement,
};
use crate::smart_contract::storage_key::StorageKey;
use crate::smart_contract::StorageItem;
use crate::{UInt160, UInt256};
use bincode;
use neo_vm::{ExecutionEngineLimits, StackItem};
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::TryInto;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_PRICE: i64 = 50_000_000;
const PREFIX_PRICE: u8 = 0x05;
const PREFIX_REQUEST: u8 = 0x07;
const PREFIX_ID_LIST: u8 = 0x06;
const PREFIX_REQUEST_ID: u8 = 0x09;
const MAX_PENDING_PER_URL: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingRequest {
    id: u64,
    original_tx_id: UInt256,
    gas_for_response: i64,
    url: String,
    filter: Option<String>,
    callback_contract: UInt160,
    callback_method: String,
    user_data: Vec<u8>,
    block_height: u32,
    timestamp: u64,
}

/// The Oracle native contract.
pub struct OracleContract {
    id: i32,
    hash: UInt160,
    methods: Vec<NativeMethod>,
    /// Oracle configuration.
    config: OracleConfig,
}

/// Oracle configuration parameters.
#[derive(Debug, Clone)]
pub struct OracleConfig {
    /// Maximum URL length.
    pub max_url_length: usize,

    /// Maximum filter length.
    pub max_filter_length: usize,

    /// Maximum callback method name length.
    pub max_callback_length: usize,

    /// Maximum user data length.
    pub max_user_data_length: usize,

    /// Maximum response data length.
    pub max_response_length: usize,

    /// Request timeout in blocks.
    pub request_timeout: u32,

    /// Minimum gas for response.
    pub min_response_gas: i64,

    /// Maximum gas for response.
    pub max_response_gas: i64,
}

impl Default for OracleConfig {
    fn default() -> Self {
        Self {
            max_url_length: 256,
            max_filter_length: 128,
            max_callback_length: HASH_SIZE,
            max_user_data_length: MAX_TRANSACTIONS_PER_BLOCK,
            max_response_length: MAX_SCRIPT_SIZE,
            request_timeout: 144, // ~24 hours at 10 second blocks
            min_response_gas: 10_000_000,
            max_response_gas: 50_000_000,
        }
    }
}

/// Builder for `OracleConfig` with fluent API and validation.
#[derive(Debug, Clone)]
pub struct OracleConfigBuilder {
    config: OracleConfig,
}

impl OracleConfigBuilder {
    /// Creates a new builder with default values.
    #[inline]
    pub fn new() -> Self {
        Self {
            config: OracleConfig::default(),
        }
    }

    /// Sets the maximum URL length.
    #[inline]
    pub fn max_url_length(mut self, len: usize) -> Self {
        self.config.max_url_length = len;
        self
    }

    /// Sets the maximum filter length.
    #[inline]
    pub fn max_filter_length(mut self, len: usize) -> Self {
        self.config.max_filter_length = len;
        self
    }

    /// Sets the maximum callback method name length.
    #[inline]
    pub fn max_callback_length(mut self, len: usize) -> Self {
        self.config.max_callback_length = len;
        self
    }

    /// Sets the maximum user data length.
    #[inline]
    pub fn max_user_data_length(mut self, len: usize) -> Self {
        self.config.max_user_data_length = len;
        self
    }

    /// Sets the maximum response data length.
    #[inline]
    pub fn max_response_length(mut self, len: usize) -> Self {
        self.config.max_response_length = len;
        self
    }

    /// Sets the request timeout in blocks.
    #[inline]
    pub fn request_timeout(mut self, timeout: u32) -> Self {
        self.config.request_timeout = timeout;
        self
    }

    /// Sets the minimum gas for response.
    #[inline]
    pub fn min_response_gas(mut self, gas: i64) -> Self {
        self.config.min_response_gas = gas;
        self
    }

    /// Sets the maximum gas for response.
    #[inline]
    pub fn max_response_gas(mut self, gas: i64) -> Self {
        self.config.max_response_gas = gas;
        self
    }

    /// Validates and builds the configuration.
    pub fn build(self) -> Result<OracleConfig> {
        // Validate constraints
        if self.config.min_response_gas > self.config.max_response_gas {
            return Err(Error::invalid_operation(
                "min_response_gas cannot exceed max_response_gas".to_string(),
            ));
        }
        if self.config.max_url_length == 0 {
            return Err(Error::invalid_operation(
                "max_url_length must be greater than 0".to_string(),
            ));
        }
        Ok(self.config)
    }

    /// Builds without validation (for internal use).
    #[inline]
    pub fn build_unchecked(self) -> OracleConfig {
        self.config
    }
}

impl Default for OracleConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl OracleContract {
    const ID: i32 = -9;

    /// Creates a new Oracle contract.
    pub fn new() -> Self {
        // Oracle contract hash: 0xfe924b7cfe89ddd271abaf7210a80a7e11178758
        let hash = UInt160::from_bytes(&[
            0xfe, 0x92, 0x4b, 0x7c, 0xfe, 0x89, 0xdd, 0xd2, 0x71, 0xab, 0xaf, 0x72, 0x10, 0xa8,
            0x0a, 0x7e, 0x11, 0x17, 0x87, 0x58,
        ])
        .expect("Operation failed");

        let methods = vec![
            NativeMethod::unsafe_method("request".to_string(), 1 << SECONDS_PER_BLOCK, 0x0f),
            NativeMethod::safe("getPrice".to_string(), 1 << 4),
            NativeMethod::unsafe_method("setPrice".to_string(), 1 << SECONDS_PER_BLOCK, 0x01),
            NativeMethod::unsafe_method("finish".to_string(), 1 << SECONDS_PER_BLOCK, 0x0f),
            NativeMethod::safe("verify".to_string(), 1 << SECONDS_PER_BLOCK),
        ];

        Self {
            id: Self::ID,
            hash,
            methods,
            config: OracleConfig::default(),
        }
    }

    pub fn invoke_method(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        match method {
            "request" => self.request(engine, args),
            "getPrice" => {
                if !args.is_empty() {
                    return Err(Error::invalid_operation(
                        "getPrice does not accept arguments".to_string(),
                    ));
                }
                let snapshot = engine.snapshot_cache();
                Ok(self
                    .get_price_value(snapshot.as_ref())
                    .to_le_bytes()
                    .to_vec())
            }
            "setPrice" => self.set_price(engine, args),
            "finish" => self.finish(engine),
            "verify" => self.verify(engine),
            _ => Err(Error::native_contract(format!(
                "Unknown method: {}",
                method
            ))),
        }
    }

    fn request(&self, engine: &mut ApplicationEngine, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() != 5 {
            return Err(Error::invalid_operation(
                "Invalid argument count".to_string(),
            ));
        }

        let url = String::from_utf8(args[0].clone())
            .map_err(|_| Error::invalid_operation("Invalid URL".to_string()))?;
        let filter = if args[1].is_empty() {
            None
        } else {
            Some(
                String::from_utf8(args[1].clone())
                    .map_err(|_| Error::invalid_operation("Invalid filter".to_string()))?,
            )
        };
        let callback = String::from_utf8(args[2].clone())
            .map_err(|_| Error::invalid_operation("Invalid callback".to_string()))?;
        let user_data = args[3].clone();
        let gas_for_response = i64::from_le_bytes(
            args[4]
                .as_slice()
                .try_into()
                .map_err(|_| Error::invalid_operation("Invalid gas amount".to_string()))?,
        );

        if url.len() > self.config.max_url_length {
            return Err(Error::invalid_operation("URL too long".to_string()));
        }
        if let Some(ref f) = filter {
            if f.len() > self.config.max_filter_length {
                return Err(Error::invalid_operation("Filter too long".to_string()));
            }
        }
        if callback.is_empty() || callback.len() > self.config.max_callback_length {
            return Err(Error::invalid_operation(
                "Callback name too long".to_string(),
            ));
        }
        if callback.starts_with('_') {
            return Err(Error::invalid_operation(
                "Callback cannot start with underscore".to_string(),
            ));
        }
        if user_data.len() > self.config.max_user_data_length {
            return Err(Error::invalid_operation("User data too long".to_string()));
        }
        if gas_for_response < self.config.min_response_gas
            || gas_for_response > self.config.max_response_gas
        {
            return Err(Error::invalid_operation("Invalid gas amount".to_string()));
        }

        let calling_contract = engine
            .get_calling_script_hash()
            .unwrap_or_else(UInt160::zero);
        let original_tx_id = engine
            .script_container()
            .and_then(|container| container.as_transaction().map(|tx| tx.hash()))
            .unwrap_or_else(UInt256::zero);
        let block_height = engine.current_block_index();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| Error::runtime_error(e.to_string()))?
            .as_secs();

        let snapshot_arc = engine.snapshot_cache();
        let snapshot = snapshot_arc.as_ref();
        let price = self.get_price_value(snapshot);
        engine.add_runtime_fee(price as u64)?;
        engine.add_runtime_fee(gas_for_response as u64)?;
        let id = self.next_request_id(snapshot)?;
        let url_hash = self.compute_url_hash(&url);

        let request = PendingRequest {
            id,
            original_tx_id,
            gas_for_response,
            url,
            filter,
            callback_contract: calling_contract,
            callback_method: callback,
            user_data,
            block_height,
            timestamp,
        };

        self.write_request(snapshot, &request)?;
        self.append_request_id(snapshot, &url_hash, id)?;
        self.emit_oracle_request(engine, id, calling_contract, &request)?;

        Ok(id.to_le_bytes().to_vec())
    }

    fn finish(&self, engine: &mut ApplicationEngine) -> Result<Vec<u8>> {
        let tx = engine
            .script_container()
            .and_then(|container| container.as_transaction())
            .ok_or_else(|| {
                Error::invalid_operation(
                    "Oracle finish must be invoked within a transaction".to_string(),
                )
            })?;

        let response = tx
            .attributes()
            .iter()
            .find_map(|attr| match attr {
                TransactionAttribute::OracleResponse(attr) => Some(attr.clone()),
                _ => None,
            })
            .ok_or_else(|| {
                Error::invalid_operation("Oracle response attribute missing".to_string())
            })?;

        let snapshot_arc = engine.snapshot_cache();
        let snapshot = snapshot_arc.as_ref();
        self.process_response(engine, snapshot, response)
    }

    fn verify(&self, engine: &mut ApplicationEngine) -> Result<Vec<u8>> {
        let tx = match engine
            .script_container()
            .and_then(|container| container.as_transaction())
        {
            Some(tx) => tx,
            None => return Ok(vec![0]),
        };
        let snapshot = engine.snapshot_cache();
        let settings = engine.protocol_settings();
        let valid = tx.attributes().iter().any(|attr| match attr {
            TransactionAttribute::OracleResponse(attr) => {
                attr.verify(settings, snapshot.as_ref(), tx)
            }
            _ => false,
        });
        Ok(vec![if valid { 1 } else { 0 }])
    }

    fn get_price_value(&self, snapshot: &DataCache) -> i64 {
        let key = self.price_key();
        snapshot
            .try_get(&key)
            .and_then(|item| {
                let bytes = item.get_value();
                if bytes.len() == 8 {
                    Some(i64::from_le_bytes(bytes.try_into().ok()?))
                } else {
                    None
                }
            })
            .unwrap_or(DEFAULT_PRICE)
    }

    fn set_price(&self, engine: &mut ApplicationEngine, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() != 1 {
            return Err(Error::invalid_operation(
                "setPrice requires 1 argument".to_string(),
            ));
        }

        let price = i64::from_le_bytes(
            args[0]
                .as_slice()
                .try_into()
                .map_err(|_| Error::invalid_operation("Invalid price value".to_string()))?,
        );

        if price <= 0 {
            return Err(Error::invalid_operation(
                "Price must be positive".to_string(),
            ));
        }

        if !engine
            .check_committee_witness()
            .map_err(|err| Error::runtime_error(err.to_string()))?
        {
            return Err(Error::invalid_operation(
                "Committee authorization required".to_string(),
            ));
        }

        let snapshot_arc = engine.snapshot_cache();
        let snapshot = snapshot_arc.as_ref();
        self.put_item(
            snapshot,
            self.price_key(),
            StorageItem::from_bytes(price.to_le_bytes().to_vec()),
        );

        Ok(Vec::new())
    }

    fn next_request_id(&self, snapshot: &DataCache) -> Result<u64> {
        let key = self.request_id_key();
        let current = snapshot
            .try_get(&key)
            .and_then(|item| {
                let bytes = item.get_value();
                if bytes.len() == 8 {
                    Some(u64::from_be_bytes(bytes.try_into().ok()?))
                } else {
                    None
                }
            })
            .unwrap_or(0);
        let next = current
            .checked_add(1)
            .ok_or_else(|| Error::runtime_error("Next request id overflowed"))?;
        self.put_item(
            snapshot,
            key,
            StorageItem::from_bytes(next.to_be_bytes().to_vec()),
        );
        Ok(current)
    }

    fn append_request_id(&self, snapshot: &DataCache, hash: &[u8; 20], id: u64) -> Result<()> {
        let mut list = self.read_id_list(snapshot, hash)?;
        if list.len() >= MAX_PENDING_PER_URL {
            return Err(Error::invalid_operation(
                "There are too many pending responses for this url".to_string(),
            ));
        }
        list.push(id);
        self.write_id_list(snapshot, hash, &list)?;
        Ok(())
    }

    fn remove_request_id(&self, snapshot: &DataCache, hash: &[u8; 20], id: u64) -> Result<()> {
        let mut list = self.read_id_list(snapshot, hash)?;
        if let Some(pos) = list.iter().position(|existing| *existing == id) {
            list.remove(pos);
        }
        if list.is_empty() {
            snapshot.delete(&self.id_list_key(hash));
        } else {
            self.write_id_list(snapshot, hash, &list)?;
        }
        Ok(())
    }

    fn read_request(&self, snapshot: &DataCache, id: u64) -> Result<Option<PendingRequest>> {
        let key = self.request_storage_key(id);
        snapshot
            .try_get(&key)
            .map(|item| self.deserialize_request(&item.get_value()))
            .transpose()
    }

    fn write_request(&self, snapshot: &DataCache, request: &PendingRequest) -> Result<()> {
        let key = self.request_storage_key(request.id);
        let bytes = self.serialize_request(request)?;
        self.put_item(snapshot, key, StorageItem::from_bytes(bytes));
        Ok(())
    }

    fn delete_request(&self, snapshot: &DataCache, id: u64) {
        let key = self.request_storage_key(id);
        snapshot.delete(&key);
    }

    fn serialize_request(&self, request: &PendingRequest) -> Result<Vec<u8>> {
        bincode::serialize(request).map_err(|err| Error::serialization(err.to_string()))
    }

    fn deserialize_request(&self, bytes: &[u8]) -> Result<PendingRequest> {
        bincode::deserialize(bytes).map_err(|err| Error::serialization(err.to_string()))
    }

    fn read_id_list(&self, snapshot: &DataCache, hash: &[u8; 20]) -> Result<Vec<u64>> {
        let key = self.id_list_key(hash);
        if let Some(item) = snapshot.try_get(&key) {
            self.deserialize_id_list(&item.get_value())
        } else {
            Ok(Vec::new())
        }
    }

    fn write_id_list(&self, snapshot: &DataCache, hash: &[u8; 20], list: &[u64]) -> Result<()> {
        let key = self.id_list_key(hash);
        let bytes = self.serialize_id_list(list)?;
        self.put_item(snapshot, key, StorageItem::from_bytes(bytes));
        Ok(())
    }

    fn price_key(&self) -> StorageKey {
        StorageKey::create(Self::ID, PREFIX_PRICE)
    }

    fn request_id_key(&self) -> StorageKey {
        StorageKey::create(Self::ID, PREFIX_REQUEST_ID)
    }

    fn request_storage_key(&self, id: u64) -> StorageKey {
        StorageKey::create_with_bytes(Self::ID, PREFIX_REQUEST, &id.to_be_bytes())
    }

    fn id_list_key(&self, hash: &[u8; 20]) -> StorageKey {
        StorageKey::create_with_bytes(Self::ID, PREFIX_ID_LIST, hash)
    }

    fn parse_request_id(key: &StorageKey) -> Option<u64> {
        let suffix = key.suffix();
        if suffix.len() != 1 + std::mem::size_of::<u64>() {
            return None;
        }
        if suffix[0] != PREFIX_REQUEST {
            return None;
        }
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&suffix[1..]);
        Some(u64::from_be_bytes(buf))
    }

    fn put_item(&self, snapshot: &DataCache, key: StorageKey, item: StorageItem) {
        if snapshot.get(&key).is_some() {
            snapshot.update(key, item);
        } else {
            snapshot.add(key, item);
        }
    }

    fn compute_url_hash(&self, url: &str) -> [u8; 20] {
        NeoHash::hash160(url.as_bytes())
    }

    fn serialize_id_list(&self, list: &[u64]) -> Result<Vec<u8>> {
        let items = list
            .iter()
            .map(|id| StackItem::from_int(*id as i64))
            .collect::<Vec<_>>();
        BinarySerializer::serialize(
            &StackItem::from_array(items),
            &ExecutionEngineLimits::default(),
        )
        .map_err(|err| Error::serialization(err.to_string()))
    }

    fn deserialize_id_list(&self, bytes: &[u8]) -> Result<Vec<u64>> {
        let item = BinarySerializer::deserialize(bytes, &ExecutionEngineLimits::default(), None)
            .map_err(|err| Error::serialization(err.to_string()))?;

        let StackItem::Array(array) = item else {
            return Err(Error::invalid_data(
                "Corrupted oracle URL id list entry".to_string(),
            ));
        };

        let mut ids = Vec::with_capacity(array.len());
        for element in array.items() {
            let value = element
                .as_int()
                .map_err(|_| Error::invalid_data("Invalid URL id entry".to_string()))?;
            let id = value
                .to_u64()
                .ok_or_else(|| Error::invalid_data("URL id entry overflow".to_string()))?;
            ids.push(id);
        }

        Ok(ids)
    }

    fn emit_oracle_request(
        &self,
        engine: &mut ApplicationEngine,
        id: u64,
        contract: UInt160,
        request: &PendingRequest,
    ) -> Result<()> {
        let state = vec![
            StackItem::from_int(id as i64),
            StackItem::from_byte_string(contract.to_bytes()),
            StackItem::from_byte_string(request.url.as_bytes().to_vec()),
            match &request.filter {
                Some(filter) => StackItem::from_byte_string(filter.as_bytes().to_vec()),
                None => StackItem::null(),
            },
        ];
        engine
            .send_notification(self.hash, "OracleRequest".to_string(), state)
            .map_err(Error::runtime_error)
    }

    fn emit_oracle_response(
        &self,
        engine: &mut ApplicationEngine,
        request_id: u64,
        request: &PendingRequest,
    ) -> Result<()> {
        let state = vec![
            StackItem::from_int(request_id as i64),
            StackItem::from_byte_string(request.original_tx_id.to_bytes()),
        ];
        engine
            .send_notification(self.hash, "OracleResponse".to_string(), state)
            .map_err(Error::runtime_error)
    }

    fn cleanup_persisted_responses(&self, engine: &mut ApplicationEngine) -> Result<()> {
        let Some(block) = engine.persisting_block().cloned() else {
            return Ok(());
        };

        let snapshot_arc = engine.snapshot_cache();
        let snapshot = snapshot_arc.as_ref();

        for transaction in &block.transactions {
            for attribute in transaction.attributes() {
                if let TransactionAttribute::OracleResponse(response) = attribute {
                    if let Some(request) = self.read_request(snapshot, response.id)? {
                        let url_hash = self.compute_url_hash(&request.url);
                        self.delete_request(snapshot, response.id);
                        self.remove_request_id(snapshot, &url_hash, response.id)?;
                    }
                }
            }
        }

        Ok(())
    }

    fn reward_oracle_nodes(&self, engine: &mut ApplicationEngine) -> Result<()> {
        let Some(block) = engine.persisting_block().cloned() else {
            return Ok(());
        };

        let snapshot = engine.snapshot_cache();
        let snapshot_ref = snapshot.as_ref();
        let price = self.get_price_value(snapshot_ref);
        if price <= 0 {
            return Ok(());
        }

        let recipients = self.resolve_oracle_accounts(engine, block.header.index());
        if recipients.is_empty() {
            return Ok(());
        }

        let mut rewards: HashMap<UInt160, i64> = HashMap::new();
        for transaction in &block.transactions {
            for attribute in transaction.attributes() {
                if let TransactionAttribute::OracleResponse(response) = attribute {
                    let index = (response.id as usize) % recipients.len();
                    let account = recipients[index];
                    *rewards.entry(account).or_insert(0) += price;
                }
            }
        }

        if rewards.is_empty() {
            return Ok(());
        }

        let gas = GasToken::new();
        for (account, amount) in rewards {
            if amount <= 0 {
                continue;
            }
            let minted = BigInt::from(amount);
            gas.mint(engine, &account, &minted, false)?;
        }

        Ok(())
    }

    fn resolve_oracle_accounts(&self, engine: &mut ApplicationEngine, index: u32) -> Vec<UInt160> {
        let role_hash = RoleManagement::new().hash();
        let role_arg = vec![Role::Oracle as u8];
        let index_arg = index.to_le_bytes().to_vec();
        match engine.call_native_contract(role_hash, "getDesignatedByRole", &[role_arg, index_arg])
        {
            Ok(bytes) => self.parse_designated_accounts(&bytes),
            Err(err) => {
                log::debug!("failed to fetch designated oracle nodes: {}", err);
                Vec::new()
            }
        }
    }

    fn parse_designated_accounts(&self, bytes: &[u8]) -> Vec<UInt160> {
        if bytes.len() < 4 {
            return Vec::new();
        }

        let count = u32::from_le_bytes(bytes[0..4].try_into().unwrap_or([0; 4])) as usize;
        let mut offset = 4;
        let mut accounts = Vec::with_capacity(count);

        for _ in 0..count {
            if offset + 33 > bytes.len() {
                break;
            }
            let mut compressed = [0u8; 33];
            compressed.copy_from_slice(&bytes[offset..offset + 33]);
            if let Ok(point) =
                crate::cryptography::crypto_utils::ECPoint::decode_compressed(&compressed)
            {
                let script = Contract::create_signature_redeem_script(point);
                if let Ok(hash) = UInt160::from_bytes(&NeoHash::hash160(&script)) {
                    accounts.push(hash);
                }
            }
            offset += 33;
        }

        accounts
    }

    fn process_response(
        &self,
        engine: &mut ApplicationEngine,
        snapshot: &DataCache,
        response: TxOracleResponse,
    ) -> Result<Vec<u8>> {
        let TxOracleResponse { id, result, .. } = response;
        if result.len() > self.config.max_response_length {
            return Err(Error::invalid_operation(
                "Response data too long".to_string(),
            ));
        }
        let request = self
            .read_request(snapshot, id)?
            .ok_or_else(|| Error::invalid_operation("Request not found".to_string()))?;
        let url_hash = self.compute_url_hash(&request.url);
        self.delete_request(snapshot, id);
        self.remove_request_id(snapshot, &url_hash, id)?;
        self.emit_oracle_response(engine, id, &request)?;
        let _ = result;
        Ok(vec![1])
    }

    pub fn get_request(&self, snapshot: &DataCache, id: u64) -> Result<Option<OracleRequest>> {
        Ok(self
            .read_request(snapshot, id)?
            .map(|pending| self.to_public_request(&pending)))
    }

    fn to_public_request(&self, request: &PendingRequest) -> OracleRequest {
        OracleRequest::new(
            request.original_tx_id,
            request.gas_for_response,
            request.url.clone(),
            request.filter.clone(),
            request.callback_contract,
            request.callback_method.clone(),
            request.user_data.clone(),
        )
    }

    pub fn get_requests(&self, snapshot: &DataCache) -> Result<Vec<(u64, OracleRequest)>> {
        let prefix = StorageKey::create(Self::ID, PREFIX_REQUEST);
        let mut results = Vec::new();
        for (key, item) in snapshot.find(Some(&prefix), SeekDirection::Forward) {
            let id = match Self::parse_request_id(&key) {
                Some(value) => value,
                None => continue,
            };
            let request = self.deserialize_request(&item.get_value())?;
            results.push((id, self.to_public_request(&request)));
        }
        Ok(results)
    }

    pub fn get_requests_by_url(
        &self,
        snapshot: &DataCache,
        url: &str,
    ) -> Result<Vec<(u64, OracleRequest)>> {
        let hash = self.compute_url_hash(url);
        let mut results = Vec::new();
        for id in self.read_id_list(snapshot, &hash)? {
            if let Some(request) = self.read_request(snapshot, id)? {
                results.push((id, self.to_public_request(&request)));
            }
        }
        Ok(results)
    }

    pub fn get_price(&self, snapshot: &DataCache) -> i64 {
        self.get_price_value(snapshot)
    }
}

impl NativeContract for OracleContract {
    fn initialize(&self, engine: &mut ApplicationEngine) -> Result<()> {
        let snapshot_arc = engine.snapshot_cache();
        let snapshot = snapshot_arc.as_ref();
        let key = self.price_key();
        if snapshot.try_get(&key).is_none() {
            self.put_item(
                snapshot,
                key,
                StorageItem::from_bytes(DEFAULT_PRICE.to_le_bytes().to_vec()),
            );
        }
        Ok(())
    }

    fn id(&self) -> i32 {
        self.id
    }

    fn hash(&self) -> UInt160 {
        self.hash
    }

    fn name(&self) -> &str {
        "OracleContract"
    }

    fn methods(&self) -> &[NativeMethod] {
        &self.methods
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        self.invoke_method(engine, method, args)
    }

    fn post_persist(&self, engine: &mut ApplicationEngine) -> Result<()> {
        self.cleanup_persisted_responses(engine)?;
        self.reward_oracle_nodes(engine)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl Default for OracleContract {
    fn default() -> Self {
        Self::new()
    }
}
