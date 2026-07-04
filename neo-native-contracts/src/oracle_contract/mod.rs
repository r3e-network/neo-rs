//! # neo-native-contracts::oracle_contract
//!
//! Native Oracle contract request, response, and fee behavior.
//!
//! ## Boundary
//!
//! This module belongs to `neo-native-contracts`. This execution-domain crate
//! owns native contract logic and storage codecs and must not own node startup,
//! RPC transport, or P2P sync.
//!
//! ## Contents
//!
//! - `metadata`: Native contract metadata and descriptor helpers.
//! - `request`: oracle request records and lifecycle helpers.
//! - `storage`: Storage contexts, key builders, and storage item helpers for
//!   execution.
//! - `tests`: Module-local tests and regression coverage.

use crate::hashes::ORACLE_CONTRACT_HASH;
use neo_config::{Hardfork, ProtocolSettings};
use neo_error::{CoreError, CoreResult};
use neo_execution::application_engine_contract::NativeArgNullMask;
use neo_execution::native_contract::OracleRequestDetails;
use neo_execution::{ApplicationEngine, Contract, NativeContract, NativeEvent, NativeMethod};
use neo_payloads::Transaction;
use neo_primitives::UInt160;
use neo_serialization::BinarySerializer;
use neo_storage::StorageItem;
use neo_storage::persistence::DataCache;
use neo_vm::StackItem;
use neo_vm_rs::ExecutionEngineLimits;
use num_bigint::BigInt;

mod metadata;
mod request;
mod storage;

pub use request::OracleRequest;

/// C# `OracleContract.MaxUrlLength` (strict-UTF8 bytes).
const MAX_URL_LENGTH: usize = 256;
/// C# `OracleContract.MaxFilterLength` (strict-UTF8 bytes).
const MAX_FILTER_LENGTH: usize = 128;
/// C# `OracleContract.MaxCallbackLength` (strict-UTF8 bytes).
const MAX_CALLBACK_LENGTH: usize = 32;
/// C# `OracleContract.MaxUserDataLength` (serialized bytes).
const MAX_USER_DATA_LENGTH: usize = 512;

/// Storage prefix for the oracle request price (C# `OracleContract.Prefix_Price`).
const PREFIX_PRICE: u8 = 5;
/// Storage prefix for the per-url request-id list (C# `Prefix_IdList`).
const PREFIX_ID_LIST: u8 = 6;
/// Storage prefix for the pending request records (C# `Prefix_Request`).
const PREFIX_REQUEST: u8 = 7;
/// Storage prefix for the next-request-id counter (C# `Prefix_RequestId`).
const PREFIX_REQUEST_ID: u8 = 9;

/// C# default oracle price: 0.5 GAS, in datoshi (genesis `InitializeAsync` value).
const DEFAULT_ORACLE_PRICE: i64 = 50000000;
/// C# `Request`: `gasForResponse` must be at least 0.1 GAS (`0_10000000` datoshi).
const MIN_GAS_FOR_RESPONSE: i64 = 10000000;
/// C# `Request`: at most 256 pending responses per url.
const MAX_PENDING_IDS_PER_URL: usize = 256;
pub(crate) const ORACLE_REQUEST_EVENT: &str = "OracleRequest";
pub(crate) const ORACLE_RESPONSE_EVENT: &str = "OracleResponse";

native_contract_handle!(
    /// Static accessor for the OracleContract native contract.
    pub struct OracleContract {
        id: -9,
        contract_name: "OracleContract",
        hash: ORACLE_CONTRACT_HASH,
    }
);

impl NativeContract for OracleContract {
    native_contract_identity!(OracleContract);

    fn methods(&self) -> &[NativeMethod] {
        &metadata::ORACLE_CONTRACT_METHODS
    }

    fn supports_empty_block_fast_forward(&self) -> bool {
        true
    }

    fn event_descriptors(&self) -> &[NativeEvent] {
        &metadata::ORACLE_CONTRACT_EVENTS
    }

    /// C# `OracleContract.Activations => [null, HF_Faun]` (OracleContract.cs:56):
    /// active from genesis, but the manifest's supported standards update at
    /// Faun, so the Faun boundary must refresh the stored contract state.
    fn activations(&self) -> &'static [Hardfork] {
        &[Hardfork::HfFaun]
    }

    /// C# `OracleContract.OnManifestCompose` (OracleContract.cs:58-64): NEP-30
    /// once HF_Faun is enabled at the height; no standards before it.
    fn supported_standards(&self, settings: &ProtocolSettings, block_height: u32) -> Vec<String> {
        if settings.is_hardfork_enabled(Hardfork::HfFaun, block_height) {
            crate::native_supported_standards(&[crate::NEP30_STANDARD])
        } else {
            Vec::new()
        }
    }

    /// Url + original txid pair consumed by the engine's oracle-response
    /// witness path (`CheckWitness` signer inheritance).
    ///
    /// C# `GetRequest(...)` exposed through the native-contract seam so the
    /// engine can resolve oracle-response witnesses without depending on
    /// `neo-native-contracts`.
    fn oracle_request_url_full(
        &self,
        snapshot: &DataCache,
        id: u64,
    ) -> CoreResult<Option<OracleRequestDetails>> {
        Ok(self
            .read_request(snapshot, id)?
            .map(|request| OracleRequestDetails::new(request.url, request.original_tx_id)))
    }

    /// C# `OracleContract.InitializeAsync(engine, hardfork)` for
    /// `hardfork == ActiveIn` (the Oracle contract is genesis-active): seed the
    /// request-id counter with `BigInteger.Zero` (stored as empty bytes) and the
    /// request price with 0.5 GAS (`0_50000000` datoshi).
    fn initialize(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        let snapshot = engine.snapshot_cache();
        snapshot.add(
            Self::request_id_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(0))),
        );
        snapshot.add(
            Self::price_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_ORACLE_PRICE,
            ))),
        );
        Ok(())
    }

    /// C# `OracleContract.PostPersistAsync`: for every oracle-response
    /// transaction in the persisting block, remove the answered request
    /// record and its id from the per-url id-list, then mint the oracle
    /// price to the designated oracle node selected by `id % nodes.len()`.
    fn post_persist(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        let (block_index, response_ids): (u32, Vec<u64>) = {
            let block = crate::support::engine::require_persisting_block(
                engine,
                "OracleContract::post_persist",
            )?;
            let ids = block
                .transactions
                .iter()
                .filter_map(|tx| Self::oracle_response_attribute(tx).map(|response| response.id))
                .collect();
            (block.index(), ids)
        };

        let snapshot = engine.snapshot_cache();
        let mut nodes: Option<Vec<(UInt160, BigInt)>> = None;
        for id in response_ids {
            // Remove the request from storage (skip responses without one).
            let key = Self::request_key(id);
            let Some(item) = snapshot.get(&key) else {
                continue;
            };
            let request = Self::decode_oracle_request(&item.value_bytes())?;
            snapshot.delete(&key);

            // Remove the id from the url id-list; C# throws when the id is
            // not listed, and deletes the entry once the list is empty.
            let list_key = Self::id_list_key(&request.url);
            let mut list = match snapshot.get(&list_key) {
                Some(list_item) => Self::decode_id_list(&list_item.value_bytes())?,
                None => Vec::new(),
            };
            let Some(position) = list.iter().position(|listed| *listed == id) else {
                return Err(CoreError::invalid_operation(
                    "OracleContract::post_persist: request id missing from the url id-list",
                ));
            };
            list.remove(position);
            if list.is_empty() {
                snapshot.delete(&list_key);
            } else {
                snapshot.update(
                    list_key,
                    StorageItem::from_bytes(Self::encode_id_list(&list)?),
                );
            }

            // Accumulate the oracle fee for the node selected by the id.
            if nodes.is_none() {
                let points = crate::RoleManagement::new().get_designated_by_role_at(
                    &snapshot,
                    crate::Role::Oracle,
                    block_index,
                )?;
                nodes = Some(
                    points
                        .into_iter()
                        .map(|point| {
                            (
                                UInt160::from_script(&Contract::create_signature_redeem_script(
                                    point,
                                )),
                                BigInt::from(0),
                            )
                        })
                        .collect(),
                );
            }
            if let Some(nodes) = nodes.as_mut() {
                if !nodes.is_empty() {
                    let index = usize::try_from(id % nodes.len() as u64).unwrap_or(0);
                    let price = self.read_price(&snapshot)?;
                    nodes[index].1 += BigInt::from(price);
                }
            }
        }

        if let Some(nodes) = nodes {
            for (account, gas) in nodes {
                if gas > BigInt::from(0) {
                    crate::GasToken::new().gas_mint(engine, &account, &gas, false)?;
                }
            }
        }
        Ok(())
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        match method {
            "getPrice" => {
                let price = self.read_price(&snapshot)?;
                Ok(BigInt::from(price).to_signed_bytes_le())
            }
            "setPrice" => {
                // C#: validate price > 0 -> AssertCommittee -> overwrite Prefix_Price.
                let price = crate::args::raw_i64_arg(args, 0, "OracleContract::setPrice").map_err(
                    |_| CoreError::invalid_operation("OracleContract::setPrice requires a price"),
                )?;
                if price <= 0 {
                    return Err(CoreError::invalid_operation(format!(
                        "Oracle price must be positive, got {price}"
                    )));
                }
                crate::committee::assert_committee(engine, "setPrice")?;
                self.put_price(&engine.snapshot_cache(), price)?;
                Ok(Vec::new())
            }
            "request" => {
                // C# Request(url, filter?, callback, userData, gasForResponse):
                // size/shape validations, fees + response-GAS mint, id
                // allocation, request record + per-url id-list, notification.
                let url = crate::args::raw_string_arg(args, 0, "OracleContract::request", "url")?;
                if url.len() > MAX_URL_LENGTH {
                    return Err(CoreError::invalid_operation(format!(
                        "URL size {} bytes exceeds maximum allowed size of {MAX_URL_LENGTH} bytes.",
                        url.len()
                    )));
                }

                // `filter` is a nullable String: a Null arg (bit 1 of the
                // native arg null-mask) means "no filter".
                let filter_is_null = engine
                    .get_state::<NativeArgNullMask>()
                    .is_some_and(|mask| mask.0 & (1 << 1) != 0);
                let filter = if filter_is_null {
                    None
                } else {
                    Some(crate::args::raw_string_arg(
                        args,
                        1,
                        "OracleContract::request",
                        "filter",
                    )?)
                };
                let filter_size = filter.as_ref().map_or(0, String::len);
                if filter_size > MAX_FILTER_LENGTH {
                    return Err(CoreError::invalid_operation(format!(
                        "Filter size {filter_size} bytes exceeds maximum allowed size of {MAX_FILTER_LENGTH} bytes.",
                    )));
                }

                let callback =
                    crate::args::raw_string_arg(args, 2, "OracleContract::request", "callback")?;
                if callback.len() > MAX_CALLBACK_LENGTH {
                    return Err(CoreError::invalid_operation(format!(
                        "Callback size {} bytes exceeds maximum allowed size of {MAX_CALLBACK_LENGTH} bytes.",
                        callback.len()
                    )));
                }
                if callback.starts_with('_') {
                    return Err(CoreError::invalid_operation(
                        "Callback cannot start with underscore.",
                    ));
                }

                let user_data_bytes = args.get(3).cloned().unwrap_or_default();

                let gas_for_response = crate::args::raw_i64_arg(args, 4, "OracleContract::request")
                    .map_err(|_| {
                        CoreError::invalid_operation(
                            "OracleContract::request requires a gasForResponse",
                        )
                    })?;
                if gas_for_response < MIN_GAS_FOR_RESPONSE {
                    return Err(CoreError::invalid_operation(format!(
                        "gasForResponse {gas_for_response} must be at least 0.1 GAS.",
                    )));
                }

                // engine.AddFee(GetPrice * FeeFactor) — the request price, in
                // datoshi — then AddFee(gasForResponse * FeeFactor) and the
                // response-GAS mint to the oracle account.
                let price = self.read_price(&snapshot)?;
                engine
                    .charge_execution_fee(u64::try_from(price).unwrap_or(0))
                    .map_err(|e| {
                        CoreError::invalid_operation(format!(
                            "OracleContract::request: price fee: {e}"
                        ))
                    })?;
                engine
                    .charge_execution_fee(u64::try_from(gas_for_response).unwrap_or(0))
                    .map_err(|e| {
                        CoreError::invalid_operation(format!(
                            "OracleContract::request: response fee: {e}"
                        ))
                    })?;
                crate::GasToken::new().gas_mint(
                    engine,
                    &Self::script_hash(),
                    &BigInt::from(gas_for_response),
                    false,
                )?;

                // Increase the request id (the request takes the pre-increment value).
                let id = self.read_request_id(&snapshot)?;
                self.write_request_id(&snapshot, &(BigInt::from(id) + 1));

                // The request must come from a deployed contract
                // (C# ContractManagement.IsContract(CallingScriptHash)).
                let calling = engine.get_calling_script_hash().ok_or_else(|| {
                    CoreError::invalid_operation(
                        "OracleContract::request requires a calling contract",
                    )
                })?;
                if !crate::ContractManagement::is_contract(&snapshot, &calling) {
                    return Err(CoreError::invalid_operation(
                        "OracleContract::request: caller is not a deployed contract",
                    ));
                }

                // C#: UserData = BinarySerializer.Serialize(userData,
                // MaxUserDataLength, engine.Limits.MaxStackSize) — re-encode
                // the marshaled item under the 512-byte cap.
                let limits = ExecutionEngineLimits::default();
                let user_data_item = crate::support::codec::decode_stack_value(
                    &user_data_bytes,
                    "OracleContract::request userData",
                )?;
                let user_data = BinarySerializer::serialize_stack_value_with_limits(
                    &user_data_item,
                    MAX_USER_DATA_LENGTH,
                    limits.max_stack_size as usize,
                )
                .map_err(|e| {
                    CoreError::invalid_operation(format!("OracleContract::request userData: {e}"))
                })?;

                let request = OracleRequest {
                    original_tx_id: self.get_original_txid(engine, &snapshot)?,
                    gas_for_response,
                    url: url.clone(),
                    filter: filter.clone(),
                    callback_contract: calling,
                    callback_method: callback,
                    user_data,
                };
                self.add_request_record(&snapshot, id, &request)?;

                // Add the id to the per-url IdList (capped at 256 pending).
                let list_key = Self::id_list_key(&url);
                let mut list = match snapshot.get(&list_key) {
                    Some(item) => Self::decode_id_list(&item.value_bytes())?,
                    None => Vec::new(),
                };
                if list.len() >= MAX_PENDING_IDS_PER_URL {
                    return Err(CoreError::invalid_operation(
                        "There are too many pending responses for this url",
                    ));
                }
                list.push(id);
                snapshot.update(
                    list_key,
                    StorageItem::from_bytes(Self::encode_id_list(&list)?),
                );

                let filter_item = match &filter {
                    Some(f) => StackItem::from_byte_string(f.as_bytes().to_vec()),
                    None => StackItem::null(),
                };
                engine
                    .send_notification(
                        Self::script_hash(),
                        ORACLE_REQUEST_EVENT.to_owned(),
                        vec![
                            StackItem::from_int(BigInt::from(id)),
                            StackItem::from_byte_string(calling.to_bytes()),
                            StackItem::from_byte_string(url.as_bytes().to_vec()),
                            filter_item,
                        ],
                    )
                    .map_err(|e| {
                        CoreError::invalid_operation(format!("OracleContract::request notify: {e}"))
                    })?;
                Ok(Vec::new())
            }
            "finish" => {
                // C# Finish: only valid as the single direct call of an
                // oracle-response transaction's fixed script.
                if engine.invocation_stack().len() != 2 {
                    return Err(CoreError::invalid_operation(
                        "OracleContract::finish: invalid invocation stack depth",
                    ));
                }
                let current = engine.current_script_hash().ok_or_else(|| {
                    CoreError::invalid_operation("OracleContract::finish: no current script")
                })?;
                if engine.get_invocation_counter(&current) != 1 {
                    return Err(CoreError::invalid_operation(
                        "OracleContract::finish: invalid invocation counter",
                    ));
                }
                let (id, code_byte, result) = {
                    let container = engine.script_container().ok_or_else(|| {
                        CoreError::invalid_operation(
                            "OracleContract::finish requires a transaction container",
                        )
                    })?;
                    let tx = container
                        .as_any()
                        .downcast_ref::<Transaction>()
                        .ok_or_else(|| {
                            CoreError::invalid_operation(
                                "OracleContract::finish: script container is not a transaction",
                            )
                        })?;
                    let response = Self::oracle_response_attribute(tx)
                        .ok_or_else(|| CoreError::invalid_operation("Oracle response not found"))?;
                    (response.id, response.code as u8, response.result.clone())
                };
                let request = self
                    .read_request(&snapshot, id)?
                    .ok_or_else(|| CoreError::invalid_operation("Oracle request not found"))?;
                engine
                    .send_notification(
                        Self::script_hash(),
                        ORACLE_RESPONSE_EVENT.to_owned(),
                        vec![
                            StackItem::from_int(BigInt::from(id)),
                            StackItem::from_byte_string(request.original_tx_id.to_bytes()),
                        ],
                    )
                    .map_err(|e| {
                        CoreError::invalid_operation(format!("OracleContract::finish notify: {e}"))
                    })?;
                let user_data = BinarySerializer::deserialize(
                    &request.user_data,
                    &ExecutionEngineLimits::default(),
                    None,
                )
                .map_err(|e| {
                    CoreError::deserialization(format!("OracleContract::finish userData: {e}"))
                })?;
                // C# CallFromNativeContractAsync(Hash, CallbackContract,
                // CallbackMethod, Url, userData, (int)Code, Result): the
                // callback runs after this native call returns.
                engine.queue_contract_call_from_native(
                    Self::script_hash(),
                    request.callback_contract,
                    request.callback_method.clone(),
                    vec![
                        StackItem::from_byte_string(request.url.as_bytes().to_vec()),
                        user_data,
                        StackItem::from_int(BigInt::from(i64::from(code_byte))),
                        StackItem::from_byte_string(result),
                    ],
                );
                Ok(Vec::new())
            }
            "verify" => {
                // C#: `(Transaction?)engine.ScriptContainer` — a null
                // container yields false, a non-transaction container is an
                // invalid cast (fault), otherwise true iff the transaction
                // carries an OracleResponse attribute.
                let Some(container) = engine.script_container() else {
                    return Ok(vec![0]);
                };
                let tx = container
                    .as_any()
                    .downcast_ref::<Transaction>()
                    .ok_or_else(|| {
                        CoreError::invalid_operation(
                            "OracleContract::verify: script container is not a transaction",
                        )
                    })?;
                Ok(vec![u8::from(
                    Self::oracle_response_attribute(tx).is_some(),
                )])
            }
            other => Err(CoreError::invalid_operation(format!(
                "OracleContract method '{other}' is not implemented"
            ))),
        }
    }
}

#[cfg(test)]
#[path = "../tests/oracle_contract/mod.rs"]
mod tests;
