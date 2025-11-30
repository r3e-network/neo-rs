//! RPC handlers for ApplicationLogs plugin.
//!
//! Matches C# Neo.Plugins.ApplicationLogs.LogReader RPC methods.

use neo_core::UInt256;
use serde_json::Value;
use std::str::FromStr;
use std::sync::Arc;

use crate::rpc_server::rpc_error::RpcError;
use crate::rpc_server::rpc_exception::RpcException;
use crate::rpc_server::rpc_method_attribute::RpcMethodDescriptor;
use crate::rpc_server::rpc_server::{RpcHandler, RpcServer};

use super::log_reader::LogReader;
use parking_lot::RwLock;
use std::collections::HashMap;

/// Global storage for log readers, keyed by network.
static LOG_READERS: once_cell::sync::Lazy<RwLock<HashMap<u32, Arc<RwLock<LogReader>>>>> =
    once_cell::sync::Lazy::new(|| RwLock::new(HashMap::new()));

/// Registers a LogReader instance for the given network.
pub fn register_log_reader(network: u32, reader: Arc<RwLock<LogReader>>) {
    LOG_READERS.write().insert(network, reader);
}

/// Removes the LogReader for the given network.
pub fn unregister_log_reader(network: u32) {
    LOG_READERS.write().remove(&network);
}

/// Returns the LogReader for the given network.
fn get_log_reader(network: u32) -> Option<Arc<RwLock<LogReader>>> {
    LOG_READERS.read().get(&network).cloned()
}

pub struct ApplicationLogsRpcHandlers;

impl ApplicationLogsRpcHandlers {
    /// Returns the RPC handlers to register.
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

    /// Retrieves the application log for a block or transaction.
    ///
    /// Matches C# `LogReader.GetApplicationLog(UInt256 hash, string triggerType = "")`.
    ///
    /// # Parameters
    /// - `hash`: The block hash or transaction hash (UInt256)
    /// - `triggerType`: Optional trigger type filter ("OnPersist", "PostPersist",
    ///   "Application", "Verification", etc.)
    ///
    /// # Returns
    /// JSON object with `executions` array containing execution logs.
    fn get_application_log(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        // Parse hash parameter
        let hash_str = params.first().and_then(|v| v.as_str()).ok_or_else(|| {
            RpcException::from(RpcError::invalid_params().with_data("hash parameter required"))
        })?;

        let hash = UInt256::from_str(hash_str).map_err(|err| {
            RpcException::from(
                RpcError::invalid_params().with_data(format!("invalid hash: {}", err)),
            )
        })?;

        // Parse optional trigger type parameter
        let trigger_type = params
            .get(1)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        // Get the LogReader for this network
        let network = server.system().settings().network;
        let reader_arc = get_log_reader(network).ok_or_else(|| {
            RpcException::from(
                RpcError::internal_server_error()
                    .with_data("ApplicationLogs plugin not initialized"),
            )
        })?;

        // Query the application log
        let reader = reader_arc.read();
        match reader.get_application_log(hash, trigger_type) {
            Ok(result) => Ok(result),
            Err(err) => Err(RpcException::from(
                RpcError::invalid_params().with_data(err),
            )),
        }
    }
}
