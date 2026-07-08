//! Wallet transaction materialization for successful invoke calls.
//!
//! Smart-contract invocation owns VM execution and response assembly. This
//! module owns the wallet-specific follow-up: building the transaction preview,
//! calculating fees, signing available accounts, and projecting pending
//! signature context for watch-only or missing accounts.

use std::sync::Arc;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_blockchain::{ChainTipProvider, LedgerProviderFactory, StorageLedgerProviderFactory};
use neo_error::{CoreError, CoreResult};
use neo_payloads::signer::Signer;
use neo_payloads::transaction::Transaction;
use neo_payloads::transaction_attribute::TransactionAttribute;
use neo_payloads::witness::Witness;
use neo_primitives::UInt160;
use neo_storage::persistence::StoreCache;
use neo_wallets::{Wallet, WalletAccount, WalletError};
use num_bigint::BigInt;
use rand::random;
use serde_json::{Map, Number as JsonNumber, Value, json};

use crate::server::rpc_server::RpcServer;
use crate::server::wallet_compat;

use super::native_provider::{
    NativeSmartContractProviderFactory, SmartContractNativeProvider,
    SmartContractNativeProviderFactory,
};

const TRANSACTION_TYPE_NAME: &str = "Neo.Network.P2P.Payloads.Transaction";

enum WalletInvocationOutcome {
    Signed(Vec<u8>),
    Pending(Value),
}

#[derive(Clone)]
struct PendingSignatureItem {
    account: UInt160,
    script: Option<Vec<u8>>,
    parameter_types: Vec<neo_primitives::ContractParameterType>,
}

pub(super) fn process_invoke_with_wallet(
    server: &RpcServer,
    result: &mut Map<String, Value>,
    script: &[u8],
    signers: Option<&[Signer]>,
    snapshot: &StoreCache,
    system_fee: i64,
) {
    let signers = match signers {
        Some(list) if !list.is_empty() => list,
        _ => return,
    };
    let wallet = match server.wallet() {
        Some(wallet) => wallet,
        None => return,
    };

    match build_and_sign_transaction(server, script, signers, snapshot, system_fee, wallet) {
        Ok(WalletInvocationOutcome::Signed(bytes)) => {
            result.insert(
                "tx".to_string(),
                Value::String(BASE64_STANDARD.encode(bytes)),
            );
        }
        Ok(WalletInvocationOutcome::Pending(context)) => {
            result.insert("pendingsignature".to_string(), context);
        }
        Err(err) => {
            result.insert("exception".to_string(), Value::String(err.to_string()));
        }
    }
}

fn build_and_sign_transaction(
    server: &RpcServer,
    script: &[u8],
    signers: &[Signer],
    snapshot: &StoreCache,
    system_fee: i64,
    wallet: Arc<dyn Wallet>,
) -> CoreResult<WalletInvocationOutcome> {
    let rpc_settings = server.settings().clone();
    let system = server.system();
    let protocol_settings = system.settings();
    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(random());
    tx.set_script(script.to_vec());
    tx.set_signers(signers.to_vec());
    tx.set_attributes(Vec::<TransactionAttribute>::new());

    // C# `Wallet.MakeTransaction`:
    //   ValidUntilBlock = Ledger.CurrentIndex(snapshot)
    //                     + snapshot.GetMaxValidUntilBlockIncrement(ProtocolSettings)
    // The increment is Policy-aware: the static setting pre-HF_Echidna, the
    // Policy storage value from HF_Echidna onward (falling back to the setting
    // when the key is not yet initialized). The static
    // `system.max_valid_until_block_increment()` is only correct pre-Echidna.
    let max_valid_until_block_increment = NativeSmartContractProviderFactory
        .provider()
        .max_valid_until_block_increment(snapshot.data_cache(), &protocol_settings)
        .unwrap_or_else(|_| system.max_valid_until_block_increment());
    let valid_until = StorageLedgerProviderFactory
        .provider(snapshot.data_cache())
        .current_index()
        .map_err(|err| CoreError::other(err.to_string()))?
        .saturating_add(max_valid_until_block_increment);
    tx.set_valid_until_block(valid_until);
    tx.set_system_fee(system_fee);

    let data_cache = snapshot.data_cache();
    let native_contract_provider = system.native_contract_provider();
    let account_script = |hash: &neo_primitives::UInt160| -> Option<Vec<u8>> {
        wallet
            .account(hash)
            .and_then(|account| account.contract().map(|contract| contract.script.clone()))
    };
    let network_fee = wallet_compat::calculate_network_fee(
        &tx,
        data_cache,
        &protocol_settings,
        &native_contract_provider,
        &account_script,
        rpc_settings.max_gas_invoke,
    )
    .map_err(|err| CoreError::other(err.to_string()))?;
    tx.set_network_fee(network_fee);

    let required_fee = BigInt::from(tx.system_fee()) + BigInt::from(tx.network_fee());
    let sender = signers[0].account;
    let available = wallet_compat::gas_balance_of(
        data_cache,
        &protocol_settings,
        &native_contract_provider,
        &sender,
    )
    .map_err(|err| CoreError::other(err.to_string()))?;
    if available < required_fee {
        return Err(CoreError::other(
            "Insufficient GAS balance to pay system and network fees.",
        ));
    }

    let mut tx_clone = tx.clone();
    let pending = sign_transaction_with_wallet(
        &mut tx_clone,
        signers,
        wallet.as_ref(),
        protocol_settings.network,
    );
    if pending.is_empty() {
        Ok(WalletInvocationOutcome::Signed(tx_clone.to_bytes()))
    } else {
        let context = build_pending_context(&tx_clone, pending, protocol_settings.network);
        Ok(WalletInvocationOutcome::Pending(context))
    }
}

fn sign_transaction_with_wallet(
    tx: &mut Transaction,
    signers: &[Signer],
    wallet: &(impl Wallet + ?Sized),
    network: u32,
) -> Vec<PendingSignatureItem> {
    let mut pending = Vec::new();
    for signer in signers {
        match wallet.account(&signer.account) {
            Some(account) if account.has_key() => {
                match build_account_witness(account.as_ref(), tx, network) {
                    Ok(witness) => tx.add_witness(witness),
                    Err(_) => pending.push(build_pending_item(signer.account, Some(account))),
                }
            }
            Some(account) => pending.push(build_pending_item(signer.account, Some(account))),
            None => pending.push(build_pending_item(signer.account, None)),
        }
    }
    pending
}

fn build_account_witness(
    account: &(impl WalletAccount + ?Sized),
    tx: &Transaction,
    network: u32,
) -> CoreResult<Witness> {
    let key = account.key().ok_or_else(|| {
        CoreError::other(WalletError::Other("Account locked".to_string()).to_string())
    })?;
    let signature = wallet_compat::sign_transaction_with_key(tx, &key, network)?;

    let verification_script = if let Some(contract) = account.contract() {
        contract.script.clone()
    } else {
        key.verification_script()
    };

    let mut invocation = Vec::with_capacity(signature.len() + 2);
    invocation.push(neo_vm_rs::OpCode::PUSHDATA1.byte());
    invocation.push(signature.len() as u8);
    invocation.extend_from_slice(&signature);

    Ok(Witness::new_with_scripts(invocation, verification_script))
}

fn build_pending_context(
    tx: &Transaction,
    pending: Vec<PendingSignatureItem>,
    network: u32,
) -> Value {
    let mut context = Map::new();
    context.insert(
        "type".to_string(),
        Value::String(TRANSACTION_TYPE_NAME.to_string()),
    );
    context.insert("hash".to_string(), Value::String(tx.hash().to_string()));
    context.insert(
        "data".to_string(),
        Value::String(BASE64_STANDARD.encode(tx.hash_data())),
    );

    let mut items = Map::new();
    for entry in pending {
        let mut obj = Map::new();
        obj.insert(
            "script".to_string(),
            entry.script.map_or(Value::Null, |bytes| {
                Value::String(BASE64_STANDARD.encode(bytes))
            }),
        );

        let parameters = entry
            .parameter_types
            .into_iter()
            .map(|param_type| {
                json!({
                    "type": param_type.as_str(),
                    "value": Value::Null})
            })
            .collect();
        obj.insert("parameters".to_string(), Value::Array(parameters));
        obj.insert("signatures".to_string(), Value::Object(Map::new()));

        items.insert(entry.account.to_string(), Value::Object(obj));
    }

    context.insert("items".to_string(), Value::Object(items));
    context.insert(
        "network".to_string(),
        Value::Number(JsonNumber::from(network)),
    );
    Value::Object(context)
}

fn build_pending_item(
    account: UInt160,
    wallet_account: Option<Arc<dyn WalletAccount>>,
) -> PendingSignatureItem {
    if let Some(account_ref) = wallet_account {
        if let Some(contract) = account_ref.contract() {
            return PendingSignatureItem {
                account,
                script: Some(contract.script.clone()),
                parameter_types: contract.parameter_list.clone(),
            };
        }
    }

    PendingSignatureItem {
        account,
        script: None,
        parameter_types: Vec::new(),
    }
}
