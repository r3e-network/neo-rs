//! Blockchain RPC endpoints (`RpcServer.Blockchain.cs`).

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};

#[cfg(feature = "rest-server")]
use crate::rest_server::RestServerUtility;
use crate::rpc_server::model::block_hash_or_index::BlockHashOrIndex as RpcBlockHashOrIndex;
use crate::rpc_server::model::contract_name_or_hash_or_id::ContractNameOrHashOrId;
use crate::rpc_server::rpc_error::RpcError;
use crate::rpc_server::rpc_exception::RpcException;
use crate::rpc_server::rpc_method_attribute::RpcMethodDescriptor;
use crate::rpc_server::rpc_server::{RpcHandler, RpcServer};
use hex;
use neo_core::ledger::{
    block::Block as LedgerBlock, block_header::BlockHeader as LedgerBlockHeader,
};
use neo_core::neo_io::{BinaryWriter, Serializable};
use neo_core::network::p2p::payloads::{
    block::Block, header::Header, transaction::Transaction, witness::Witness as PayloadWitness,
};
use neo_core::persistence::seek_direction::SeekDirection;
use neo_core::persistence::IReadOnlyStoreGeneric;
use neo_core::smart_contract::contract_state::ContractState;
#[cfg(not(feature = "rest-server"))]
use neo_core::smart_contract::contract_state::{MethodToken, NefFile};
#[cfg(not(feature = "rest-server"))]
use neo_core::smart_contract::manifest::{
    ContractAbi, ContractEventDescriptor, ContractGroup, ContractManifest,
    ContractMethodDescriptor, ContractParameterDefinition, ContractPermission,
    ContractPermissionDescriptor, WildCardContainer,
};
use neo_core::smart_contract::native::{
    contract_management::ContractManagement,
    helpers::NativeHelpers,
    ledger_contract::{HashOrIndex, LedgerContract},
    NativeRegistry,
};
use neo_core::smart_contract::storage_key::StorageKey;
use neo_core::wallets::helper::Helper as WalletHelper;
use neo_core::{UInt160, UInt256, Witness as LedgerWitness};
use serde_json::{json, Map, Value};
use std::str::FromStr;
use std::sync::Arc;

pub struct RpcServerBlockchain;

impl RpcServerBlockchain {
    pub fn register_handlers() -> Vec<RpcHandler> {
        vec![
            Self::handler("getbestblockhash", Self::get_best_block_hash),
            Self::handler("getblockcount", Self::get_block_count),
            Self::handler("getblockheadercount", Self::get_block_header_count),
            Self::handler("getblockhash", Self::get_block_hash),
            Self::handler("getblock", Self::get_block),
            Self::handler("getblockheader", Self::get_block_header),
            Self::handler("getrawmempool", Self::get_raw_mem_pool),
            Self::handler("getrawtransaction", Self::get_raw_transaction),
            Self::handler("getcontractstate", Self::get_contract_state),
            Self::handler("getstorage", Self::get_storage),
            Self::handler("findstorage", Self::find_storage),
            Self::handler("getnativecontracts", Self::get_native_contracts),
            Self::handler("getnextblockvalidators", Self::get_next_block_validators),
            Self::handler("getcandidates", Self::get_candidates),
            Self::handler("gettransactionheight", Self::get_transaction_height),
            Self::handler("getcommittee", Self::get_committee),
        ]
    }

    fn handler(
        name: &'static str,
        func: fn(&RpcServer, &[Value]) -> Result<Value, RpcException>,
    ) -> RpcHandler {
        RpcHandler::new(
            RpcMethodDescriptor::new(name),
            Arc::new(move |server, params| func(server, params)),
        )
    }

    fn get_best_block_hash(server: &RpcServer, _params: &[Value]) -> Result<Value, RpcException> {
        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let hash = ledger.current_hash(&store).map_err(Self::internal_error)?;
        Ok(Value::String(hash.to_string()))
    }

    fn get_block_count(server: &RpcServer, _params: &[Value]) -> Result<Value, RpcException> {
        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let count = ledger
            .current_index(&store)
            .map_err(Self::internal_error)?
            .saturating_add(1);
        Ok(json!(count))
    }

    fn get_block_header_count(
        server: &RpcServer,
        _params: &[Value],
    ) -> Result<Value, RpcException> {
        let system = server.system();
        let header_cache = system.context().header_cache();
        let cache_height = header_cache.last().map(|header| header.index());
        let store = system.store_cache();
        let ledger = LedgerContract::new();
        let base_height = if let Some(index) = cache_height {
            index
        } else {
            ledger.current_index(&store).map_err(Self::internal_error)?
        };
        Ok(json!(base_height.saturating_add(1)))
    }

    fn get_block_hash(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let height = Self::expect_u32_param(params, 0, "getblockhash")?;
        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let current = ledger.current_index(&store).map_err(Self::internal_error)?;
        if height > current {
            return Err(RpcException::new(RpcError::unknown_height()));
        }

        let hash = ledger
            .get_block_hash_by_index(&store, height)
            .map_err(Self::internal_error)?
            .ok_or_else(|| RpcException::new(RpcError::unknown_block()))?;
        Ok(Value::String(hash.to_string()))
    }

    fn get_block(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let identifier = Self::parse_block_identifier(params, "getblock")?;
        let verbose = Self::parse_verbose(params.get(1))?;
        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let block = Self::fetch_payload_block(&ledger, &store, &identifier)?;
        if verbose {
            let current_index = ledger.current_index(&store).map_err(Self::internal_error)?;
            let next_hash = ledger
                .get_block_hash_by_index(&store, block.header.index().saturating_add(1))
                .map_err(Self::internal_error)?;
            return Ok(Self::block_to_json(
                server,
                &block,
                current_index,
                next_hash,
            ));
        }

        let bytes = Self::serialize_block(&block)?;
        Ok(Value::String(BASE64_STANDARD.encode(bytes)))
    }

    fn get_block_header(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let identifier = Self::parse_block_identifier(params, "getblockheader")?;
        let verbose = Self::parse_verbose(params.get(1))?;
        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let block = Self::fetch_payload_block(&ledger, &store, &identifier)?;
        let header = &block.header;
        if verbose {
            let current_index = ledger.current_index(&store).map_err(Self::internal_error)?;
            let next_hash = ledger
                .get_block_hash_by_index(&store, header.index().saturating_add(1))
                .map_err(Self::internal_error)?;
            return Ok(Self::header_to_json(
                server,
                header,
                current_index,
                next_hash,
            ));
        }

        let bytes = Self::serialize_header(header)?;
        Ok(Value::String(BASE64_STANDARD.encode(bytes)))
    }

    fn get_raw_mem_pool(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let include_unverified = match params.get(0) {
            None => false,
            Some(Value::Bool(value)) => *value,
            Some(Value::Number(number)) => match number.as_u64() {
                Some(0) => false,
                Some(1) => true,
                _ => {
                    return Err(RpcException::new(
                        RpcError::invalid_params()
                            .with_data("shouldGetUnverified must be a boolean"),
                    ))
                }
            },
            _ => {
                return Err(RpcException::new(
                    RpcError::invalid_params().with_data("shouldGetUnverified must be a boolean"),
                ))
            }
        };

        let pool_arc = server.system().context().memory_pool_handle();
        let pool = pool_arc.lock().map_err(|_| {
            RpcException::new(
                RpcError::internal_server_error().with_data("failed to access memory pool"),
            )
        })?;
        if !include_unverified {
            let hashes: Vec<Value> = pool
                .verified_transactions_vec()
                .iter()
                .map(|tx| Value::String(tx.hash().to_string()))
                .collect();
            return Ok(Value::Array(hashes));
        }

        let (verified, unverified) = pool.verified_and_unverified_transactions();

        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let height = ledger.current_index(&store).map_err(Self::internal_error)?;
        let verified_hashes: Vec<Value> = verified
            .iter()
            .map(|tx| Value::String(tx.hash().to_string()))
            .collect();
        let unverified_hashes: Vec<Value> = unverified
            .iter()
            .map(|tx| Value::String(tx.hash().to_string()))
            .collect();

        Ok(json!({
            "height": height,
            "verified": verified_hashes,
            "unverified": unverified_hashes,
        }))
    }

    fn get_raw_transaction(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let hash = Self::expect_hash_param(params, 0, "getrawtransaction")?;
        let verbose = Self::parse_verbose(params.get(1))?;
        let system = server.system();

        let pool_arc = system.context().memory_pool_handle();
        let tx_from_pool = pool_arc
            .lock()
            .map_err(|_| {
                RpcException::new(
                    RpcError::internal_server_error().with_data("failed to access memory pool"),
                )
            })?
            .try_get(&hash);

        if !verbose {
            if let Some(tx) = tx_from_pool {
                let bytes = Self::serialize_transaction(&tx)?;
                return Ok(Value::String(BASE64_STANDARD.encode(bytes)));
            }
        }

        let store = system.store_cache();
        let ledger = LedgerContract::new();
        let state = ledger
            .get_transaction_state(&store, &hash)
            .map_err(Self::internal_error)?;

        let transaction = tx_from_pool.or_else(|| state.as_ref().map(|s| s.transaction().clone()));
        let tx = transaction.ok_or_else(|| RpcException::new(RpcError::unknown_transaction()))?;

        if !verbose {
            let bytes = Self::serialize_transaction(&tx)?;
            return Ok(Value::String(BASE64_STANDARD.encode(bytes)));
        }

        let mut json = tx.to_json(system.settings());
        if let (Value::Object(ref mut obj), Some(state)) = (&mut json, state) {
            let block_index = state.block_index();
            let current_index = ledger.current_index(&store).map_err(Self::internal_error)?;
            let confirmations = current_index.saturating_sub(block_index).saturating_add(1);
            obj.insert("confirmations".to_string(), json!(confirmations));

            if let Some(block_hash) = ledger
                .get_block_hash_by_index(&store, block_index)
                .map_err(Self::internal_error)?
            {
                obj.insert(
                    "blockhash".to_string(),
                    Value::String(block_hash.to_string()),
                );

                if let Some(block) = ledger
                    .get_block(&store, HashOrIndex::Index(block_index))
                    .map_err(Self::internal_error)?
                {
                    obj.insert("blocktime".to_string(), json!(block.header.timestamp));
                }
            }
        }

        Ok(json)
    }

    fn get_contract_state(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let identifier = Self::parse_contract_identifier(params, "getcontractstate")?;
        let store = server.system().store_cache();
        let contract = Self::load_contract_state(&store, &identifier)?
            .ok_or_else(|| RpcException::new(RpcError::unknown_contract()))?;
        Ok(contract_state_to_json(&contract))
    }

    fn get_storage(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let identifier = Self::parse_contract_identifier(params, "getstorage")?;
        let key = params.get(1).and_then(Value::as_str).ok_or_else(|| {
            RpcException::new(
                RpcError::invalid_params().with_data("getstorage requires Base64 key parameter"),
            )
        })?;
        let key_bytes = BASE64_STANDARD.decode(key).map_err(|_| {
            RpcException::new(
                RpcError::invalid_params()
                    .with_data(format!("invalid Base64 storage key: {}", key)),
            )
        })?;

        let store = server.system().store_cache();
        let contract_id = Self::resolve_contract_id(&store, &identifier)?;
        let storage_key = StorageKey::new(contract_id, key_bytes);
        let value = store
            .get(&storage_key)
            .ok_or_else(|| RpcException::new(RpcError::unknown_storage_item()))?;
        Ok(Value::String(BASE64_STANDARD.encode(value.get_value())))
    }

    fn find_storage(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let identifier = Self::parse_contract_identifier(params, "findstorage")?;
        let prefix = params.get(1).and_then(Value::as_str).ok_or_else(|| {
            RpcException::new(
                RpcError::invalid_params()
                    .with_data("findstorage requires Base64 prefix parameter"),
            )
        })?;
        let prefix_bytes = BASE64_STANDARD.decode(prefix).map_err(|_| {
            RpcException::new(
                RpcError::invalid_params()
                    .with_data(format!("invalid Base64 storage prefix: {}", prefix)),
            )
        })?;
        let start = match params.get(2) {
            None => 0usize,
            Some(Value::Number(number)) => number
                .as_u64()
                .and_then(|value| usize::try_from(value).ok())
                .ok_or_else(|| {
                    RpcException::new(
                        RpcError::invalid_params()
                            .with_data("start index must be a non-negative integer"),
                    )
                })?,
            _ => {
                return Err(RpcException::new(
                    RpcError::invalid_params()
                        .with_data("start index must be a non-negative integer"),
                ))
            }
        };

        let store = server.system().store_cache();
        let contract_id = Self::resolve_contract_id(&store, &identifier)?;
        let prefix_key = StorageKey::new(contract_id, prefix_bytes);
        let iter = store.find(Some(&prefix_key), SeekDirection::Forward);

        let mut results = Vec::new();
        let mut skipped = 0usize;
        let mut truncated = false;
        let page_size = server.settings().find_storage_page_size;
        for (key, value) in iter {
            if key.id != contract_id {
                continue;
            }
            if skipped < start {
                skipped += 1;
                continue;
            }
            if results.len() >= page_size {
                truncated = true;
                break;
            }

            results.push(json!({
                "key": BASE64_STANDARD.encode(key.suffix()),
                "value": BASE64_STANDARD.encode(value.get_value()),
            }));
        }

        Ok(json!({
            "truncated": truncated,
            "next": start + results.len(),
            "results": results,
        }))
    }

    fn get_native_contracts(server: &RpcServer, _params: &[Value]) -> Result<Value, RpcException> {
        let store = server.system().store_cache();
        let registry = NativeRegistry::new();
        let mut contracts = Vec::new();
        for contract in registry.contracts() {
            if let Some(state) =
                ContractManagement::get_contract_from_store_cache(&store, &contract.hash())
                    .map_err(Self::internal_error)?
            {
                contracts.push(contract_state_to_json(&state));
            }
        }
        Ok(Value::Array(contracts))
    }

    fn get_next_block_validators(
        server: &RpcServer,
        _params: &[Value],
    ) -> Result<Value, RpcException> {
        let system = server.system();
        let settings = system.settings();
        let validators = NativeHelpers::get_next_block_validators(settings);
        let result: Vec<Value> = validators
            .iter()
            .map(|point| {
                json!({
                    "publickey": format!("0x{}", hex::encode(point.as_bytes())),
                    "votes": 0,
                })
            })
            .collect();
        Ok(Value::Array(result))
    }

    fn get_candidates(server: &RpcServer, _params: &[Value]) -> Result<Value, RpcException> {
        let system = server.system();
        let settings = system.settings();
        let next_validators = NativeHelpers::get_next_block_validators(settings);
        let candidates: Vec<Value> = next_validators
            .iter()
            .map(|point| {
                json!({
                    "publickey": format!("0x{}", hex::encode(point.as_bytes())),
                    "votes": "0",
                    "active": true,
                })
            })
            .collect();
        Ok(Value::Array(candidates))
    }

    fn get_transaction_height(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let hash = Self::expect_hash_param(params, 0, "gettransactionheight")?;
        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let state = ledger
            .get_transaction_state(&store, &hash)
            .map_err(Self::internal_error)?
            .ok_or_else(|| RpcException::new(RpcError::unknown_transaction()))?;
        Ok(json!(state.block_index()))
    }

    fn get_committee(server: &RpcServer, _params: &[Value]) -> Result<Value, RpcException> {
        let store = server.system().store_cache();
        let snapshot = store.data_cache();
        let neo = neo_core::smart_contract::native::neo_token::NeoToken::new();
        let committee = neo.committee_from_snapshot(snapshot).ok_or_else(|| {
            RpcException::new(
                RpcError::internal_server_error().with_data("committee not available"),
            )
        })?;
        let members: Vec<Value> = committee
            .iter()
            .map(|point| Value::String(format!("0x{}", hex::encode(point.as_bytes()))))
            .collect();
        Ok(Value::Array(members))
    }

    fn parse_block_identifier(
        params: &[Value],
        method: &str,
    ) -> Result<RpcBlockHashOrIndex, RpcException> {
        let token = params.get(0).ok_or_else(|| {
            RpcException::new(
                RpcError::invalid_params()
                    .with_data(format!("{method} requires at least one parameter")),
            )
        })?;

        match token {
            Value::Number(number) => {
                let value = number
                    .as_u64()
                    .and_then(|value| u32::try_from(value).ok())
                    .ok_or_else(|| {
                        RpcException::new(
                            RpcError::invalid_params()
                                .with_data(format!("{method} index is out of range")),
                        )
                    })?;
                Ok(RpcBlockHashOrIndex::from_index(value))
            }
            Value::String(text) => RpcBlockHashOrIndex::try_parse(text).ok_or_else(|| {
                RpcException::new(RpcError::invalid_params().with_data(format!(
                    "{} expects block hash or index, got '{}'",
                    method, text
                )))
            }),
            _ => Err(RpcException::new(RpcError::invalid_params().with_data(
                format!("{method} expects the first parameter to be hash or index"),
            ))),
        }
    }

    fn parse_verbose(arg: Option<&Value>) -> Result<bool, RpcException> {
        match arg {
            None => Ok(false),
            Some(Value::Bool(value)) => Ok(*value),
            Some(Value::Number(number)) => match number.as_u64() {
                Some(0) => Ok(false),
                Some(1) => Ok(true),
                _ => Err(RpcException::new(
                    RpcError::invalid_params().with_data("verbose flag must be a boolean or 0/1"),
                )),
            },
            _ => Err(RpcException::new(
                RpcError::invalid_params().with_data("verbose flag must be a boolean"),
            )),
        }
    }

    fn fetch_payload_block(
        ledger: &LedgerContract,
        store: &neo_core::persistence::StoreCache,
        identifier: &RpcBlockHashOrIndex,
    ) -> Result<Block, RpcException> {
        let selector = match identifier {
            RpcBlockHashOrIndex::Index(index) => HashOrIndex::Index(*index),
            RpcBlockHashOrIndex::Hash(hash) => HashOrIndex::Hash(*hash),
        };

        let ledger_block = ledger
            .get_block(store, selector)
            .map_err(Self::internal_error)?
            .ok_or_else(|| RpcException::new(RpcError::unknown_block()))?;
        Ok(Self::convert_ledger_block(&ledger_block))
    }

    fn block_to_json(
        server: &RpcServer,
        block: &Block,
        current_index: u32,
        next_hash: Option<UInt256>,
    ) -> Value {
        let mut json = Self::header_fields_to_map(server, &block.header, current_index, next_hash);
        json.insert("size".to_string(), json!(block.size()));
        let system = server.system();
        let settings = system.settings();
        let transactions: Vec<Value> = block
            .transactions
            .iter()
            .map(|tx| tx.to_json(settings))
            .collect();
        json.insert("tx".to_string(), Value::Array(transactions));
        Value::Object(json)
    }

    fn header_to_json(
        server: &RpcServer,
        header: &Header,
        current_index: u32,
        next_hash: Option<UInt256>,
    ) -> Value {
        Value::Object(Self::header_fields_to_map(
            server,
            header,
            current_index,
            next_hash,
        ))
    }

    fn header_fields_to_map(
        server: &RpcServer,
        header: &Header,
        current_index: u32,
        next_hash: Option<UInt256>,
    ) -> Map<String, Value> {
        let mut header_clone = header.clone();
        let hash = header_clone.hash();
        let mut json = Map::new();
        json.insert("hash".to_string(), Value::String(hash.to_string()));
        json.insert("size".to_string(), json!(header.size()));
        json.insert("version".to_string(), json!(header.version()));
        json.insert(
            "previousblockhash".to_string(),
            Value::String(header.prev_hash().to_string()),
        );
        json.insert(
            "merkleroot".to_string(),
            Value::String(header.merkle_root().to_string()),
        );
        json.insert("time".to_string(), json!(header.timestamp()));
        json.insert(
            "nonce".to_string(),
            Value::String(format!("{:016X}", header.nonce())),
        );
        json.insert("index".to_string(), json!(header.index()));
        json.insert("primary".to_string(), json!(header.primary_index()));
        let system = server.system();
        let address_version = system.settings().address_version;
        let next_consensus = WalletHelper::to_address(header.next_consensus(), address_version);
        json.insert("nextconsensus".to_string(), Value::String(next_consensus));
        json.insert(
            "witnesses".to_string(),
            Value::Array(vec![header.witness.to_json()]),
        );
        let confirmations = current_index.saturating_sub(header.index()) + 1;
        json.insert("confirmations".to_string(), json!(confirmations));
        if let Some(hash) = next_hash {
            json.insert("nextblockhash".to_string(), Value::String(hash.to_string()));
        }
        json
    }

    fn serialize_block(block: &Block) -> Result<Vec<u8>, RpcException> {
        let mut writer = BinaryWriter::new();
        block.serialize(&mut writer).map_err(Self::internal_error)?;
        Ok(writer.into_bytes())
    }

    fn serialize_header(header: &Header) -> Result<Vec<u8>, RpcException> {
        let mut writer = BinaryWriter::new();
        header
            .serialize(&mut writer)
            .map_err(Self::internal_error)?;
        Ok(writer.into_bytes())
    }

    fn serialize_transaction(tx: &Transaction) -> Result<Vec<u8>, RpcException> {
        let mut writer = BinaryWriter::new();
        tx.serialize(&mut writer).map_err(Self::internal_error)?;
        Ok(writer.into_bytes())
    }

    fn convert_ledger_block(block: &LedgerBlock) -> Block {
        Block {
            header: Self::convert_ledger_header(&block.header),
            transactions: block.transactions.clone(),
        }
    }

    fn convert_ledger_header(header: &LedgerBlockHeader) -> Header {
        let mut converted = Header::new();
        converted.set_version(header.version);
        converted.set_prev_hash(header.previous_hash);
        converted.set_merkle_root(header.merkle_root);
        converted.set_timestamp(header.timestamp);
        converted.set_nonce(header.nonce);
        converted.set_index(header.index);
        converted.set_primary_index(header.primary_index);
        converted.set_next_consensus(header.next_consensus);
        let witness = header
            .witnesses
            .get(0)
            .map(Self::convert_witness)
            .unwrap_or_else(PayloadWitness::new);
        converted.witness = witness;
        converted
    }

    fn convert_witness(witness: &LedgerWitness) -> PayloadWitness {
        PayloadWitness::new_with_scripts(
            witness.invocation_script.clone(),
            witness.verification_script.clone(),
        )
    }

    fn expect_u32_param(params: &[Value], index: usize, method: &str) -> Result<u32, RpcException> {
        params
            .get(index)
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| {
                RpcException::new(RpcError::invalid_params().with_data(format!(
                    "{} expects numeric parameter {}",
                    method,
                    index + 1
                )))
            })
    }

    fn expect_hash_param(
        params: &[Value],
        index: usize,
        method: &str,
    ) -> Result<UInt256, RpcException> {
        params
            .get(index)
            .and_then(Value::as_str)
            .ok_or_else(|| {
                RpcException::new(RpcError::invalid_params().with_data(format!(
                    "{} expects string parameter {}",
                    method,
                    index + 1
                )))
            })
            .and_then(|text| {
                UInt256::from_str(text).map_err(|err| {
                    RpcException::new(
                        RpcError::invalid_params()
                            .with_data(format!("invalid hash '{}': {}", text, err)),
                    )
                })
            })
    }

    fn parse_contract_identifier(
        params: &[Value],
        method: &str,
    ) -> Result<ContractNameOrHashOrId, RpcException> {
        let token = params.get(0).ok_or_else(|| {
            RpcException::new(
                RpcError::invalid_params()
                    .with_data(format!("{method} requires at least one parameter")),
            )
        })?;

        match token {
            Value::Number(number) => {
                let value = number
                    .as_i64()
                    .and_then(|value| i32::try_from(value).ok())
                    .ok_or_else(|| {
                        RpcException::new(
                            RpcError::invalid_params()
                                .with_data(format!("{method} contract id out of range")),
                        )
                    })?;
                Ok(ContractNameOrHashOrId::from_id(value))
            }
            Value::String(text) => ContractNameOrHashOrId::try_parse(text).ok_or_else(|| {
                RpcException::new(
                    RpcError::invalid_params()
                        .with_data(format!("invalid contract identifier '{}'", text)),
                )
            }),
            _ => Err(RpcException::new(RpcError::invalid_params().with_data(
                format!("{method} expects contract identifier as string or integer"),
            ))),
        }
    }

    fn load_contract_state(
        store: &neo_core::persistence::StoreCache,
        identifier: &ContractNameOrHashOrId,
    ) -> Result<Option<ContractState>, RpcException> {
        match identifier {
            ContractNameOrHashOrId::Id(id) => {
                ContractManagement::get_contract_by_id_from_store_cache(store, *id)
                    .map_err(Self::internal_error)
            }
            ContractNameOrHashOrId::Hash(hash) => {
                ContractManagement::get_contract_from_store_cache(store, hash)
                    .map_err(Self::internal_error)
            }
            ContractNameOrHashOrId::Name(name) => {
                let hash = Self::contract_name_to_hash(name)?;
                ContractManagement::get_contract_from_store_cache(store, &hash)
                    .map_err(Self::internal_error)
            }
        }
    }

    fn contract_name_to_hash(name: &str) -> Result<UInt160, RpcException> {
        let registry = NativeRegistry::new();
        if let Some(contract) = registry.get_by_name(name) {
            return Ok(contract.hash());
        }
        UInt160::from_str(name).map_err(|err| {
            RpcException::new(
                RpcError::invalid_params()
                    .with_data(format!("invalid contract identifier '{}': {}", name, err)),
            )
        })
    }

    fn resolve_contract_id(
        store: &neo_core::persistence::StoreCache,
        identifier: &ContractNameOrHashOrId,
    ) -> Result<i32, RpcException> {
        match identifier {
            ContractNameOrHashOrId::Id(id) => {
                let state = ContractManagement::get_contract_by_id_from_store_cache(store, *id)
                    .map_err(Self::internal_error)?;
                state
                    .map(|contract| contract.id)
                    .ok_or_else(|| RpcException::new(RpcError::unknown_contract()))
            }
            _ => {
                let contract = Self::load_contract_state(store, identifier)?
                    .ok_or_else(|| RpcException::new(RpcError::unknown_contract()))?;
                Ok(contract.id)
            }
        }
    }

    fn internal_error(err: impl ToString) -> RpcException {
        RpcException::new(RpcError::internal_server_error().with_data(err.to_string()))
    }
}

#[cfg(feature = "rest-server")]
fn contract_state_to_json(contract: &ContractState) -> Value {
    RestServerUtility::contract_state_to_j_token(contract)
}

#[cfg(not(feature = "rest-server"))]
fn contract_state_to_json(contract: &ContractState) -> Value {
    json!({
        "Id": contract.id,
        "UpdateCounter": contract.update_counter,
        "Name": contract.manifest.name,
        "Hash": contract.hash.to_string(),
        "Manifest": manifest_to_json(&contract.manifest),
        "NefFile": nef_to_json(&contract.nef),
    })
}

#[cfg(not(feature = "rest-server"))]
fn manifest_to_json(manifest: &ContractManifest) -> Value {
    let groups: Vec<Value> = manifest.groups.iter().map(group_to_json).collect();
    let permissions: Vec<Value> = manifest
        .permissions
        .iter()
        .map(permission_to_json)
        .collect();
    let trusts: Vec<Value> = match &manifest.trusts {
        WildCardContainer::Wildcard => vec![Value::String("*".to_string())],
        WildCardContainer::List(items) => items.iter().map(permission_descriptor_to_json).collect(),
    };

    json!({
        "Name": manifest.name,
        "Abi": abi_to_json(&manifest.abi),
        "Groups": groups,
        "Permissions": permissions,
        "Trusts": trusts,
        "SupportedStandards": manifest.supported_standards,
        "Extra": manifest.extra.clone().unwrap_or(Value::Null),
    })
}

#[cfg(not(feature = "rest-server"))]
fn abi_to_json(abi: &ContractAbi) -> Value {
    let methods: Vec<Value> = abi.methods.iter().map(method_to_json).collect();
    let events: Vec<Value> = abi.events.iter().map(event_to_json).collect();
    json!({
        "Methods": methods,
        "Events": events,
    })
}

#[cfg(not(feature = "rest-server"))]
fn method_to_json(method: &ContractMethodDescriptor) -> Value {
    let parameters: Vec<Value> = method.parameters.iter().map(parameter_to_json).collect();
    json!({
        "Name": method.name,
        "Safe": method.safe,
        "Offset": method.offset,
        "Parameters": parameters,
        "ReturnType": method.return_type,
    })
}

#[cfg(not(feature = "rest-server"))]
fn parameter_to_json(parameter: &ContractParameterDefinition) -> Value {
    json!({
        "Type": parameter.param_type,
        "Name": parameter.name,
    })
}

#[cfg(not(feature = "rest-server"))]
fn group_to_json(group: &ContractGroup) -> Value {
    json!({
        "PubKey": encode_with_0x(group.pub_key.as_bytes()),
        "Signature": BASE64_STANDARD.encode(&group.signature),
    })
}

#[cfg(not(feature = "rest-server"))]
fn permission_to_json(permission: &ContractPermission) -> Value {
    let methods = match &permission.methods {
        WildCardContainer::Wildcard => Value::String("*".to_string()),
        WildCardContainer::List(entries) => json!(entries),
    };
    json!({
        "Contract": permission_descriptor_to_json(&permission.contract),
        "Methods": methods,
    })
}

#[cfg(not(feature = "rest-server"))]
fn permission_descriptor_to_json(descriptor: &ContractPermissionDescriptor) -> Value {
    match descriptor {
        ContractPermissionDescriptor::Wildcard => Value::String("*".to_string()),
        ContractPermissionDescriptor::Group(group) => {
            json!({ "Group": encode_with_0x(group.as_bytes()) })
        }
        ContractPermissionDescriptor::Hash(hash) => json!({ "Hash": hash.to_string() }),
    }
}

#[cfg(not(feature = "rest-server"))]
fn event_to_json(event: &ContractEventDescriptor) -> Value {
    let parameters: Vec<Value> = event.parameters.iter().map(parameter_to_json).collect();
    json!({
        "Name": event.name,
        "Parameters": parameters,
    })
}

#[cfg(not(feature = "rest-server"))]
fn nef_to_json(nef: &NefFile) -> Value {
    let tokens: Vec<Value> = nef.tokens.iter().map(token_to_json).collect();
    json!({
        "Checksum": nef.checksum,
        "Compiler": nef.compiler,
        "Script": BASE64_STANDARD.encode(&nef.script),
        "Source": nef.source,
        "Tokens": tokens,
    })
}

#[cfg(not(feature = "rest-server"))]
fn token_to_json(token: &MethodToken) -> Value {
    json!({
        "Hash": token.hash.to_string(),
        "Method": token.method,
        "CallFlags": token.call_flags,
        "ParametersCount": token.parameters_count,
        "HasReturnValue": token.has_return_value,
    })
}

#[cfg(not(feature = "rest-server"))]
fn encode_with_0x(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}
