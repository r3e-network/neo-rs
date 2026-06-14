//! Blockchain RPC endpoints (`RpcServer.Blockchain.cs`).

use crate::client::models::RpcContractState;
use crate::server::model::block_hash_or_index::BlockHashOrIndex as RpcBlockHashOrIndex;
use crate::server::model::contract_name_or_hash_or_id::ContractNameOrHashOrId;
use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::{internal_error, serialize_to_base64};
use crate::server::rpc_server::{RpcHandler, RpcServer};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use hex;
use neo_execution::contract_state::ContractState;
use neo_io::Serializable;
use neo_native_contracts::{LedgerContract, contract_management::ContractManagement};
use neo_payloads::{Header, block::Block};
use neo_primitives::{UInt160, UInt256};
use neo_storage::StorageKey;
use neo_storage::persistence::SeekDirection;
use num_traits::ToPrimitive;

use crate::server::ledger_queries;
use crate::server::native_queries;
use serde_json::{Map, Value, json};
use std::str::FromStr;

pub struct RpcServerBlockchain;

impl RpcServerBlockchain {
    pub fn register_handlers() -> Vec<RpcHandler> {
        super::rpc_handlers![
            "getbestblockhash" => Self::get_best_block_hash,
            "getblockcount" => Self::get_block_count,
            "getblockheadercount" => Self::get_block_header_count,
            "getblockhash" => Self::get_block_hash,
            "getblock" => Self::get_block,
            "getblockheader" => Self::get_block_header,
            "getblocksysfee" => Self::get_block_sys_fee,
            "getrawmempool" => Self::get_raw_mem_pool,
            "getrawtransaction" => Self::get_raw_transaction,
            "getcontractstate" => Self::get_contract_state,
            "getstorage" => Self::get_storage,
            "findstorage" => Self::find_storage,
            "getnativecontracts" => Self::get_native_contracts,
            "getnextblockvalidators" => Self::get_next_block_validators,
            "getcandidates" => Self::get_candidates,
            "gettransactionheight" => Self::get_transaction_height,
            "getcommittee" => Self::get_committee,
        ]
    }

    fn get_best_block_hash(server: &RpcServer, _params: &[Value]) -> Result<Value, RpcException> {
        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let hash = ledger
            .current_hash(store.data_cache())
            .map_err(internal_error)?;
        Ok(Value::String(hash.to_string()))
    }

    fn get_block_count(server: &RpcServer, _params: &[Value]) -> Result<Value, RpcException> {
        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let count = ledger
            .current_index(store.data_cache())
            .map_err(internal_error)?
            .saturating_add(1);
        Ok(json!(count))
    }

    fn get_block_header_count(
        server: &RpcServer,
        _params: &[Value],
    ) -> Result<Value, RpcException> {
        let system = server.system();
        let header_cache = system.header_cache();
        let cache_height = header_cache.last().map(|header| header.index());
        let store = system.store_cache();
        let ledger = LedgerContract::new();
        let base_height = if let Some(index) = cache_height {
            index
        } else {
            ledger
                .current_index(store.data_cache())
                .map_err(internal_error)?
        };
        Ok(json!(base_height.saturating_add(1)))
    }

    fn get_block_hash(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let height = Self::expect_u32_param(params, 0, "getblockhash")?;
        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let current = ledger
            .current_index(store.data_cache())
            .map_err(internal_error)?;
        if height > current {
            return Err(RpcException::from(RpcError::unknown_height()));
        }

        let hash = ledger
            .get_block_hash(store.data_cache(), height)
            .map_err(internal_error)?
            .ok_or_else(|| RpcException::from(RpcError::unknown_block()))?;
        Ok(Value::String(hash.to_string()))
    }

    fn get_block(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let identifier = Self::parse_block_identifier(params, "getblock")?;
        let verbose = Self::parse_verbose(params.get(1))?;
        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let block = Self::fetch_payload_block(&store, &identifier)?;
        if verbose {
            let current_index = ledger
                .current_index(store.data_cache())
                .map_err(internal_error)?;
            let next_hash = ledger
                .get_block_hash(store.data_cache(), block.header.index().saturating_add(1))
                .map_err(internal_error)?;
            return Ok(Self::block_to_json(
                server,
                &block,
                current_index,
                next_hash,
            ));
        }

        Ok(Value::String(serialize_to_base64(&block)?))
    }

    fn get_block_header(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let identifier = Self::parse_block_identifier(params, "getblockheader")?;
        let verbose = Self::parse_verbose(params.get(1))?;
        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let block = Self::fetch_payload_block(&store, &identifier)?;
        let header = &block.header;
        if verbose {
            let current_index = ledger
                .current_index(store.data_cache())
                .map_err(internal_error)?;
            let next_hash = ledger
                .get_block_hash(store.data_cache(), header.index().saturating_add(1))
                .map_err(internal_error)?;
            return Ok(Self::header_to_json(
                server,
                header,
                current_index,
                next_hash,
            ));
        }

        Ok(Value::String(serialize_to_base64(header)?))
    }

    fn get_block_sys_fee(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let height = Self::expect_u32_param(params, 0, "getblocksysfee")?;
        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let current = ledger
            .current_index(store.data_cache())
            .map_err(internal_error)?;
        if height > current {
            return Err(RpcException::from(RpcError::unknown_height()));
        }

        let block =
            ledger_queries::get_full_block(store.data_cache(), &RpcBlockHashOrIndex::Index(height))
                .map_err(internal_error)?
                .ok_or_else(|| RpcException::from(RpcError::unknown_block()))?;

        let system_fee: i64 = block
            .transactions
            .iter()
            .map(neo_payloads::Transaction::system_fee)
            .sum();
        Ok(Value::String(system_fee.to_string()))
    }

    fn get_raw_mem_pool(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let include_unverified = match params.first() {
            None => false,
            Some(Value::Bool(value)) => *value,
            Some(Value::Number(number)) => match number.as_u64() {
                Some(0) => false,
                Some(1) => true,
                _ => {
                    return Err(RpcException::from(
                        RpcError::invalid_params()
                            .with_data("shouldGetUnverified must be a boolean"),
                    ));
                }
            },
            _ => {
                return Err(RpcException::from(
                    RpcError::invalid_params().with_data("shouldGetUnverified must be a boolean"),
                ));
            }
        };

        let pool = server.system().mempool();
        if !include_unverified {
            let hashes: Vec<Value> = pool
                .verified_snapshot()
                .iter()
                .map(|item| Value::String(item.hash().to_string()))
                .collect();
            return Ok(Value::Array(hashes));
        }

        let (verified, unverified) = (pool.verified_snapshot(), pool.unverified_snapshot());

        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let height = ledger
            .current_index(store.data_cache())
            .map_err(internal_error)?;
        let verified_hashes: Vec<Value> = verified
            .iter()
            .map(|item| Value::String(item.hash().to_string()))
            .collect();
        let unverified_hashes: Vec<Value> = unverified
            .iter()
            .map(|item| Value::String(item.hash().to_string()))
            .collect();

        Ok(json!({
            "height": height,
            "verified": verified_hashes,
            "unverified": unverified_hashes}))
    }

    fn get_raw_transaction(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let hash = Self::expect_hash_param(params, 0, "getrawtransaction")?;
        let verbose = Self::parse_verbose(params.get(1))?;
        let system = server.system();

        let tx_from_pool = system.mempool().get(&hash);

        if !verbose {
            if let Some(item) = tx_from_pool {
                return Ok(Value::String(serialize_to_base64(
                    item.transaction.as_ref(),
                )?));
            }
        }

        let store = system.store_cache();
        let ledger = LedgerContract::new();
        let state = ledger
            .get_transaction_state(store.data_cache(), &hash)
            .map_err(internal_error)?;

        // Convert Arc<Transaction> to Transaction for uniform handling
        let transaction = tx_from_pool
            .map(|item| (*item.transaction).clone())
            .or_else(|| state.as_ref().and_then(|s| s.transaction.clone()));
        let tx = transaction.ok_or_else(|| RpcException::from(RpcError::unknown_transaction()))?;

        if !verbose {
            return Ok(Value::String(serialize_to_base64(&tx)?));
        }

        let settings = system.settings();
        let mut json = tx.to_json(&settings);
        if let (Value::Object(obj), Some(state)) = (&mut json, state) {
            let block_index = state.block_index();
            let current_index = ledger
                .current_index(store.data_cache())
                .map_err(internal_error)?;
            let confirmations = current_index.saturating_sub(block_index).saturating_add(1);
            obj.insert("confirmations".to_string(), json!(confirmations));

            // C# GetRawTransaction verbose adds only blockhash, confirmations and
            // blocktime to Transaction.ToJson (RpcServer.Blockchain.cs:373-381);
            // it does NOT add a vmstate field (that belongs to getapplicationlog).
            // Emitting it here surprises strict clients / response-diff tooling.

            if let Some(block_hash) = ledger
                .get_block_hash(store.data_cache(), block_index)
                .map_err(internal_error)?
            {
                obj.insert(
                    "blockhash".to_string(),
                    Value::String(block_hash.to_string()),
                );

                if let Some(block) = ledger
                    .get_trimmed_block(store.data_cache(), &block_hash)
                    .map_err(internal_error)?
                {
                    obj.insert("blocktime".to_string(), json!(block.header.timestamp()));
                }
            }
        }

        Ok(json)
    }

    fn get_contract_state(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let identifier = Self::parse_contract_identifier(params, "getcontractstate")?;
        let store = server.system().store_cache();
        let contract = Self::load_contract_state(&store, &identifier)?
            .ok_or_else(|| RpcException::from(RpcError::unknown_contract()))?;
        Ok(contract_state_to_json(&contract))
    }

    fn get_storage(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let identifier = Self::parse_contract_identifier(params, "getstorage")?;
        let key = params.get(1).and_then(Value::as_str).ok_or_else(|| {
            RpcException::from(
                RpcError::invalid_params().with_data("getstorage requires Base64 key parameter"),
            )
        })?;
        let key_bytes = BASE64_STANDARD.decode(key).map_err(|_| {
            RpcException::from(
                RpcError::invalid_params().with_data(format!("invalid Base64 storage key: {key}")),
            )
        })?;

        let store = server.system().store_cache();
        let contract_id = Self::resolve_contract_id(&store, &identifier)?;
        let storage_key = StorageKey::new(contract_id, key_bytes);
        let value = store
            .get(&storage_key)
            .ok_or_else(|| RpcException::from(RpcError::unknown_storage_item()))?;
        Ok(Value::String(BASE64_STANDARD.encode(&*value.value_bytes())))
    }

    fn find_storage(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let identifier = Self::parse_contract_identifier(params, "findstorage")?;
        let prefix = params.get(1).and_then(Value::as_str).ok_or_else(|| {
            RpcException::from(
                RpcError::invalid_params()
                    .with_data("findstorage requires Base64 prefix parameter"),
            )
        })?;
        let prefix_bytes = BASE64_STANDARD.decode(prefix).map_err(|_| {
            RpcException::from(
                RpcError::invalid_params()
                    .with_data(format!("invalid Base64 storage prefix: {prefix}")),
            )
        })?;
        let start = match params.get(2) {
            None => 0usize,
            Some(Value::Number(number)) => number
                .as_u64()
                .and_then(|value| usize::try_from(value).ok())
                .ok_or_else(|| {
                    RpcException::from(
                        RpcError::invalid_params()
                            .with_data("start index must be a non-negative integer"),
                    )
                })?,
            _ => {
                return Err(RpcException::from(
                    RpcError::invalid_params()
                        .with_data("start index must be a non-negative integer"),
                ));
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
                "value": BASE64_STANDARD.encode(&*value.value_bytes())}));
        }

        Ok(json!({
            "truncated": truncated,
            "next": start + results.len(),
            "results": results}))
    }

    fn get_native_contracts(server: &RpcServer, _params: &[Value]) -> Result<Value, RpcException> {
        let system = server.system();
        let store = system.store_cache();
        let settings = system.settings();
        let ledger = LedgerContract::new();
        let block_height = ledger
            .current_index(store.data_cache())
            .map_err(internal_error)?;

        let registry = crate::server::native_queries::native_registry();
        let mut contract_states = Vec::new();

        for contract in registry.contracts() {
            let state = ContractManagement::get_contract_from_snapshot(
                store.data_cache(),
                &contract.hash(),
            )
            .map_err(internal_error)?
            .or_else(|| contract.contract_state(&settings, block_height));

            if let Some(state) = state {
                contract_states.push(state);
            }
        }

        contract_states.sort_by_key(|state| std::cmp::Reverse(state.id));

        Ok(Value::Array(
            contract_states.iter().map(contract_state_to_json).collect(),
        ))
    }

    fn get_next_block_validators(
        server: &RpcServer,
        _params: &[Value],
    ) -> Result<Value, RpcException> {
        let system = server.system();
        let store = system.store_cache();
        let snapshot = std::sync::Arc::new(store.data_cache().clone());
        let neo_hash = neo_native_contracts::NeoToken::script_hash();
        let validators = native_queries::neo_next_block_validators(
            server,
            std::sync::Arc::clone(&snapshot),
            &neo_hash,
        )
        .map_err(internal_error)?;
        let mut result = Vec::with_capacity(validators.len());
        for point in validators {
            let votes = native_queries::neo_candidate_vote(
                server,
                std::sync::Arc::clone(&snapshot),
                &neo_hash,
                &point,
            )
            .map_err(internal_error)?;
            let votes_value = votes.to_i64().ok_or_else(|| {
                RpcException::from(
                    RpcError::internal_server_error().with_data("candidate vote out of range"),
                )
            })?;
            result.push(json!({
                "publickey": hex::encode(&point),
                "votes": votes_value}));
        }
        Ok(Value::Array(result))
    }

    fn get_candidates(server: &RpcServer, _params: &[Value]) -> Result<Value, RpcException> {
        let system = server.system();
        let store = system.store_cache();
        let snapshot = std::sync::Arc::new(store.data_cache().clone());
        let neo_hash = neo_native_contracts::NeoToken::script_hash();
        let candidates =
            native_queries::neo_candidates(server, std::sync::Arc::clone(&snapshot), &neo_hash)
                .map_err(|_| {
                    RpcException::from(
                        RpcError::internal_server_error().with_data("Can't get candidates."),
                    )
                })?;
        let validators = native_queries::neo_next_block_validators(
            server,
            std::sync::Arc::clone(&snapshot),
            &neo_hash,
        )
        .map_err(|_| {
            RpcException::from(
                RpcError::internal_server_error().with_data("Can't get next block validators."),
            )
        })?;
        // C# `NeoToken.GetCandidatesInternal` also drops candidates whose
        // signature-contract script hash is blocked in the PolicyContract.
        // The native `getCandidates` read in this tree does not yet carry
        // that filter, so apply it here; once the native side gains it the
        // filter below becomes an idempotent no-op.
        let policy_hash = neo_native_contracts::PolicyContract::script_hash();
        let mut result = Vec::with_capacity(candidates.len());
        for (point, votes) in &candidates {
            let ec_point = neo_crypto::ECPoint::from_bytes(point).map_err(|_| {
                RpcException::from(
                    RpcError::internal_server_error().with_data("Can't get candidates."),
                )
            })?;
            let account =
                neo_execution::Contract::create_signature_contract(ec_point).script_hash();
            let blocked = native_queries::policy_is_blocked(
                server,
                std::sync::Arc::clone(&snapshot),
                &policy_hash,
                &account,
            )
            .map_err(|_| {
                RpcException::from(
                    RpcError::internal_server_error().with_data("Can't get candidates."),
                )
            })?;
            if blocked {
                continue;
            }
            let active = validators.iter().any(|validator| validator == point);
            result.push(json!({
                "publickey": hex::encode(point),
                "votes": votes.to_string(),
                "active": active}));
        }
        Ok(Value::Array(result))
    }

    fn get_transaction_height(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let hash = Self::expect_hash_param(params, 0, "gettransactionheight")?;
        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let state = ledger
            .get_transaction_state(store.data_cache(), &hash)
            .map_err(internal_error)?
            .ok_or_else(|| RpcException::from(RpcError::unknown_transaction()))?;
        Ok(json!(state.block_index()))
    }

    fn get_committee(server: &RpcServer, _params: &[Value]) -> Result<Value, RpcException> {
        let store = server.system().store_cache();
        let snapshot = std::sync::Arc::new(store.data_cache().clone());
        let neo_hash = neo_native_contracts::NeoToken::script_hash();
        let committee =
            native_queries::neo_committee(server, snapshot, &neo_hash).map_err(|err| {
                RpcException::from(
                    RpcError::internal_server_error()
                        .with_data(format!("committee not available: {err}")),
                )
            })?;
        let members: Vec<Value> = committee
            .iter()
            .map(|point| Value::String(hex::encode(point)))
            .collect();
        Ok(Value::Array(members))
    }

    fn parse_block_identifier(
        params: &[Value],
        method: &str,
    ) -> Result<RpcBlockHashOrIndex, RpcException> {
        let token = params.first().ok_or_else(|| {
            RpcException::from(
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
                        RpcException::from(
                            RpcError::invalid_params()
                                .with_data(format!("{method} index is out of range")),
                        )
                    })?;
                Ok(RpcBlockHashOrIndex::from_index(value))
            }
            Value::String(text) => RpcBlockHashOrIndex::try_parse(text).ok_or_else(|| {
                RpcException::from(RpcError::invalid_params().with_data(format!(
                    "{method} expects block hash or index, got '{text}'"
                )))
            }),
            _ => Err(RpcException::from(RpcError::invalid_params().with_data(
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
                _ => Err(RpcException::from(
                    RpcError::invalid_params().with_data("verbose flag must be a boolean or 0/1"),
                )),
            },
            _ => Err(RpcException::from(
                RpcError::invalid_params().with_data("verbose flag must be a boolean"),
            )),
        }
    }

    fn fetch_payload_block(
        store: &neo_storage::persistence::StoreCache,
        identifier: &RpcBlockHashOrIndex,
    ) -> Result<Block, RpcException> {
        ledger_queries::get_full_block(store.data_cache(), identifier)
            .map_err(internal_error)?
            .ok_or_else(|| RpcException::from(RpcError::unknown_block()))
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
            .map(|tx| tx.to_json(&settings))
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
        // Canonical header wire-JSON is owned by neo-core `Header::to_json`
        // (single source of truth shared with the RPC client); the server adds
        // only the contextual confirmations / nextblockhash on top.
        let system = server.system();
        let settings = system.settings();
        let mut json = header.to_json(&settings);
        let confirmations = current_index.saturating_sub(header.index()) + 1;
        json.insert("confirmations".to_string(), json!(confirmations));
        if let Some(hash) = next_hash {
            json.insert("nextblockhash".to_string(), Value::String(hash.to_string()));
        }
        json
    }

    fn expect_u32_param(params: &[Value], index: usize, method: &str) -> Result<u32, RpcException> {
        params
            .get(index)
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| {
                RpcException::from(RpcError::invalid_params().with_data(format!(
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
                RpcException::from(RpcError::invalid_params().with_data(format!(
                    "{} expects string parameter {}",
                    method,
                    index + 1
                )))
            })
            .and_then(|text| {
                UInt256::from_str(text).map_err(|err| {
                    RpcException::from(
                        RpcError::invalid_params()
                            .with_data(format!("invalid hash '{text}': {err}")),
                    )
                })
            })
    }

    fn parse_contract_identifier(
        params: &[Value],
        method: &str,
    ) -> Result<ContractNameOrHashOrId, RpcException> {
        let token = params.first().ok_or_else(|| {
            RpcException::from(
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
                        RpcException::from(
                            RpcError::invalid_params()
                                .with_data(format!("{method} contract id out of range")),
                        )
                    })?;
                Ok(ContractNameOrHashOrId::from_id(value))
            }
            Value::String(text) => ContractNameOrHashOrId::try_parse(text).ok_or_else(|| {
                RpcException::from(
                    RpcError::invalid_params()
                        .with_data(format!("invalid contract identifier '{text}'")),
                )
            }),
            _ => Err(RpcException::from(RpcError::invalid_params().with_data(
                format!("{method} expects contract identifier as string or integer"),
            ))),
        }
    }

    fn load_contract_state(
        store: &neo_storage::persistence::StoreCache,
        identifier: &ContractNameOrHashOrId,
    ) -> Result<Option<ContractState>, RpcException> {
        match identifier {
            ContractNameOrHashOrId::Id(id) => {
                ContractManagement::get_contract_by_id_from_snapshot(store.data_cache(), *id)
                    .map_err(internal_error)
            }
            ContractNameOrHashOrId::Hash(hash) => {
                ContractManagement::get_contract_from_snapshot(store.data_cache(), hash)
                    .map_err(internal_error)
            }
            ContractNameOrHashOrId::Name(name) => {
                let hash = Self::contract_name_to_hash(name)?;
                ContractManagement::get_contract_from_snapshot(store.data_cache(), &hash)
                    .map_err(internal_error)
            }
        }
    }

    fn contract_name_to_hash(name: &str) -> Result<UInt160, RpcException> {
        let registry = crate::server::native_queries::native_registry();
        if let Some(contract) = registry.get_by_name(name) {
            return Ok(contract.hash());
        }
        UInt160::from_str(name).map_err(|err| {
            RpcException::from(
                RpcError::invalid_params()
                    .with_data(format!("invalid contract identifier '{name}': {err}")),
            )
        })
    }

    fn resolve_contract_id(
        store: &neo_storage::persistence::StoreCache,
        identifier: &ContractNameOrHashOrId,
    ) -> Result<i32, RpcException> {
        if let ContractNameOrHashOrId::Id(id) = identifier {
            let state =
                ContractManagement::get_contract_by_id_from_snapshot(store.data_cache(), *id)
                    .map_err(internal_error)?;
            state
                .map(|contract| contract.id)
                .ok_or_else(|| RpcException::from(RpcError::unknown_contract()))
        } else {
            let contract = Self::load_contract_state(store, identifier)?
                .ok_or_else(|| RpcException::from(RpcError::unknown_contract()))?;
            Ok(contract.id)
        }
    }
}

fn contract_state_to_json(contract: &ContractState) -> Value {
    let rpc_contract = RpcContractState {
        contract_state: contract.clone(),
    };

    match rpc_contract.to_json() {
        Ok(jobj) => serde_json::from_str(&jobj.to_string())
            .unwrap_or_else(|err| json!({"error": err.to_string()})),
        Err(err) => json!({"error": err.to_string()}),
    }
}

#[cfg(test)]
mod tests;
