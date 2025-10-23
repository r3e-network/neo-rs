// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Controllers.v1.ContractsController`.

use crate::rest_server::exceptions::{
    contract_not_found_exception::ContractNotFoundException,
    invalid_parameter_range_exception::InvalidParameterRangeException,
    json_property_null_or_empty_exception::JsonPropertyNullOrEmptyException,
    node_network_exception::NodeNetworkException,
    query_parameter_not_found_exception::QueryParameterNotFoundException,
    script_hash_format_exception::ScriptHashFormatException,
    rest_error_codes::RestErrorCodes,
};
use crate::rest_server::binder::uint160_binder_provider::UInt160BinderProvider;
use crate::rest_server::helpers::{contract_helper::ContractHelper, script_helper::ScriptHelper};
use crate::rest_server::helpers::script_helper::ScriptHelperError;
use crate::rest_server::models::count_model::CountModel;
use crate::rest_server::models::error::error_model::ErrorModel;
use crate::rest_server::models::execution_engine_model::{
    BlockchainEventModel, ExecutionEngineModel,
};
use crate::rest_server::rest_server_settings::RestServerSettings;
use crate::rest_server::rest_server_plugin::RestServerGlobals;
use crate::rest_server::rest_server_utility::{RestServerUtility, RestServerUtilityError};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use neo_core::persistence::i_read_only_store::IReadOnlyStoreGeneric;
use neo_core::persistence::seek_direction::SeekDirection;
use neo_core::smart_contract::contract_state::ContractState;
use neo_core::smart_contract::native::NativeRegistry;
use neo_core::smart_contract::ApplicationEngine;
use neo_core::smart_contract::storage_key::StorageKey;
use neo_core::{NeoSystem, UInt160};
use serde_json::{json, to_value, Value};
use std::sync::Arc;

pub struct ContractsController {
    neo_system: Arc<NeoSystem>,
}

impl ContractsController {
    pub fn new() -> Result<Self, ErrorModel> {
        RestServerGlobals::neo_system()
            .map(|system| Self { neo_system: system })
            .ok_or_else(|| NodeNetworkException::new().to_error_model())
    }

    pub fn list(
        &self,
        page: Option<i32>,
        size: Option<i32>,
    ) -> Result<Option<Value>, ErrorModel> {
        let (page, size) = Self::resolve_pagination(page, size)?;
        let store_cache = self.neo_system.store_cache();
        let contracts = ContractHelper::list_contracts(&store_cache)
            .map_err(Self::storage_error)?;

        if contracts.is_empty() {
            return Ok(None);
        }

        let mut ordered = contracts;
        ordered.sort_by_key(|contract| contract.id);

        let start = (page.saturating_sub(1) as usize).saturating_mul(size as usize);
        let slice: Vec<Value> = ordered
            .into_iter()
            .skip(start)
            .take(size as usize)
            .map(|contract| RestServerUtility::contract_state_to_j_token(&contract))
            .collect();

        if slice.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Value::Array(slice)))
        }
    }

    pub fn count(&self) -> Result<Value, ErrorModel> {
        let store_cache = self.neo_system.store_cache();
        let contracts = ContractHelper::list_contracts(&store_cache)
            .map_err(Self::storage_error)?;
        to_value(CountModel::new(contracts.len() as i32))
            .map_err(|err| Self::serialization_error(err.to_string()))
    }

    pub fn contract(&self, script_hash: &UInt160) -> Result<Value, ErrorModel> {
        let contract = self.get_contract_state(script_hash)?;
        Ok(RestServerUtility::contract_state_to_j_token(&contract))
    }

    pub fn manifest(&self, script_hash: &UInt160) -> Result<Value, ErrorModel> {
        let contract = self.get_contract_state(script_hash)?;
        Ok(RestServerUtility::contract_manifest_to_j_token(&contract.manifest))
    }

    pub fn abi(&self, script_hash: &UInt160) -> Result<Value, ErrorModel> {
        let contract = self.get_contract_state(script_hash)?;
        Ok(RestServerUtility::contract_abi_to_j_token(&contract.manifest.abi))
    }

    pub fn nef(&self, script_hash: &UInt160) -> Result<Value, ErrorModel> {
        let contract = self.get_contract_state(script_hash)?;
        Ok(RestServerUtility::contract_nef_file_to_j_token(&contract.nef))
    }

    pub fn storage(&self, script_hash: &UInt160) -> Result<Option<Value>, ErrorModel> {
        if self.is_native_contract(script_hash) {
            return Ok(None);
        }

        let contract = self.get_contract_state(script_hash)?;
        let store_cache = self.neo_system.store_cache();
        let prefix_bytes = StorageKey::create_search_prefix(contract.id, &[]);
        let prefix_key = StorageKey::from_bytes(&prefix_bytes);
        let entries = store_cache.find(Some(&prefix_key), SeekDirection::Forward);

        let storage: Vec<Value> = entries
            .filter(|(key, _)| key.id == contract.id)
            .map(|(key, item)| {
                json!({
                    "key": BASE64.encode(key.suffix()),
                    "value": BASE64.encode(item.get_value()),
                })
            })
            .collect();

        if storage.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Value::Array(storage)))
        }
    }

    pub fn invoke_contract(
        &self,
        script_hash: &UInt160,
        method: &str,
        payload: &Value,
    ) -> Result<Value, ErrorModel> {
        if method.trim().is_empty() {
            return Err(QueryParameterNotFoundException::new("method").to_error_model());
        }

        if payload.is_null() {
            return Err(JsonPropertyNullOrEmptyException::with_param_name("invokeParameters")
                .to_error_model());
        }

        let invoke_params =
            RestServerUtility::contract_invoke_parameters_from_j_token(payload)
                .map_err(Self::bad_request)?;

        let contract = self.get_contract_state(script_hash)?;
        let snapshot = Arc::new(self.neo_system.store_cache().data_cache().clone());
        let signers = if invoke_params.signers.is_empty() {
            None
        } else {
            Some(invoke_params.signers.clone())
        };

        let engine = ScriptHelper::invoke_method_with_signers(
            self.neo_system.settings(),
            snapshot,
            &contract.hash,
            method,
            &invoke_params.contract_parameters,
            signers,
        )
        .map_err(Self::script_error)?;

        let model = Self::execution_engine_to_model(&engine)?;
        to_value(model).map_err(|err| Self::serialization_error(err.to_string()))
    }

    pub fn parse_script_hash(value: &str) -> Result<UInt160, ErrorModel> {
        UInt160BinderProvider::bind(value).ok_or_else(|| {
            ScriptHashFormatException::with_message(format!("'{value}' is invalid."))
                .to_error_model()
        })
    }

    fn get_contract_state(&self, script_hash: &UInt160) -> Result<ContractState, ErrorModel> {
        let store_cache = self.neo_system.store_cache();
        ContractHelper::get_contract_state(&store_cache, script_hash)
            .map_err(Self::storage_error)?
            .ok_or_else(|| ContractNotFoundException::new(*script_hash).to_error_model())
    }

    fn resolve_pagination(
        page: Option<i32>,
        size: Option<i32>,
    ) -> Result<(i32, i32), ErrorModel> {
        let settings = RestServerSettings::current();
        let max_size = i32::try_from(settings.max_page_size).unwrap_or(i32::MAX);
        let page = page.unwrap_or(1);
        let size = size.unwrap_or(max_size);

        if page < 1 || size < 1 || size > max_size {
            return Err(InvalidParameterRangeException::new().to_error_model());
        }

        Ok((page, size))
    }

    fn execution_engine_to_model(
        engine: &ApplicationEngine,
    ) -> Result<ExecutionEngineModel, ErrorModel> {
        let notifications = engine
            .notifications()
            .iter()
            .map(|notification| {
                let mut state_values = Vec::new();
                for item in &notification.state {
                    let value =
                        RestServerUtility::stack_item_to_j_token(item).map_err(Self::stack_error)?;
                    state_values.push(value);
                }
                Ok(BlockchainEventModel::new(
                    notification.script_hash,
                    notification.event_name.clone(),
                    state_values,
                ))
            })
            .collect::<Result<Vec<_>, ErrorModel>>()?;

        let result_stack = engine
            .result_stack()
            .to_vec()
            .into_iter()
            .map(|item| RestServerUtility::stack_item_to_j_token(&item).map_err(Self::stack_error))
            .collect::<Result<Vec<_>, ErrorModel>>()?;

        let fault_exception = engine.fault_exception().map(|message| {
            ErrorModel::with_params(
                RestErrorCodes::GENERIC_EXCEPTION,
                "ApplicationEngineException".to_string(),
                message.to_string(),
            )
        });

        Ok(ExecutionEngineModel::new(
            engine.fee_consumed(),
            engine.state(),
            notifications,
            result_stack,
            fault_exception,
        ))
    }

    fn is_native_contract(&self, script_hash: &UInt160) -> bool {
        NativeRegistry::new().is_native(script_hash)
    }

    fn storage_error(message: String) -> ErrorModel {
        ErrorModel::with_params(
            RestErrorCodes::GENERIC_EXCEPTION,
            "StorageError".to_string(),
            message,
        )
    }

    fn stack_error(error: RestServerUtilityError) -> ErrorModel {
        ErrorModel::with_params(
            RestErrorCodes::GENERIC_EXCEPTION,
            "StackItemError".to_string(),
            error.to_string(),
        )
    }

    fn script_error(error: ScriptHelperError) -> ErrorModel {
        ErrorModel::with_params(
            RestErrorCodes::GENERIC_EXCEPTION,
            "ScriptExecutionError".to_string(),
            error.to_string(),
        )
    }

    fn serialization_error(message: String) -> ErrorModel {
        ErrorModel::with_params(
            RestErrorCodes::GENERIC_EXCEPTION,
            "SerializationError".to_string(),
            message,
        )
    }

    fn bad_request(message: String) -> ErrorModel {
        ErrorModel::with_params(
            RestErrorCodes::GENERIC_EXCEPTION,
            "InvalidRequest".to_string(),
            message,
        )
    }
}
