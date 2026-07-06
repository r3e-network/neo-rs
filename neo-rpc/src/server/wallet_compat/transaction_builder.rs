//! Neo v3.10.0 `Neo.Wallets.Wallet.MakeTransaction` parity paths.

use std::collections::BTreeMap;
use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_native_contracts::{GasToken, LedgerContract};
use neo_payloads::signer::Signer;
use neo_payloads::transaction::Transaction;
use neo_payloads::transaction_attribute::TransactionAttribute;
use neo_primitives::{UInt160, Verifiable, WitnessScope};
use neo_storage::persistence::DataCache;
use neo_vm::script_builder::ScriptBuilder;
use neo_vm_rs::{OpCode, VmState as VMState};
use neo_wallets::{TransferOutput, Wallet};
use num_bigint::BigInt;
use num_traits::Zero;
use rand::random;

use super::accounts::{find_paying_accounts, get_signers, spendable_wallet_accounts};
use super::network_fee::calculate_network_fee;
use super::probes::{
    CallArg, emit_dynamic_call, gas_balance_of, nep17_balance_of, run_test_invocation,
};
use super::{WalletCompatError, WalletCompatResult, core_err};

/// C# `Wallet.MakeTransaction(snapshot, script, sender, cosigners,
/// attributes, maxGas)` — builds the transaction, test-executes the
/// script for the system fee, and computes the network fee.
pub(crate) fn make_transaction<W>(
    wallet: &W,
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    script: &[u8],
    sender: Option<UInt160>,
    cosigners: &[Signer],
    attributes: &[TransactionAttribute],
    native_contract_provider: &Arc<dyn NativeContractProvider>,
    max_gas: i64,
) -> WalletCompatResult<Transaction>
where
    W: Wallet + ?Sized,
{
    let accounts = spendable_wallet_accounts(wallet, sender);

    let mut balances_gas = Vec::new();
    for account in accounts {
        let value = gas_balance_of(snapshot, settings, native_contract_provider, &account)?;
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
        native_contract_provider,
        max_gas,
    )
}

#[allow(clippy::too_many_arguments)]
fn make_transaction_with_balances<W>(
    wallet: &W,
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    script: &[u8],
    cosigners: &[Signer],
    attributes: &[TransactionAttribute],
    balances_gas: Vec<(UInt160, BigInt)>,
    native_contract_provider: &Arc<dyn NativeContractProvider>,
    max_gas: i64,
) -> WalletCompatResult<Transaction>
where
    W: Wallet + ?Sized,
{
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

        let container = Arc::new(tx.clone()) as Arc<dyn Verifiable>;
        let engine = run_test_invocation(
            script.to_vec(),
            snapshot,
            Some(container),
            settings,
            native_contract_provider,
            max_gas,
        )
        .map_err(|e| WalletCompatError::Other(e.to_string()))?;
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
                .account(hash)
                .and_then(|account| account.contract().map(|contract| contract.script.clone()))
        };
        let network_fee = calculate_network_fee(
            &tx,
            snapshot,
            settings,
            native_contract_provider,
            &account_script,
            max_gas,
        )?;
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
pub(crate) fn make_transfer_transaction<W>(
    wallet: &W,
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    outputs: &[TransferOutput],
    from: Option<UInt160>,
    cosigners: Option<&[Signer]>,
    native_contract_provider: &Arc<dyn NativeContractProvider>,
    max_gas: i64,
) -> WalletCompatResult<Transaction>
where
    W: Wallet + ?Sized,
{
    let accounts = spendable_wallet_accounts(wallet, from);

    let mut cosigner_list: BTreeMap<UInt160, Signer> = cosigners
        .unwrap_or_default()
        .iter()
        .map(|signer| (signer.account, signer.clone()))
        .collect();

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
            let value = nep17_balance_of(
                snapshot,
                settings,
                native_contract_provider,
                &asset_id,
                account,
            )?;
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
                .map_err(|e| WalletCompatError::Other(e.to_string()))?;
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
                let value = gas_balance_of(snapshot, settings, native_contract_provider, account)?;
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
        native_contract_provider,
        max_gas,
    )
}
