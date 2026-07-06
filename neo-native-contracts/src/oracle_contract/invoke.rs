//! Oracle native-method dispatch.
//!
//! Keeps request/finish/verify method handling out of the contract root while
//! preserving the C#-compatible validation order, fee charging, storage writes,
//! notifications, and deferred callback behavior.

use super::{
    MAX_CALLBACK_LENGTH, MAX_FILTER_LENGTH, MAX_PENDING_IDS_PER_URL, MAX_URL_LENGTH,
    MAX_USER_DATA_LENGTH, MIN_GAS_FOR_RESPONSE, ORACLE_REQUEST_EVENT, ORACLE_RESPONSE_EVENT,
    OracleContract, OracleRequest,
};
use neo_error::{CoreError, CoreResult};
use neo_execution::ApplicationEngine;
use neo_execution::application_engine_contract::NativeArgNullMask;
use neo_payloads::Transaction;
use neo_serialization::BinarySerializer;
use neo_storage::StorageItem;
use neo_vm::StackItem;
use neo_vm_rs::ExecutionEngineLimits;
use num_bigint::BigInt;

impl OracleContract {
    pub(super) fn invoke_native(
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

                // engine.AddFee(GetPrice * FeeFactor) - the request price, in
                // datoshi - then AddFee(gasForResponse * FeeFactor) and the
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
                // MaxUserDataLength, engine.Limits.MaxStackSize) - re-encode
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
                // C#: `(Transaction?)engine.ScriptContainer` - a null
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
