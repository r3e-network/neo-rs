// Copyright (C) 2015-2025 The Neo Project.
//
// helper.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::network::p2p::payloads::signer::Signer;
use crate::network::p2p::payloads::transaction::HEADER_SIZE;
use crate::neo_io::serializable::helper::{
    get_var_size, get_var_size_bytes, get_var_size_serializable_slice,
};
use crate::IVerifiable as CoreIVerifiable;
use crate::{
    network::p2p,
    persistence::DataCache,
    protocol_settings::ProtocolSettings,
    smart_contract::{
        application_engine::ApplicationEngine,
        call_flags::CallFlags,
        helper::Helper as ContractHelper,
        native::contract_management::ContractManagement,
        native::{
            ledger_contract::LedgerContract, native_contract::NativeContract,
            policy_contract::PolicyContract, GasToken,
        },
        ContractBasicMethod, ContractParameterType,
        trigger_type::TriggerType,
    },
    wallets::{transfer_output::TransferOutput, wallet::Wallet, wallet::WalletError, KeyPair},
    Transaction, UInt160,
};
use neo_primitives::UInt256;
use neo_primitives::WitnessScope;
use neo_vm::{op_code::OpCode, ScriptBuilder};
use num_bigint::{BigInt, Sign};
use rand::rngs::OsRng;
use rand::RngCore;
use std::sync::Arc;

/// A helper class related to wallets.
/// Matches C# Helper class exactly
pub struct Helper;

impl Helper {
    /// Signs an IVerifiable with the specified private key.
    /// Matches C# Sign method
    pub fn sign(
        verifiable: &dyn CoreIVerifiable,
        key: &KeyPair,
        network: u32,
    ) -> Result<Vec<u8>, String> {
        let sign_data =
            p2p::helper::get_sign_data_vec(verifiable, network).map_err(|e| e.to_string())?;
        key.sign(&sign_data).map_err(|e| e.to_string())
    }

    /// Converts the specified script hash to an address.
    /// Matches C# ToAddress method
    pub fn to_address(script_hash: &UInt160, version: u8) -> String {
        let mut data = Vec::with_capacity(21);
        data.push(version);
        data.extend_from_slice(&script_hash.to_array());
        base58::base58_check_encode(&data)
    }

    /// Converts the specified address to a script hash.
    /// Matches C# ToScriptHash method
    pub fn to_script_hash(address: &str, version: u8) -> Result<UInt160, String> {
        let data = address.base58_check_decode()?;
        if data.len() != 21 {
            return Err(format!("Invalid address format: expected 21 bytes after Base58Check decoding, but got {} bytes. The address may be corrupted or in an invalid format.", data.len()));
        }
        if data[0] != version {
            return Err(format!("Invalid address version: expected version {}, but got {}. The address may be for a different network.", version, data[0]));
        }
        UInt160::from_bytes(&data[1..]).map_err(|e| e.to_string())
    }

    /// XOR operation on byte arrays.
    /// Matches C# XOR method
    pub fn xor(x: &[u8], y: &[u8]) -> Result<Vec<u8>, String> {
        if x.len() != y.len() {
            return Err(format!(
                "The x.Length({}) and y.Length({}) must be equal.",
                x.len(),
                y.len()
            ));
        }
        let mut result = vec![0u8; x.len()];
        for i in 0..x.len() {
            result[i] = x[i] ^ y[i];
        }
        Ok(result)
    }

    /// Calculates the network fee for the specified transaction.
    /// Matches C# CalculateNetworkFee method with wallet
    pub fn calculate_network_fee_with_wallet(
        tx: &Transaction,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
        wallet: Option<&dyn Wallet>,
        max_execution_cost: i64,
    ) -> Result<i64, String> {
        match wallet {
            Some(wallet) => {
                let resolver: Box<AccountScriptResolver<'_>> = Box::new(move |hash: &UInt160| {
                    wallet
                        .get_account(hash)
                        .and_then(|account| account.contract().map(|c| c.script.clone()))
                });
                calculate_network_fee_impl(
                    tx,
                    snapshot,
                    settings,
                    Some(resolver.as_ref()),
                    max_execution_cost,
                )
            }
            None => calculate_network_fee_impl(tx, snapshot, settings, None, max_execution_cost),
        }
    }

    /// Calculates the network fee for the specified transaction.
    /// Matches C# CalculateNetworkFee method with account script function
    pub fn calculate_network_fee(
        tx: &Transaction,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
        account_script: Option<Box<AccountScriptResolver<'_>>>,
        max_execution_cost: i64,
    ) -> Result<i64, String> {
        calculate_network_fee_impl(
            tx,
            snapshot,
            settings,
            account_script.as_deref(),
            max_execution_cost,
        )
    }

    /// Builds a NEP-17 transfer transaction matching the C# wallet implementation.
    #[allow(clippy::too_many_arguments)]
    pub fn make_transfer_transaction(
        wallet: &dyn Wallet,
        snapshot: &DataCache,
        outputs: &[TransferOutput],
        from: Option<UInt160>,
        cosigners: Option<&[Signer]>,
        settings: &ProtocolSettings,
        persisting_block: Option<&crate::ledger::Block>,
        max_gas: i64,
    ) -> Result<Transaction, WalletError> {
        let accounts: Vec<UInt160> = if let Some(sender) = from {
            vec![sender]
        } else {
            wallet
                .get_accounts()
                .into_iter()
                .filter(|a| !a.is_locked() && a.has_key())
                .map(|a| a.script_hash())
                .collect()
        };

        if accounts.is_empty() {
            return Err(WalletError::InsufficientFunds);
        }

        let mut cosigner_map: std::collections::HashMap<UInt160, Signer> = cosigners
            .map(|list| list.iter().cloned().map(|s| (s.account, s)).collect())
            .unwrap_or_default();

        let mut script_builder = ScriptBuilder::new();
        let mut balances_gas: Option<Vec<(UInt160, BigInt)>> = None;

        // Group outputs by asset id
        let mut grouped: std::collections::HashMap<UInt160, Vec<&TransferOutput>> =
            std::collections::HashMap::new();
        for output in outputs {
            grouped.entry(output.asset_id).or_default().push(output);
        }

        for (asset_id, group) in grouped {
            let mut balances: Vec<(UInt160, BigInt)> = Vec::new();
            for account in &accounts {
                let mut inner = ScriptBuilder::new();
                inner.emit_push(&account.to_bytes());
                inner.emit_push_int(1);
                inner.emit_opcode(OpCode::PACK);
                inner.emit_push_int(CallFlags::READ_ONLY.bits() as i64);
                inner.emit_push("balanceOf".as_bytes());
                inner.emit_push(&asset_id.to_bytes());
                inner
                    .emit_syscall("System.Contract.Call")
                    .map_err(|e| WalletError::Other(e.to_string()))?;

                let mut engine = ApplicationEngine::new(
                    TriggerType::Application,
                    None,
                    Arc::new(snapshot.clone()),
                    persisting_block.cloned(),
                    settings.clone(),
                    max_gas,
                    None,
                )
                .map_err(|e| WalletError::Other(e.to_string()))?;
                engine
                    .load_script(inner.to_array(), CallFlags::READ_ONLY, Some(asset_id))
                    .map_err(|e| WalletError::Other(e.to_string()))?;
                engine
                    .execute()
                    .map_err(|e| WalletError::Other(e.to_string()))?;
                if let Ok(value) = engine
                    .result_stack()
                    .peek(0)
                    .map_err(|e| WalletError::Other(e.to_string()))
                    .and_then(|item| item.as_int().map_err(|e| WalletError::Other(e.to_string())))
                {
                    if value.sign() == Sign::Plus {
                        balances.push((*account, value));
                    }
                }
            }

            let required: BigInt = group
                .iter()
                .map(|o| o.value.value().clone())
                .fold(BigInt::from(0), |acc, v| acc + v);
            let available: BigInt = balances
                .iter()
                .map(|(_, v)| v.clone())
                .fold(BigInt::from(0), |acc, v| acc + v);
            if available < required {
                return Err(WalletError::InsufficientFunds);
            }

            for output in group {
                balances.sort_by(|a, b| a.1.cmp(&b.1));
                let payments = find_paying_accounts(&mut balances, output.value.value());
                for (account, value) in payments {
                    if let Some(signer) = cosigner_map.get_mut(&account) {
                        if signer.scopes != WitnessScope::GLOBAL {
                            signer.scopes |= WitnessScope::CALLED_BY_ENTRY;
                        }
                    } else {
                        cosigner_map
                            .insert(account, Signer::new(account, WitnessScope::CALLED_BY_ENTRY));
                    }
                    emit_transfer(
                        &mut script_builder,
                        &output.asset_id,
                        &account,
                        &output.script_hash,
                        &value,
                        output.data.as_deref(),
                    )?;
                }
            }

            if asset_id == GasToken::new().hash() {
                balances_gas = Some(balances);
            }
        }

        let balances_gas = balances_gas.unwrap_or_else(|| {
            accounts
                .iter()
                .filter_map(|account| {
                    let gas = GasToken::new().balance_of_snapshot(snapshot, account);
                    if gas.sign() == Sign::Plus {
                        Some((*account, gas))
                    } else {
                        None
                    }
                })
                .collect()
        });

        let mut tx = Transaction::new();
        tx.set_nonce(OsRng.next_u32());
        tx.set_script(script_builder.to_array());

        let ledger = LedgerContract::new();
        let policy = PolicyContract::new();
        let current_height = ledger
            .current_index(snapshot)
            .map_err(|e| WalletError::Other(e.to_string()))?;
        let valid_until = current_height
            + policy
                .get_max_valid_until_block_increment_snapshot(snapshot, settings)
                .unwrap_or(settings.max_valid_until_block_increment);
        tx.set_valid_until_block(valid_until);

        let signers = reorder_signers(
            from.unwrap_or(accounts[0]),
            cosigner_map.values().cloned().collect(),
        );
        tx.set_signers(signers);

        // Execute script to calculate system fee
        let script_container: Arc<dyn CoreIVerifiable> = Arc::new(tx.clone());
        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            Some(script_container),
            Arc::new(snapshot.clone_cache()),
            persisting_block.cloned(),
            settings.clone(),
            max_gas,
            None,
        )
        .map_err(|e| WalletError::TransactionCreationFailed(e.to_string()))?;
        engine
            .load_script(tx.script().to_vec(), CallFlags::ALL, None)
            .map_err(|e| WalletError::TransactionCreationFailed(e.to_string()))?;
        engine
            .execute()
            .map_err(|e| WalletError::TransactionCreationFailed(e.to_string()))?;
        if engine.state() == neo_vm::vm_state::VMState::FAULT {
            return Err(WalletError::TransactionCreationFailed(
                "Smart contract execution failed.".to_string(),
            ));
        }
        tx.set_system_fee(engine.fee_consumed());

        let network_fee = Helper::calculate_network_fee_with_wallet(
            &tx,
            snapshot,
            settings,
            Some(wallet),
            max_gas,
        )
        .map_err(WalletError::TransactionCreationFailed)?;
        tx.set_network_fee(network_fee);

        // Ensure gas balances can pay for fees
        for (_, gas_balance) in balances_gas {
            if gas_balance >= BigInt::from(tx.system_fee() + tx.network_fee()) {
                return Ok(tx);
            }
        }

        Err(WalletError::InsufficientFunds)
    }

    /// Builds a smart-contract invocation transaction matching the C# wallet implementation.
    #[allow(clippy::too_many_arguments)]
    pub fn make_transaction(
        wallet: &dyn Wallet,
        snapshot: &DataCache,
        script: &[u8],
        sender: Option<UInt160>,
        cosigners: Option<&[Signer]>,
        attributes: Option<&[p2p::payloads::transaction_attribute::TransactionAttribute]>,
        settings: &ProtocolSettings,
        persisting_block: Option<&crate::ledger::Block>,
        max_gas: i64,
    ) -> Result<Transaction, WalletError> {
        let accounts: Vec<UInt160> = if let Some(sender) = sender {
            vec![sender]
        } else {
            wallet
                .get_accounts()
                .into_iter()
                .filter(|a| !a.is_locked() && a.has_key())
                .map(|a| a.script_hash())
                .collect()
        };

        if accounts.is_empty() {
            return Err(WalletError::InsufficientFunds);
        }

        let gas = GasToken::new();
        let balances_gas: Vec<(UInt160, BigInt)> = accounts
            .iter()
            .filter_map(|account| {
                let balance = gas.balance_of_snapshot(snapshot, account);
                if balance.sign() == Sign::Plus {
                    Some((*account, balance))
                } else {
                    None
                }
            })
            .collect();

        if balances_gas.is_empty() {
            return Err(WalletError::InsufficientFunds);
        }

        let ledger = LedgerContract::new();
        let policy = PolicyContract::new();
        let current_height = ledger
            .current_index(snapshot)
            .map_err(|e| WalletError::Other(e.to_string()))?;
        let valid_until = current_height
            + policy
                .get_max_valid_until_block_increment_snapshot(snapshot, settings)
                .unwrap_or(settings.max_valid_until_block_increment);

        let attributes = attributes.unwrap_or(&[]);
        let cosigners = cosigners.unwrap_or(&[]);

        for (account, balance) in balances_gas {
            let mut tx = Transaction::new();
            tx.set_nonce(OsRng.next_u32());
            tx.set_script(script.to_vec());
            tx.set_valid_until_block(valid_until);
            tx.set_signers(reorder_signers(account, cosigners.to_vec()));
            tx.set_attributes(attributes.to_vec());

            let script_container: Arc<dyn CoreIVerifiable> = Arc::new(tx.clone());
            let mut engine = ApplicationEngine::new(
                TriggerType::Application,
                Some(script_container),
                Arc::new(snapshot.clone_cache()),
                persisting_block.cloned(),
                settings.clone(),
                max_gas,
                None,
            )
            .map_err(|e| WalletError::TransactionCreationFailed(e.to_string()))?;
            engine
                .load_script(tx.script().to_vec(), CallFlags::ALL, None)
                .map_err(|e| WalletError::TransactionCreationFailed(e.to_string()))?;
            engine
                .execute()
                .map_err(|e| WalletError::TransactionCreationFailed(e.to_string()))?;
            if engine.state() == neo_vm::vm_state::VMState::FAULT {
                return Err(WalletError::TransactionCreationFailed(
                    "Smart contract execution failed.".to_string(),
                ));
            }
            tx.set_system_fee(engine.fee_consumed());

            let network_fee = Helper::calculate_network_fee_with_wallet(
                &tx,
                snapshot,
                settings,
                Some(wallet),
                max_gas,
            )
            .map_err(WalletError::TransactionCreationFailed)?;
            tx.set_network_fee(network_fee);

            if balance >= BigInt::from(tx.system_fee() + tx.network_fee()) {
                return Ok(tx);
            }
        }

        Err(WalletError::InsufficientFunds)
    }
}

fn calculate_network_fee_impl(
    tx: &Transaction,
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    account_script: Option<&AccountScriptResolver<'_>>,
    max_execution_cost: i64,
) -> Result<i64, String> {
    let hashes = tx.get_script_hashes_for_verifying(snapshot);

    let ledger = LedgerContract::new();
    let height = ledger
        .current_index(snapshot)
        .unwrap_or(0)
        .saturating_add(1);
    let policy = PolicyContract::new();
    let exec_fee_factor = policy
        .get_exec_fee_factor_snapshot(snapshot, settings, height)
        .unwrap_or(PolicyContract::DEFAULT_EXEC_FEE_FACTOR) as i64;
    let fee_per_byte = policy
        .get_fee_per_byte_snapshot(snapshot)
        .unwrap_or(PolicyContract::DEFAULT_FEE_PER_BYTE as i64);

    let base_size = HEADER_SIZE
        + get_var_size_serializable_slice(tx.signers())
        + get_var_size_serializable_slice(tx.attributes())
        + get_var_size_bytes(tx.script())
        + get_var_size(hashes.len() as u64);
    let mut size: i64 = base_size as i64;
    let mut network_fee: i64 = 0;
    let mut remaining_execution_cost = max_execution_cost;

    for (index, hash) in hashes.iter().enumerate() {
        let mut invocation_script: Option<Vec<u8>> = None;
        let mut witness_script = account_script.and_then(|resolver| resolver(hash));

        if witness_script.is_none() {
            if let Some(witness) = tx.witnesses().get(index) {
                if witness.verification_script.is_empty() {
                    invocation_script = Some(witness.invocation_script.clone());
                } else {
                    witness_script = Some(witness.verification_script.clone());
                }
            }
        }

        let witness_script = witness_script.unwrap_or_default();

        if witness_script.is_empty() {
            let contract = ContractManagement::get_contract_from_snapshot(snapshot, hash)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| {
                    format!(
                        "The smart contract or address {} ({}) is not found. If this is your wallet address and you want to sign a transaction with it, make sure you have opened this wallet.",
                        hash.to_hex_string(),
                        Helper::to_address(hash, settings.address_version)
                    )
                })?;

            let contract_hash = contract.hash;
            let mut abi = contract.manifest.abi.clone();
            let method = abi
                .get_method(
                    ContractBasicMethod::VERIFY,
                    ContractBasicMethod::VERIFY_P_COUNT,
                )
                .ok_or_else(|| {
                    format!("The smart contract {} haven't got verify method", contract.hash)
                })?
                .clone();

            if method.return_type != ContractParameterType::Boolean {
                return Err("The verify method doesn't return boolean value.".to_string());
            }

            if method.parameters.len() > 0 && invocation_script.is_none() {
                let mut builder = ScriptBuilder::new();
                for param in &method.parameters {
                    match param.param_type {
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
                            builder.emit_push(&[0u8; UInt160::LENGTH]);
                        }
                        ContractParameterType::Hash256 => {
                            builder.emit_push(&[0u8; UInt256::LENGTH]);
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
                invocation_script = Some(builder.to_array());
            }

            let has_invocation_script = invocation_script.is_some();
            let invocation_script = invocation_script.unwrap_or_default();
            size += (get_var_size_bytes(&[]) + get_var_size_bytes(&invocation_script)) as i64;

            let snapshot_clone = Arc::new(snapshot.clone_cache());
            let mut engine = ApplicationEngine::new(
                TriggerType::Verification,
                Some(Arc::new(tx.clone())),
                snapshot_clone,
                None,
                settings.clone(),
                remaining_execution_cost,
                None,
            )
            .map_err(|e| e.to_string())?;

            engine
                .load_contract_method(contract, method, CallFlags::READ_ONLY)
                .map_err(|e| e.to_string())?;

            if has_invocation_script {
                engine
                    .load_script(invocation_script, CallFlags::NONE, None)
                    .map_err(|e| e.to_string())?;
            }

            engine.execute().map_err(|e| e.to_string())?;
            if engine.result_stack().len() != 1 {
                return Err(format!(
                    "Smart contract {} verification fault.",
                    contract_hash
                ));
            }
            let result_item = engine
                .result_stack()
                .peek(0)
                .map_err(|e| e.to_string())?;
            if result_item.get_boolean().unwrap_or(false) == false {
                return Err(format!(
                    "Smart contract {} verification fault.",
                    contract_hash
                ));
            }

            remaining_execution_cost -= engine.fee_consumed();
            if remaining_execution_cost <= 0 {
                return Err("Insufficient GAS.".to_string());
            }
            network_fee += engine.fee_consumed();
            continue;
        }

        if ContractHelper::is_signature_contract(&witness_script) {
            size += 67 + var_size_with_payload(witness_script.len());
            network_fee += exec_fee_factor * ContractHelper::signature_contract_cost();
        } else if let Some((m, n)) = parse_multi_sig_contract(&witness_script) {
            let invocation_len = 66 * m as i64;
            size += var_size_with_payload(invocation_len as usize);
            size += var_size_with_payload(witness_script.len());
            network_fee +=
                exec_fee_factor * ContractHelper::multi_signature_contract_cost(m as i32, n as i32);
        } else {
            return Err(format!(
                "Contract-based verification for script hash {} is not yet supported in this build.",
                hex::encode(hash.to_array())
            ));
        }
    }

    network_fee += size * fee_per_byte;
    for attribute in tx.attributes() {
        network_fee += attribute.calculate_network_fee(snapshot, tx);
    }

    Ok(network_fee)
}

fn var_size_prefix(len: usize) -> i64 {
    if len < 0xFD {
        1
    } else if len <= 0xFFFF {
        3
    } else if len <= 0xFFFF_FFFF {
        5
    } else {
        9
    }
}

fn var_size_with_payload(len: usize) -> i64 {
    var_size_prefix(len) + len as i64
}

/// Finds paying accounts for a required amount (C# FindPayingAccounts).
fn find_paying_accounts(
    ordered_accounts: &mut Vec<(UInt160, BigInt)>,
    amount: &BigInt,
) -> Vec<(UInt160, BigInt)> {
    let mut result = Vec::new();
    let mut remaining = amount.clone();
    let sum_balance: BigInt = ordered_accounts
        .iter()
        .map(|(_, v)| v.clone())
        .fold(BigInt::from(0), |acc, v| acc + v);

    if sum_balance == *amount {
        result.append(ordered_accounts);
        return result;
    }

    for i in 0..ordered_accounts.len() {
        if ordered_accounts[i].1 < remaining {
            continue;
        }
        if ordered_accounts[i].1 == remaining {
            result.push(ordered_accounts.remove(i));
        } else {
            let (account, value) = &ordered_accounts[i];
            result.push((*account, remaining.clone()));
            ordered_accounts[i].1 = value - remaining.clone();
        }
        return result;
    }

    let mut idx = ordered_accounts.len();
    while idx > 0 {
        idx -= 1;
        let (account, value) = ordered_accounts[idx].clone();
        if value <= remaining {
            result.push((account, value.clone()));
            remaining -= value;
            ordered_accounts.remove(idx);
        }
        if remaining.sign() != Sign::Plus {
            return result;
        }
    }

    if remaining.sign() == Sign::Plus {
        for (account, value) in ordered_accounts.iter_mut() {
            if *value >= remaining {
                result.push((*account, remaining.clone()));
                *value -= remaining.clone();
                break;
            }
        }
    }

    result
}

fn emit_transfer(
    builder: &mut ScriptBuilder,
    asset_id: &UInt160,
    from: &UInt160,
    to: &UInt160,
    amount: &BigInt,
    data: Option<&dyn std::any::Any>,
) -> Result<(), WalletError> {
    let mut arg_count = 0usize;

    if let Some(extra) = data {
        if let Some(bytes) = extra.downcast_ref::<Vec<u8>>() {
            builder.emit_push(bytes);
        } else if let Some(text) = extra.downcast_ref::<String>() {
            builder.emit_push(text.as_bytes());
        } else {
            builder.emit_opcode(OpCode::PUSHNULL);
        }
    } else {
        builder.emit_opcode(OpCode::PUSHNULL);
    }
    arg_count += 1;

    builder
        .emit_push_bigint(amount.clone())
        .map_err(|e| WalletError::TransactionCreationFailed(e.to_string()))?;
    arg_count += 1;

    builder.emit_push(&to.to_bytes());
    arg_count += 1;

    builder.emit_push(&from.to_bytes());
    arg_count += 1;

    if arg_count == 0 {
        builder.emit_opcode(OpCode::NEWARRAY0);
    } else {
        builder.emit_push_int(arg_count as i64);
        builder.emit_opcode(OpCode::PACK);
    }

    builder.emit_push_int(CallFlags::ALL.bits() as i64);
    builder.emit_push("transfer".as_bytes());
    builder.emit_push(&asset_id.to_bytes());
    builder
        .emit_syscall("System.Contract.Call")
        .map_err(|e| WalletError::TransactionCreationFailed(e.to_string()))?;
    builder.emit_opcode(OpCode::ASSERT);
    Ok(())
}

fn reorder_signers(sender: UInt160, mut signers: Vec<Signer>) -> Vec<Signer> {
    if let Some(pos) = signers.iter().position(|s| s.account == sender) {
        if pos != 0 {
            let signer = signers.remove(pos);
            signers.insert(0, signer);
        }
        signers
    } else {
        let mut list = Vec::with_capacity(signers.len() + 1);
        list.push(Signer::new(sender, WitnessScope::NONE));
        list.extend(signers);
        list
    }
}

fn parse_multi_sig_contract(script: &[u8]) -> Option<(usize, usize)> {
    if script.len() < 43 {
        return None;
    }

    let first = OpCode::try_from(script[0]).ok()?;
    let first_byte = first as u8;
    if !((OpCode::PUSH1 as u8)..=(OpCode::PUSH16 as u8)).contains(&first_byte) {
        return None;
    }
    let m = (first as u8 - OpCode::PUSH0 as u8) as usize;

    let mut offset = 1;
    let mut n = 0usize;
    while offset < script.len() {
        if script[offset] != OpCode::PUSHDATA1 as u8 {
            break;
        }
        if offset + 2 >= script.len() {
            return None;
        }
        let key_len = script[offset + 1] as usize;
        if key_len != 33 || offset + 2 + key_len > script.len() {
            return None;
        }
        offset += 2 + key_len;
        n += 1;
    }

    if n == 0 || offset >= script.len() {
        return None;
    }

    let push_n = OpCode::try_from(script[offset]).ok()?;
    let opcode_value = push_n as u8;
    if !((OpCode::PUSH1 as u8)..=(OpCode::PUSH16 as u8)).contains(&opcode_value) {
        return None;
    }
    if (push_n as u8 - OpCode::PUSH0 as u8) as usize != n {
        return None;
    }
    offset += 1;

    if offset + 5 != script.len() {
        return None;
    }
    if script[offset] != OpCode::SYSCALL as u8 {
        return None;
    }

    Some((m, n))
}

/// Base58 utilities
pub mod base58 {
    use crate::cryptography::crypto_utils::Base58;

    /// Encodes data with a 4-byte double-SHA256 checksum using Base58Check.
    pub fn base58_check_encode(data: &[u8]) -> String {
        Base58::encode_check(data)
    }
}

/// Base58Check decode extension
pub trait Base58CheckDecode {
    fn base58_check_decode(&self) -> Result<Vec<u8>, String>;
}

impl Base58CheckDecode for str {
    fn base58_check_decode(&self) -> Result<Vec<u8>, String> {
        let bytes = bs58::decode(self)
            .into_vec()
            .map_err(|e| format!("Invalid Base58 string: {}", e))?;

        if bytes.len() < 4 {
            return Err("Invalid Base58Check format: decoded data length is too short (requires at least 4 checksum bytes).".to_string());
        }

        let (payload, checksum) = bytes.split_at(bytes.len() - 4);
        let expected = crate::cryptography::crypto_utils::NeoHash::hash256(payload);
        if checksum != &expected[..4] {
            return Err("Invalid Base58Check checksum: provided checksum does not match calculated checksum.".to_string());
        }

        Ok(payload.to_vec())
    }
}
pub type AccountScriptResolver<'a> = dyn Fn(&UInt160) -> Option<Vec<u8>> + 'a;
