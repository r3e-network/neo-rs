//! Neo v3.10.1 `Neo.Wallets.Helper.CalculateNetworkFee` parity path.

use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_execution::ApplicationEngine;
use neo_execution::contract_state::ContractState;
use neo_execution::helper::Helper as ContractHelper;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_io::serializable::helper::SerializeHelper;
use neo_manifest::CallFlags;
use neo_payloads::HEADER_SIZE;
use neo_payloads::transaction::Transaction;
use neo_primitives::{ContractParameterType, TriggerType, UInt160, Verifiable, Witness as _};
use neo_storage::persistence::DataCache;
use neo_vm::script_builder::ScriptBuilder;
use neo_vm_rs::{OpCode, VmState as VMState};
use num_bigint::BigInt;
use num_traits::Zero;

use super::native_provider::{NativeWalletCompatProvider, WalletCompatNativeProvider};
use super::{WalletCompatError, WalletCompatResult, core_err};
use crate::server::contract_state_provider::{
    DeployedContractProvider, DeployedContractProviderFactory,
    NativeDeployedContractProviderFactory,
};
use crate::server::ledger_queries;

/// C# `Helper.CalculateNetworkFee(tx, snapshot, settings, accountScript,
/// maxExecutionCost)`.
///
/// `account_script` resolves a signer hash to the wallet account's
/// contract script (C# `wallet.GetAccount(hash)?.Contract?.Script`);
/// pass a closure returning `None` for wallet-less calls so the
/// transaction's own witnesses are consulted instead.
pub(crate) fn calculate_network_fee<F>(
    tx: &Transaction,
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    native_contract_provider: &Arc<dyn NativeContractProvider>,
    account_script: &F,
    mut max_execution_cost: i64,
) -> WalletCompatResult<i64>
where
    F: Fn(&UInt160) -> Option<Vec<u8>> + ?Sized,
{
    let hashes: Vec<UInt160> = tx.signers().iter().map(|signer| signer.account).collect();

    // Base size: header + signers + attributes + script + witness count.
    let mut size = HEADER_SIZE
        + SerializeHelper::get_var_size_serializable_slice(tx.signers())
        + SerializeHelper::get_var_size_serializable_slice(tx.attributes())
        + SerializeHelper::get_var_size_bytes(tx.script())
        + SerializeHelper::get_var_size_usize(hashes.len());

    let policy = NativeWalletCompatProvider::new(Arc::clone(native_contract_provider));
    let current_index = ledger_queries::current_index(snapshot).map_err(core_err)?;
    let exec_fee_factor = i64::from(
        policy
            .exec_fee_factor(snapshot, settings, current_index.saturating_add(1))
            .map_err(core_err)?,
    );

    let mut network_fee = BigInt::zero();
    for (index, hash) in hashes.iter().enumerate() {
        let mut witness_script = account_script(hash);
        let mut invocation_script: Option<Vec<u8>> = None;

        if witness_script.is_none() {
            // Try to find the script in the transaction's witnesses.
            if let Some(witness) = tx.witnesses().get(index) {
                let verification = witness.verification_script().to_vec();
                if verification.is_empty() {
                    // Contract-based witness: keep its invocation script.
                    invocation_script = Some(witness.invocation_script().to_vec());
                } else {
                    witness_script = Some(verification);
                }
            }
        }

        match witness_script {
            Some(script) if !script.is_empty() => {
                if ContractHelper::is_signature_contract(&script) {
                    size += 67 + SerializeHelper::get_var_size_bytes(&script);
                    network_fee += exec_fee_factor * ContractHelper::signature_contract_cost();
                } else if let Some((m, public_keys)) =
                    ContractHelper::parse_multi_sig_contract(&script)
                {
                    let n = public_keys.len();
                    let size_inv = 66 * m;
                    size += SerializeHelper::get_var_size_usize(size_inv)
                        + size_inv
                        + SerializeHelper::get_var_size_bytes(&script);
                    network_fee += exec_fee_factor
                        * ContractHelper::multi_signature_contract_cost(m as i32, n as i32);
                }
                // Other script shapes contribute nothing (C# falls through).
            }
            _ => {
                // Contract-based verification (C# branch).
                let fee = contract_verification_fee(
                    tx,
                    snapshot,
                    settings,
                    native_contract_provider,
                    hash,
                    invocation_script,
                    &mut max_execution_cost,
                    &mut size,
                )?;
                network_fee += fee;
            }
        }
    }

    let fee_per_byte = i64::from(policy.fee_per_byte(snapshot).map_err(core_err)?);
    network_fee += size as i64 * fee_per_byte;

    for attribute in tx.attributes() {
        network_fee += attribute.calculate_network_fee(snapshot, tx);
    }

    i64::try_from(network_fee)
        .map_err(|_| WalletCompatError::Other("network fee out of i64 range".to_string()))
}

// Rationale: fee calculation must thread transaction, snapshot, protocol
// settings, native provider, contract hash, mutable script, and output counters.
#[allow(clippy::too_many_arguments)]
fn contract_verification_fee(
    tx: &Transaction,
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    native_contract_provider: &Arc<dyn NativeContractProvider>,
    hash: &UInt160,
    mut invocation_script: Option<Vec<u8>>,
    max_execution_cost: &mut i64,
    size: &mut usize,
) -> WalletCompatResult<i64> {
    let contract = NativeDeployedContractProviderFactory
        .provider()
        .contract_state_by_hash(snapshot, hash)
        .map_err(core_err)?
        .ok_or_else(|| {
            let address = neo_wallets::wallet_helper::WalletAddress::to_address(
                hash,
                settings.address_version,
            );
            WalletCompatError::Other(format!(
                "The smart contract or address {hash} ({address}) is not found. If this is your \
                 wallet address and you want to sign a transaction with it, make sure you have \
                 opened this wallet."
            ))
        })?;

    // C# looks `verify` up with pcount -1 (any parameter count).
    let verify_method = contract
        .manifest
        .abi
        .methods
        .iter()
        .find(|method| method.name == "verify")
        .cloned()
        .ok_or_else(|| {
            WalletCompatError::Other(format!(
                "The smart contract {} haven't got verify method",
                contract.hash
            ))
        })?;
    if verify_method.return_type != ContractParameterType::Boolean {
        return Err(WalletCompatError::Other(
            "The verify method doesn't return boolean value.".to_string(),
        ));
    }

    if !verify_method.parameters.is_empty() && invocation_script.is_none() {
        invocation_script = Some(dummy_verify_invocation_script(&verify_method.parameters));
    }

    let invocation_size = invocation_script.as_deref().map_or(
        SerializeHelper::get_var_size_bytes(&[]),
        SerializeHelper::get_var_size_bytes,
    );
    *size += SerializeHelper::get_var_size_bytes(&[]) + invocation_size;

    let fee = run_contract_verify(
        tx,
        snapshot,
        settings,
        native_contract_provider,
        contract,
        verify_method,
        invocation_script,
        *max_execution_cost,
    )?;
    *max_execution_cost -= fee;
    if *max_execution_cost <= 0 {
        return Err(WalletCompatError::Other("Insufficient GAS.".to_string()));
    }
    Ok(fee)
}

fn dummy_verify_invocation_script(
    parameters: &[neo_manifest::ContractParameterDefinition],
) -> Vec<u8> {
    let mut builder = ScriptBuilder::new();
    for parameter in parameters {
        match parameter.param_type {
            ContractParameterType::Any
            | ContractParameterType::Signature
            | ContractParameterType::String
            | ContractParameterType::ByteArray => {
                builder.emit_push(&[0u8; 64]);
            }
            ContractParameterType::Boolean => {
                builder.emit_push_bool(true);
            }
            ContractParameterType::Integer => {
                builder.emit_instruction(OpCode::PUSHINT256, &[0u8; 32]);
            }
            ContractParameterType::Hash160 => {
                builder.emit_push(&[0u8; 20]);
            }
            ContractParameterType::Hash256 => {
                builder.emit_push(&[0u8; 32]);
            }
            ContractParameterType::PublicKey => {
                builder.emit_push(&[0u8; 33]);
            }
            ContractParameterType::Array => {
                builder.emit_opcode(OpCode::NEWARRAY0);
            }
            _ => {}
        }
    }
    builder.to_array()
}

fn run_contract_verify(
    tx: &Transaction,
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    native_contract_provider: &Arc<dyn NativeContractProvider>,
    contract: ContractState,
    verify_method: neo_manifest::ContractMethodDescriptor,
    invocation_script: Option<Vec<u8>>,
    max_execution_cost: i64,
) -> WalletCompatResult<i64> {
    let container = Arc::new(tx.clone()) as Arc<dyn Verifiable>;
    let contract_hash = contract.hash;
    let mut engine = ApplicationEngine::new_with_shared_block_and_native_contract_provider(
        TriggerType::Verification,
        Some(container),
        Arc::new(snapshot.clone()),
        None,
        settings.clone(),
        max_execution_cost,
        None,
        Some(Arc::clone(native_contract_provider)),
    )
    .map_err(|err| WalletCompatError::Other(err.to_string()))?;
    engine
        .load_contract_method(contract, verify_method, CallFlags::READ_ONLY)
        .map_err(|err| WalletCompatError::Other(err.to_string()))?;
    if let Some(script) = invocation_script {
        engine
            .load_script(script, CallFlags::NONE, None)
            .map_err(|err| WalletCompatError::Other(err.to_string()))?;
    }
    let state = engine.execute_allow_fault();
    if state == VMState::HALT && engine.result_stack().len() != 1 {
        return Err(WalletCompatError::Other(format!(
            "Smart contract {contract_hash} verification fault."
        )));
    }
    Ok(engine.fee_consumed())
}
