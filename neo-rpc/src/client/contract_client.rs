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

use super::models::RpcInvokeResult;
use crate::RpcClient;
use neo_core::smart_contract::native::ContractManagement;
use neo_core::{
    smart_contract::call_flags::CallFlags, ContractManifest, KeyPair, Signer, Transaction,
    WitnessScope,
};
use neo_primitives::UInt160;
use neo_vm::op_code::OpCode;
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
        let manifest_json = manifest.to_json()?.to_string();

        let mut sb = ScriptBuilder::new();
        // C# parity: ScriptBuilderExtensions.EmitDynamicCall(ContractManagement.Hash, "deploy", nef, manifestJson)
        // CreateArray(args)
        sb.emit_push(manifest_json.as_bytes());
        sb.emit_push(nef_file);
        sb.emit_push_int(2);
        sb.emit_pack();
        // EmitPush(flags)
        sb.emit_push_int(CallFlags::ALL.bits() as i64);
        // EmitPush(method)
        sb.emit_push("deploy".as_bytes());
        // EmitPush(scriptHash)
        sb.emit_push(&ContractManagement::contract_hash().to_array());
        // Syscall
        sb.emit_syscall("System.Contract.Call")?;

        let script = sb.to_array();

        let sender = key.get_script_hash();
        let signers = vec![Signer::new(sender, WitnessScope::CALLED_BY_ENTRY)];

        let mut manager = crate::TransactionManagerFactory::new(self.rpc_client.clone())
            .make_transaction(&script, &signers)
            .await?;
        manager.add_signature(key)?;
        manager.sign().await
    }

    /// Helper method to create script from contract call
    fn make_script(
        &self,
        script_hash: &UInt160,
        operation: &str,
        args: Vec<serde_json::Value>,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut sb = ScriptBuilder::new();

        // C# parity: ScriptBuilderExtensions.EmitDynamicCall(scriptHash, method, CallFlags.All, args)
        if args.is_empty() {
            sb.emit_opcode(OpCode::NEWARRAY0);
        } else {
            // CreateArray(args): push elements in reverse order, push count, PACK
            for arg in args.iter().rev() {
                Self::emit_argument(&mut sb, arg)?;
            }
            sb.emit_push_int(args.len() as i64);
            sb.emit_pack();
        }

        // EmitPush(flags), EmitPush(method), EmitPush(scriptHash), SYSCALL System.Contract.Call
        sb.emit_push_int(CallFlags::ALL.bits() as i64);
        sb.emit_push(operation.as_bytes());
        sb.emit_push(&script_hash.to_array());
        sb.emit_syscall("System.Contract.Call")?;

        Ok(sb.to_array())
    }

    /// Helper to emit argument based on type
    fn emit_argument(
        sb: &mut ScriptBuilder,
        arg: &serde_json::Value,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match arg {
            serde_json::Value::Null => {
                sb.emit_opcode(OpCode::PUSHNULL);
                Ok(())
            }
            serde_json::Value::Bool(b) => {
                sb.emit_push_bool(*b);
                Ok(())
            }
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    sb.emit_push_int(i);
                    Ok(())
                } else if let Some(u) = n.as_u64() {
                    sb.emit_push_int(u as i64);
                    Ok(())
                } else {
                    Err("Invalid number format".into())
                }
            }
            serde_json::Value::String(s) => {
                sb.emit_push(s.as_bytes());
                Ok(())
            }
            serde_json::Value::Array(arr) => {
                // C# CreateArray pushes in reverse order.
                for item in arr.iter().rev() {
                    Self::emit_argument(sb, item)?;
                }
                sb.emit_push_int(arr.len() as i64);
                sb.emit_pack();
                Ok(())
            }
            _ => Err("Unsupported argument type".into()),
        }
    }
}
