//! Wallet-side transaction building, ported from C#.
//!
//! The C# RPC wallet endpoints lean on `Neo.Wallets.Helper.CalculateNetworkFee`
//! and `Neo.Wallets.Wallet.MakeTransaction`. Those helpers have no
//! counterpart in the `neo-wallets` crate yet, so this module ports the
//! v3.9.1 algorithms 1:1 for the RPC server's use:
//!
//! - [`calculate_network_fee`] — `Helper.CalculateNetworkFee(tx, snapshot,
//!   settings, accountScript, maxExecutionCost)`.
//! - [`make_transaction`] — `Wallet.MakeTransaction(snapshot, script,
//!   sender, cosigners, attributes, maxGas)`.
//! - [`make_transfer_transaction`] — `Wallet.MakeTransaction(snapshot,
//!   outputs, from, cosigners)`.
//! - [`sign_transaction_with_key`] — `Helper.Sign(verifiable, key, network)`.
//!
//! All engine probes run the real native/contract code through a fresh
//! [`ApplicationEngine`], matching the C# `ApplicationEngine.Run` test
//! invocations these algorithms are specified in terms of.

use std::collections::BTreeMap;
use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_execution::ApplicationEngine;
use neo_execution::contract_state::ContractState;
use neo_execution::helper::Helper as ContractHelper;
use neo_io::serializable::helper::{
    get_var_size_bytes, get_var_size_serializable_slice, get_var_size_usize,
};
use neo_manifest::CallFlags;
use neo_native_contracts::{ContractManagement, GasToken, LedgerContract, PolicyContract};
use neo_payloads::HEADER_SIZE;
use neo_payloads::signer::Signer;
use neo_payloads::transaction::Transaction;
use neo_payloads::transaction_attribute::TransactionAttribute;
use neo_primitives::{ContractParameterType, TriggerType, UInt160, Verifiable, WitnessScope};
use neo_vm::script_builder::ScriptBuilder;
use neo_storage::persistence::DataCache;
use neo_vm_rs::{OpCode, VmState as VMState};
use neo_wallets::{KeyPair, TransferOutput, Wallet};
use num_bigint::BigInt;
use num_traits::Zero;
use rand::random;

/// Wallet-layer failure vocabulary mirroring the C# exceptions the RPC
/// server maps onto JSON-RPC errors.
#[derive(Debug)]
pub(crate) enum WalletCompatError {
    /// C# `InvalidOperationException("Insufficient GAS...")` — wallet
    /// balances cannot cover the system + network fees, or a transfer
    /// amount exceeds the wallet balance.
    InsufficientFunds(String),
    /// Any other invalid-operation failure (faulted probe scripts,
    /// missing contracts, …).
    Other(String),
}

impl std::fmt::Display for WalletCompatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InsufficientFunds(msg) | Self::Other(msg) => f.write_str(msg),
        }
    }
}

type WalletCompatResult<T> = Result<T, WalletCompatError>;

/// C# `Neo.Wallets.Helper.Sign(IVerifiable, KeyPair, network)`: signs
/// the verifiable's network-prefixed sign data with the key.
pub(crate) fn sign_transaction_with_key(
    tx: &Transaction,
    key: &KeyPair,
    network: u32,
) -> Result<Vec<u8>, String> {
    let data = neo_payloads::get_sign_data(tx, network).map_err(|err| err.to_string())?;
    key.sign(&data).map_err(|err| err.to_string())
}

/// C# `Helper.CalculateNetworkFee(tx, snapshot, settings, accountScript,
/// maxExecutionCost)`.
///
/// `account_script` resolves a signer hash to the wallet account's
/// contract script (C# `wallet.GetAccount(hash)?.Contract?.Script`);
/// pass a closure returning `None` for wallet-less calls so the
/// transaction's own witnesses are consulted instead.
pub(crate) fn calculate_network_fee(
    tx: &Transaction,
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    account_script: &dyn Fn(&UInt160) -> Option<Vec<u8>>,
    mut max_execution_cost: i64,
) -> WalletCompatResult<i64> {
    let hashes: Vec<UInt160> = tx.signers().iter().map(|signer| signer.account).collect();

    // Base size: header + signers + attributes + script + witness count.
    let mut size = HEADER_SIZE
        + get_var_size_serializable_slice(tx.signers())
        + get_var_size_serializable_slice(tx.attributes())
        + get_var_size_bytes(tx.script())
        + get_var_size_usize(hashes.len());

    let policy = PolicyContract::new();
    let current_index = LedgerContract::new()
        .current_index(snapshot)
        .map_err(core_err)?;
    let exec_fee_factor = i64::from(
        policy
            .get_exec_fee_factor_snapshot(snapshot, settings, current_index.saturating_add(1))
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
                    size += 67 + get_var_size_bytes(&script);
                    network_fee += exec_fee_factor * ContractHelper::signature_contract_cost();
                } else if let Some((m, public_keys)) =
                    ContractHelper::parse_multi_sig_contract(&script)
                {
                    let n = public_keys.len();
                    let size_inv = 66 * m;
                    size += get_var_size_usize(size_inv) + size_inv + get_var_size_bytes(&script);
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
                    hash,
                    invocation_script,
                    &mut max_execution_cost,
                    &mut size,
                )?;
                network_fee += fee;
            }
        }
    }

    let fee_per_byte = i64::from(
        policy
            .get_fee_per_byte_snapshot(snapshot)
            .map_err(core_err)?,
    );
    network_fee += size as i64 * fee_per_byte;

    for attribute in tx.attributes() {
        network_fee += attribute.calculate_network_fee(snapshot, tx);
    }

    i64::try_from(network_fee)
        .map_err(|_| WalletCompatError::Other("network fee out of i64 range".to_string()))
}

/// C# contract-based verification cost branch of `CalculateNetworkFee`.
#[allow(clippy::too_many_arguments)]
fn contract_verification_fee(
    tx: &Transaction,
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    hash: &UInt160,
    mut invocation_script: Option<Vec<u8>>,
    max_execution_cost: &mut i64,
    size: &mut usize,
) -> WalletCompatResult<i64> {
    let contract = ContractManagement::get_contract_from_snapshot(snapshot, hash)
        .map_err(core_err)?
        .ok_or_else(|| {
            // C# `Helper.CalculateNetworkFee` includes the base58
            // rendering: `{hash} ({hash.ToAddress(settings.AddressVersion)})`.
            let address = neo_wallets::wallet_helper::to_address(hash, settings.address_version);
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
        // Push a dummy argument per parameter, exactly as C# does, so
        // the fee covers argument loading.
        let mut builder = ScriptBuilder::new();
        for parameter in &verify_method.parameters {
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
        invocation_script = Some(builder.to_array());
    }

    // Empty verification script + the invocation script bytes.
    let invocation_size = invocation_script
        .as_deref()
        .map_or(get_var_size_bytes(&[]), get_var_size_bytes);
    *size += get_var_size_bytes(&[]) + invocation_size;

    let fee = run_contract_verify(
        tx,
        snapshot,
        settings,
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

/// Runs the contract's `verify` under `TriggerType::Verification` and
/// returns the consumed fee (C# `engine.FeeConsumed`).
fn run_contract_verify(
    tx: &Transaction,
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    contract: ContractState,
    verify_method: neo_manifest::ContractMethodDescriptor,
    invocation_script: Option<Vec<u8>>,
    max_execution_cost: i64,
) -> WalletCompatResult<i64> {
    let container = Arc::new(tx.clone()) as Arc<dyn Verifiable>;
    let contract_hash = contract.hash;
    let mut engine = ApplicationEngine::new(
        TriggerType::Verification,
        Some(container),
        Arc::new(snapshot.clone()),
        None,
        settings.clone(),
        max_execution_cost,
        None,
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
    if state == VMState::HALT {
        // C# demands exactly one boolean on the result stack.
        if engine.result_stack().len() != 1 {
            return Err(WalletCompatError::Other(format!(
                "Smart contract {contract_hash} verification fault."
            )));
        }
    }
    Ok(engine.fee_consumed())
}

/// Runs `script` as a test invocation (C# `ApplicationEngine.Run`) with
/// an optional transaction container and returns the engine.
fn run_test_invocation(
    script: Vec<u8>,
    snapshot: &DataCache,
    container: Option<Arc<dyn Verifiable>>,
    settings: &ProtocolSettings,
    max_gas: i64,
) -> Result<ApplicationEngine, String> {
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        container,
        Arc::new(snapshot.clone()),
        None,
        settings.clone(),
        max_gas,
        None,
    )
    .map_err(|err| err.to_string())?;
    engine
        .load_script(script, CallFlags::ALL, None)
        .map_err(|err| err.to_string())?;
    engine.execute_allow_fault();
    Ok(engine)
}

/// `NativeContract.GAS.BalanceOf(snapshot, account)` via a `balanceOf`
/// engine probe (the canonical read in the Rust tree).
pub(crate) fn gas_balance_of(
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    account: &UInt160,
) -> WalletCompatResult<BigInt> {
    nep17_balance_of(snapshot, settings, &GasToken::script_hash(), account)
}

/// `balanceOf` probe for an arbitrary NEP-17 asset.
fn nep17_balance_of(
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    asset: &UInt160,
    account: &UInt160,
) -> WalletCompatResult<BigInt> {
    let mut builder = ScriptBuilder::new();
    emit_dynamic_call(
        &mut builder,
        asset,
        "balanceOf",
        &[CallArg::Bytes(account.to_bytes())],
    )
    .map_err(WalletCompatError::Other)?;
    let engine = run_test_invocation(
        builder.to_array(),
        snapshot,
        None,
        settings,
        BALANCE_PROBE_GAS,
    )
    .map_err(WalletCompatError::Other)?;
    if engine.state() != VMState::HALT {
        return Err(WalletCompatError::Other(format!(
            "Failed to execute balanceOf method for asset {asset} on account {account}. The \
             smart contract execution faulted with state: {:?}.",
            engine.state()
        )));
    }
    engine
        .result_stack()
        .peek(0)
        .map_err(|err| WalletCompatError::Other(err.to_string()))?
        .as_int()
        .map_err(|err| WalletCompatError::Other(err.to_string()))
}

/// GAS budget for `balanceOf` probes — C# uses the test-mode default
/// (`ApplicationEngine.TestModeGas`, 2 GAS in datoshi).
const BALANCE_PROBE_GAS: i64 = 2_0000_0000;

/// Argument for [`emit_dynamic_call`].
enum CallArg {
    Bytes(Vec<u8>),
    Int(BigInt),
    Null,
}

/// `ScriptBuilderExtensions.EmitDynamicCall(hash, method, args…)` with
/// `CallFlags::ALL` (the C# default used by transfer scripts).
fn emit_dynamic_call(
    builder: &mut ScriptBuilder,
    contract: &UInt160,
    method: &str,
    args: &[CallArg],
) -> Result<(), String> {
    if args.is_empty() {
        builder.emit_push_int(0);
        builder.emit_pack();
    } else {
        for arg in args.iter().rev() {
            match arg {
                CallArg::Bytes(bytes) => {
                    builder.emit_push(bytes);
                }
                CallArg::Int(value) => {
                    builder
                        .emit_push_bigint(value.clone())
                        .map_err(|err| err.to_string())?;
                }
                CallArg::Null => {
                    builder.emit_opcode(OpCode::PUSHNULL);
                }
            }
        }
        builder.emit_push_int(args.len() as i64);
        builder.emit_pack();
    }
    builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
    builder.emit_push(method.as_bytes());
    builder.emit_push(&contract.to_array());
    builder
        .emit_syscall("System.Contract.Call")
        .map_err(|err| err.to_string())?;
    Ok(())
}

/// C# `Wallet.GetSigners(sender, cosigners)`: moves the sender's signer
/// to the front, or prepends a `WitnessScope::NONE` sender signer.
fn get_signers(sender: UInt160, cosigners: &[Signer]) -> Vec<Signer> {
    for (i, cosigner) in cosigners.iter().enumerate() {
        if cosigner.account == sender {
            if i == 0 {
                return cosigners.to_vec();
            }
            let mut list = cosigners.to_vec();
            let signer = list.remove(i);
            list.insert(0, signer);
            return list;
        }
    }
    let mut list = Vec::with_capacity(cosigners.len() + 1);
    list.push(Signer::new(sender, WitnessScope::NONE));
    list.extend_from_slice(cosigners);
    list
}

/// C# `Wallet.FindPayingAccounts(orderedAccounts, amount)`.
fn find_paying_accounts(
    ordered_accounts: &mut Vec<(UInt160, BigInt)>,
    mut amount: BigInt,
) -> Vec<(UInt160, BigInt)> {
    let mut result = Vec::new();
    let sum_balance: BigInt = ordered_accounts.iter().map(|(_, value)| value).sum();
    if sum_balance == amount {
        result.append(ordered_accounts);
        return result;
    }

    for i in 0..ordered_accounts.len() {
        if ordered_accounts[i].1 < amount {
            continue;
        }
        if ordered_accounts[i].1 == amount {
            result.push(ordered_accounts.remove(i));
        } else {
            result.push((ordered_accounts[i].0, amount.clone()));
            let remaining = ordered_accounts[i].1.clone() - amount.clone();
            ordered_accounts[i] = (ordered_accounts[i].0, remaining);
        }
        break;
    }
    if result.is_empty() && !ordered_accounts.is_empty() {
        let mut i = ordered_accounts.len() - 1;
        while ordered_accounts[i].1 <= amount {
            let (account, value) = ordered_accounts.remove(i);
            amount -= value.clone();
            result.push((account, value));
            if i == 0 {
                break;
            }
            i -= 1;
        }
        if amount > BigInt::zero() {
            for i in 0..ordered_accounts.len() {
                if ordered_accounts[i].1 < amount {
                    continue;
                }
                if ordered_accounts[i].1 == amount {
                    result.push(ordered_accounts.remove(i));
                } else {
                    result.push((ordered_accounts[i].0, amount.clone()));
                    let remaining = ordered_accounts[i].1.clone() - amount.clone();
                    ordered_accounts[i] = (ordered_accounts[i].0, remaining);
                }
                break;
            }
        }
    }
    result
}

/// C# `Wallet.MakeTransaction(snapshot, script, sender, cosigners,
/// attributes, maxGas)` — builds the transaction, test-executes the
/// script for the system fee, and computes the network fee.
pub(crate) fn make_transaction(
    wallet: &dyn Wallet,
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    script: &[u8],
    sender: Option<UInt160>,
    cosigners: &[Signer],
    attributes: &[TransactionAttribute],
    max_gas: i64,
) -> WalletCompatResult<Transaction> {
    let accounts: Vec<UInt160> = match sender {
        Some(sender) => vec![sender],
        None => wallet
            .get_accounts()
            .into_iter()
            .filter(|account| !account.is_locked() && account.has_key())
            .map(|account| account.script_hash())
            .collect(),
    };

    let mut balances_gas = Vec::new();
    for account in accounts {
        let value = gas_balance_of(snapshot, settings, &account)?;
        if value > BigInt::zero() {
            balances_gas.push((account, value));
        }
    }

    make_transaction_with_balances(
        wallet,
        snapshot,
        settings,
        script,
        cosigners,
        attributes,
        balances_gas,
        max_gas,
    )
}

/// Core of C# `Wallet.MakeTransaction(snapshot, script, cosigners,
/// attributes, balancesGas, maxGas)`.
#[allow(clippy::too_many_arguments)]
fn make_transaction_with_balances(
    wallet: &dyn Wallet,
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    script: &[u8],
    cosigners: &[Signer],
    attributes: &[TransactionAttribute],
    balances_gas: Vec<(UInt160, BigInt)>,
    max_gas: i64,
) -> WalletCompatResult<Transaction> {
    let current_index = LedgerContract::new()
        .current_index(snapshot)
        .map_err(core_err)?;
    let max_increment = settings.max_valid_until_block_increment;

    for (account, value) in balances_gas {
        let mut tx = Transaction::new();
        tx.set_version(0);
        tx.set_nonce(random());
        tx.set_script(script.to_vec());
        tx.set_valid_until_block(current_index.saturating_add(max_increment));
        tx.set_signers(get_signers(account, cosigners));
        tx.set_attributes(attributes.to_vec());

        // Test-execute the script to derive the system fee.
        let container = Arc::new(tx.clone()) as Arc<dyn Verifiable>;
        let engine = run_test_invocation(
            script.to_vec(),
            snapshot,
            Some(container),
            settings,
            max_gas,
        )
        .map_err(WalletCompatError::Other)?;
        if engine.state() == VMState::FAULT {
            let detail = engine
                .fault_exception()
                .map(|msg| format!(" {msg}"))
                .unwrap_or_default();
            return Err(WalletCompatError::Other(format!(
                "Smart contract execution failed. The execution faulted and cannot be \
                 completed.{detail}"
            )));
        }
        tx.set_system_fee(engine.fee_consumed());

        let account_script = |hash: &UInt160| -> Option<Vec<u8>> {
            wallet
                .get_account(hash)
                .and_then(|account| account.contract().map(|contract| contract.script.clone()))
        };
        let network_fee = calculate_network_fee(&tx, snapshot, settings, &account_script, max_gas)?;
        tx.set_network_fee(network_fee);

        if value >= BigInt::from(tx.system_fee()) + BigInt::from(tx.network_fee()) {
            return Ok(tx);
        }
    }
    Err(WalletCompatError::InsufficientFunds(
        "Insufficient GAS balance to cover system and network fees. Please ensure your account \
         has enough GAS to pay for transaction fees."
            .to_string(),
    ))
}

/// C# `Wallet.MakeTransaction(snapshot, outputs, from, cosigners)` —
/// builds the NEP-17 transfer script from the outputs and the paying
/// accounts, then delegates to the script-based overload.
pub(crate) fn make_transfer_transaction(
    wallet: &dyn Wallet,
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    outputs: &[TransferOutput],
    from: Option<UInt160>,
    cosigners: Option<&[Signer]>,
    max_gas: i64,
) -> WalletCompatResult<Transaction> {
    let accounts: Vec<UInt160> = match from {
        Some(from) => vec![from],
        None => wallet
            .get_accounts()
            .into_iter()
            .filter(|account| !account.is_locked() && account.has_key())
            .map(|account| account.script_hash())
            .collect(),
    };

    let mut cosigner_list: BTreeMap<UInt160, Signer> = cosigners
        .unwrap_or_default()
        .iter()
        .map(|signer| (signer.account, signer.clone()))
        .collect();

    // Group the outputs by asset, preserving first-appearance order.
    let mut asset_order: Vec<UInt160> = Vec::new();
    for output in outputs {
        if !asset_order.contains(&output.asset_id) {
            asset_order.push(output.asset_id);
        }
    }

    let gas_hash = GasToken::script_hash();
    let mut balances_gas: Option<Vec<(UInt160, BigInt)>> = None;
    let mut builder = ScriptBuilder::new();

    for asset_id in asset_order {
        let group: Vec<&TransferOutput> = outputs
            .iter()
            .filter(|output| output.asset_id == asset_id)
            .collect();
        let sum: BigInt = group
            .iter()
            .map(|output| output.value.value().clone())
            .sum();

        let mut balances: Vec<(UInt160, BigInt)> = Vec::new();
        for account in &accounts {
            let value = nep17_balance_of(snapshot, settings, &asset_id, account)?;
            if value > BigInt::zero() {
                balances.push((*account, value));
            }
        }
        let sum_balance: BigInt = balances.iter().map(|(_, value)| value).sum();
        if sum_balance < sum {
            return Err(WalletCompatError::InsufficientFunds(format!(
                "Insufficient balance for transfer: required {sum} units, but only \
                 {sum_balance} units are available across all accounts. Please ensure \
                 sufficient balance before attempting the transfer."
            )));
        }

        for output in group {
            balances.sort_by(|a, b| a.1.cmp(&b.1));
            let balances_used = find_paying_accounts(&mut balances, output.value.value().clone());
            for (account, value) in balances_used {
                match cosigner_list.get_mut(&account) {
                    Some(signer) => {
                        if signer.scopes != WitnessScope::GLOBAL {
                            signer.scopes |= WitnessScope::CALLED_BY_ENTRY;
                        }
                    }
                    None => {
                        cosigner_list
                            .insert(account, Signer::new(account, WitnessScope::CALLED_BY_ENTRY));
                    }
                }
                emit_dynamic_call(
                    &mut builder,
                    &output.asset_id,
                    "transfer",
                    &[
                        CallArg::Bytes(account.to_bytes()),
                        CallArg::Bytes(output.script_hash.to_bytes()),
                        CallArg::Int(value),
                        CallArg::Null,
                    ],
                )
                .map_err(WalletCompatError::Other)?;
                builder.emit_opcode(OpCode::ASSERT);
            }
        }

        if asset_id == gas_hash {
            balances_gas = Some(balances);
        }
    }
    let script = builder.to_array();

    let balances_gas = match balances_gas {
        Some(balances) => balances,
        None => {
            let mut balances = Vec::new();
            for account in &accounts {
                let value = gas_balance_of(snapshot, settings, account)?;
                if value > BigInt::zero() {
                    balances.push((*account, value));
                }
            }
            balances
        }
    };

    let cosigners: Vec<Signer> = cosigner_list.into_values().collect();
    make_transaction_with_balances(
        wallet,
        snapshot,
        settings,
        &script,
        &cosigners,
        &[],
        balances_gas,
        max_gas,
    )
}

fn core_err(err: neo_error::CoreError) -> WalletCompatError {
    WalletCompatError::Other(err.to_string())
}
