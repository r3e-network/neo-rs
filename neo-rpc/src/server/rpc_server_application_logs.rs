//! ApplicationLogs RPC endpoints (`ApplicationLogs` plugin).

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_method_attribute::RpcMethodDescriptor;
use crate::server::rpc_server::{RpcHandler, RpcServer};
use neo_core::application_logs::ApplicationLogsService;
use neo_core::smart_contract::TriggerType;
use neo_core::UInt256;
use serde_json::Value;
use std::str::FromStr;
use std::sync::Arc;

pub struct RpcServerApplicationLogs;

impl RpcServerApplicationLogs {
    pub fn register_handlers() -> Vec<RpcHandler> {
        vec![Self::handler(
            "getapplicationlog",
            Self::get_application_log,
        )]
    }

    fn handler(
        name: &'static str,
        func: fn(&RpcServer, &[Value]) -> Result<Value, RpcException>,
    ) -> RpcHandler {
        RpcHandler::new(RpcMethodDescriptor::new(name), Arc::new(func))
    }

    fn get_application_log(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let hash = expect_hash_param(params, 0, "getapplicationlog")?;
        let trigger_filter = match params.get(1) {
            None | Some(Value::Null) => None,
            Some(Value::String(value)) => {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            }
            _ => {
                return Err(invalid_params(
                    "getapplicationlog expects string parameter 2",
                ))
            }
        };

        let service = server
            .system()
            .get_service::<ApplicationLogsService>()
            .map_err(|e| internal_error(e.to_string()))?
            .ok_or_else(|| internal_error("ApplicationLogs service not available"))?;

        let mut raw = service
            .get_block_log(&hash)
            .or_else(|| service.get_transaction_log(&hash))
            .ok_or_else(|| {
                RpcException::from(
                    RpcError::invalid_params().with_data("Unknown transaction/blockhash"),
                )
            })?;

        if let Some(filter) = trigger_filter {
            if TriggerType::from_str(&filter).is_ok() {
                if let Value::Object(obj) = &mut raw {
                    if let Some(Value::Array(executions)) = obj.get_mut("executions") {
                        executions.retain(|entry| {
                            entry
                                .get("trigger")
                                .and_then(Value::as_str)
                                .map(|value| value.eq_ignore_ascii_case(&filter))
                                .unwrap_or(false)
                        });
                    }
                }
            }
        }

        Ok(raw)
    }
}

fn invalid_params(message: impl Into<String>) -> RpcException {
    RpcException::from(RpcError::invalid_params().with_data(message.into()))
}

fn internal_error(message: impl Into<String>) -> RpcException {
    RpcException::from(RpcError::internal_server_error().with_data(message.into()))
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
                        .with_data(format!("invalid hash '{}': {}", text, err)),
                )
            })
        })
}
