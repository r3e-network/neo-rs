use super::contract_script::{build_dynamic_call_script, emit_contract_call};
use super::models::RpcInvokeResult;
use crate::{RpcClient, RpcError};
use neo_manifest::ContractManifest;
use neo_native_contracts::ContractManagement;
use neo_payloads::{Signer, Transaction};
use neo_primitives::UInt160;
use neo_primitives::{CallFlags, WitnessScope};
use neo_vm::script_builder::ScriptBuilder;
use neo_wallets::KeyPair;
use std::sync::Arc;

/// Contract related operations through RPC API
/// Matches C# `ContractClient`
pub struct ContractClient {
    /// The RPC client instance
    rpc_client: Arc<RpcClient>,
}

impl ContractClient {
    /// `ContractClient` Constructor
    /// Matches C# constructor
    #[must_use]
    pub const fn new(rpc_client: Arc<RpcClient>) -> Self {
        Self { rpc_client }
    }

    /// Use RPC method to test invoke operation
    /// Matches C# `TestInvokeAsync`
    pub async fn test_invoke(
        &self,
        script_hash: &UInt160,
        operation: &str,
        args: Vec<serde_json::Value>,
    ) -> Result<RpcInvokeResult, RpcError> {
        // Create script using script builder
        let script = build_dynamic_call_script(script_hash, operation, &args, CallFlags::ALL)?;

        // Call RPC invoke script method
        self.rpc_client
            .invoke_script(&script)
            .await
            .map_err(Into::into)
    }

    /// Deploy Contract, return signed transaction
    /// Matches C# `CreateDeployContractTxAsync`
    pub async fn create_deploy_contract_tx(
        &self,
        nef_file: &[u8],
        manifest: &ContractManifest,
        key: &KeyPair,
    ) -> Result<Transaction, RpcError> {
        let script = Self::build_deploy_contract_script(nef_file, manifest, CallFlags::ALL)?;

        let sender = key.get_script_hash();
        let signers = vec![Signer::new(sender, WitnessScope::CALLED_BY_ENTRY)];

        let mut manager = crate::TransactionManagerFactory::new(self.rpc_client.clone())
            .make_transaction(&script, &signers)
            .await?;
        manager.add_signature(key)?;
        manager.sign().await
    }

    #[cfg(test)]
    fn build_dynamic_call_script(
        script_hash: &UInt160,
        method: &str,
        args: &[serde_json::Value],
        call_flags: CallFlags,
    ) -> Result<Vec<u8>, RpcError> {
        build_dynamic_call_script(script_hash, method, args, call_flags)
    }

    fn build_deploy_contract_script(
        nef_file: &[u8],
        manifest: &ContractManifest,
        call_flags: CallFlags,
    ) -> Result<Vec<u8>, RpcError> {
        let manifest_json = manifest.to_json()?.to_string();

        let mut sb = ScriptBuilder::new();
        // C# parity: ScriptBuilderExtensions.EmitDynamicCall(ContractManagement.Hash, "deploy", nef, manifestJson)
        sb.emit_push(manifest_json.as_bytes());
        sb.emit_push(nef_file);
        sb.emit_push_int(2);
        sb.emit_pack();
        emit_contract_call(
            &mut sb,
            &ContractManagement::script_hash(),
            "deploy",
            call_flags,
        )?;

        Ok(sb.to_array())
    }
}

// NOTE: Script byte layout parity is covered by the VM/native-contract
// compatibility tests, so this optional client module only checks RPC assembly.

#[cfg(test)]
#[path = "../../tests/client/contract_client.rs"]
mod tests;
