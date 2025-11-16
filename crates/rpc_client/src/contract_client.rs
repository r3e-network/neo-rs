// Copyright (C) 2015-2025 The Neo Project.
//
// contract_client.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::models::RpcInvokeResult;
use crate::RpcClient;
use neo_core::{Contract, ContractManifest, KeyPair, Transaction, UInt160};
use neo_vm::ScriptBuilder;
use std::sync::Arc;

/// Contract related operations through RPC API
/// Matches C# ContractClient
pub struct ContractClient {
    /// The RPC client instance
    rpc_client: Arc<RpcClient>,
}

impl ContractClient {
    /// ContractClient Constructor
    /// Matches C# constructor
    pub fn new(rpc_client: Arc<RpcClient>) -> Self {
        Self { rpc_client }
    }

    /// Use RPC method to test invoke operation
    /// Matches C# TestInvokeAsync
    pub async fn test_invoke(
        &self,
        script_hash: &UInt160,
        operation: &str,
        args: Vec<serde_json::Value>,
    ) -> Result<RpcInvokeResult, Box<dyn std::error::Error>> {
        // Create script using script builder
        let script = self.make_script(script_hash, operation, args)?;

        // Call RPC invoke script method
        self.rpc_client
            .invoke_script(&script)
            .await
            .map_err(|err| Box::new(err) as Box<dyn std::error::Error>)
    }

    /// Deploy Contract, return signed transaction
    /// Matches C# CreateDeployContractTxAsync
    pub async fn create_deploy_contract_tx(
        &self,
        nef_file: &[u8],
        manifest: &ContractManifest,
        key: &KeyPair,
    ) -> Result<Transaction, Box<dyn std::error::Error>> {
        let _ = (nef_file, manifest, key);
        Err("Contract deployment via RPC is not implemented yet in the Rust port".into())
    }

    /// Helper method to create script from contract call
    fn make_script(
        &self,
        script_hash: &UInt160,
        operation: &str,
        args: Vec<serde_json::Value>,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut sb = ScriptBuilder::new();

        // Convert args to ContractParameter format and emit
        for arg in args.iter().rev() {
            self.emit_argument(&mut sb, arg)?;
        }

        // Emit operation and script hash
        sb.emit_push(operation.as_bytes())?;
        sb.emit_push(&script_hash.to_array())?;
        sb.emit_syscall("System.Contract.Call")?;

        Ok(sb.to_array())
    }

    /// Helper to emit argument based on type
    fn emit_argument(
        &self,
        sb: &mut ScriptBuilder,
        arg: &serde_json::Value,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match arg {
            serde_json::Value::Null => sb.emit_push_null(),
            serde_json::Value::Bool(b) => sb.emit_push(*b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    sb.emit_push_int(i)
                } else if let Some(u) = n.as_u64() {
                    sb.emit_push_int(u as i64)
                } else {
                    Err("Invalid number format".into())
                }
            }
            serde_json::Value::String(s) => sb.emit_push(s.as_bytes()),
            serde_json::Value::Array(arr) => {
                for item in arr {
                    self.emit_argument(sb, item)?;
                }
                sb.emit_push_int(arr.len() as i64)?;
                sb.emit_pack()
            }
            _ => Err("Unsupported argument type".into()),
        }
    }
}
