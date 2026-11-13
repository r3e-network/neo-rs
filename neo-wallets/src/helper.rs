//! Wallet helper utilities.
//!
//! Conversions between script hashes, Neo addresses and verification scripts,
//! plus the transaction / network-fee construction helpers. Mirrors the C#
//! `Neo.Wallets.Helper` class as ported into `neo-core`'s `wallets::helper`.
//!
//! The fee/transaction builders were migrated from `neo-core` on top of the
//! decomposed crates (`neo-protocol`, `neo-smart-contract`,
//! `neo-native-contracts`, `neo-storage`, `neo-vm`, `neo-io`) so `neo-wallets`
//! keeps a full `Helper` surface without depending on the `neo-core` monolith.

use crate::key_pair::KeyPair;
use crate::transfer_output::TransferOutput;
use crate::wallet::{Wallet, WalletError};
use neo_crypto::Crypto;
use neo_io::serializable::helper::{
    get_var_size, get_var_size_bytes, get_var_size_serializable_slice,
};
use neo_native_contracts::{
    ContractManagement, GasToken, LedgerContract, NativeContract, PolicyContract,
    composition::default_native_services,
};
use neo_primitives::base58_check::{self, AddressDecodeError, Base58CheckDecodeError};
use neo_primitives::{
    CallFlags, ContractBasicMethod, ContractParameterType, TriggerType, UInt160, UInt256,
    Verifiable, WitnessScope,
};
use neo_protocol::ProtocolSettings;
use neo_protocol::get_sign_data_vec;
use neo_protocol::payloads::{Transaction, TransactionAttribute};
use neo_protocol::signer::Signer;
use neo_protocol::transaction::HEADER_SIZE;
use neo_smart_contract::application_engine::ApplicationEngine;
use neo_smart_contract::helper::Helper as ContractHelper;
use neo_storage::persistence::DataCache;
use neo_vm::{OpCode, ScriptBuilder, VmState};
use num_bigint::{BigInt, Sign};
use rand::RngCore;
use rand::rngs::OsRng;
use std::sync::Arc;

/// `PUSHDATA1` opcode byte (matches `neo_vm::OpCode::PUSHDATA1`).
const OP_PUSHDATA1: u8 = 0x0C;
/// `SYSCALL` opcode byte (matches `neo_vm::OpCode::SYSCALL`).
const OP_SYSCALL: u8 = 0x41;
/// Interop service name used by single-signature verification scripts.
const CHECK_SIG_NAME: &str = "System.Crypto.CheckSig";

const SIGNATURE_INVOCATION_SCRIPT_LENGTH: usize = 2 + 64;

/// A helper class related to wallets.
///
/// Matches the C# `Neo.Wallets.Helper` class.
pub struct Helper;

impl Helper {
    /// Signs a verifiable payload with the specified private key (C# `Sign`).
    ///
    /// # Errors
    ///
    /// Returns a human readable error when the signing preimage cannot be
    /// derived or the key fails to sign.
    pub fn sign(
        verifiable: &dyn Verifiable,
        key: &KeyPair,
        network: u32,
    ) -> Result<Vec<u8>, String> {
        let sign_data = get_sign_data_vec(verifiable, network).map_err(|e| e.to_string())?;
        key.sign(&sign_data).map_err(|e| e.to_string())
    }

    /// Converts the specified script hash to a Neo address (C# `ToAddress`).
    #[must_use]
    pub fn to_address(script_hash: &UInt160, version: u8) -> String {
        base58_check::encode_address_payload(version, &script_hash.to_array())
    }

    /// Converts the specified Neo address to a script hash (C# `ToScriptHash`).
    ///
    /// # Errors
    ///
    /// Returns a human readable error when the address is not valid Base58Check,
    /// has an unexpected length, or carries a different address version.
    pub fn to_script_hash(address: &str, version: u8) -> Result<UInt160, String> {
        let script_hash =
            base58_check::decode_address_payload(address, version).map_err(map_address_error)?;
        UInt160::from_bytes(&script_hash).map_err(|e| e.to_string())
    }

    /// Creates a signature redeem (verification) script for a public key.
    ///
    /// Layout: `PUSHDATA1 <len> <pubkey> SYSCALL <checkSigHash>` (40 bytes for a
    /// 33-byte compressed public key). Matches C# `Contract.CreateSignatureRedeemScript`.
    #[must_use]
    pub fn signature_redeem_script(public_key: &[u8]) -> Vec<u8> {
        let mut script = Vec::with_capacity(public_key.len() + 6);
        script.push(OP_PUSHDATA1);
        script.push(public_key.len() as u8);
        script.extend_from_slice(public_key);
        script.push(OP_SYSCALL);
        script.extend_from_slice(&Self::check_sig_hash());
        script
    }

    /// Returns the 4-byte interop hash of `System.Crypto.CheckSig`.
    ///
    /// The Neo interop hash is the first four bytes of `SHA256(name)`; this is
    /// exactly what `neo_vm::interop_hash(name).to_le_bytes()` yields.
    #[must_use]
    pub fn check_sig_hash() -> [u8; 4] {
        let hash = Crypto::sha256(CHECK_SIG_NAME.as_bytes());
        [hash[0], hash[1], hash[2], hash[3]]
    }

    /// XOR operation on two equal-length byte arrays (C# `XOR`).
    ///
    /// # Errors
    ///
    /// Returns an error when the two slices differ in length.
    pub fn xor(x: &[u8], y: &[u8]) -> Result<Vec<u8>, String> {
        if x.len() != y.len() {
            return Err(format!(
                "The x.Length({}) and y.Length({}) must be equal.",
                x.len(),
                y.len()
            ));
        }
        let mut result = vec![0u8; x.len()];
        for (i, result_byte) in result.iter_mut().enumerate() {
            *result_byte = x[i] ^ y[i];
        }
        Ok(result)
    }

    /// Calculates the network fee for the specified transaction, resolving
    /// verification scripts from a wallet when available (C# `CalculateNetworkFee`).
    ///
    /// # Errors
    ///
    /// Returns a human readable error when a witness cannot be priced.
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

    /// Calculates the network fee for the specified transaction using an
    /// explicit account-script resolver (C# `CalculateNetworkFee`).
    ///
    /// # Errors
    ///
    /// Returns a human readable error when a witness cannot be priced.
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
    ///
    /// # Errors
    ///
    /// Returns [`WalletError`] when accounts lack funds or the transaction cannot
    /// be assembled / priced.
    #[allow(clippy::too_many_arguments)]
    pub fn make_transfer_transaction(
        wallet: &dyn Wallet,
        snapshot: &DataCache,
        outputs: &[TransferOutput],
        from: Option<UInt160>,
        cosigners: Option<&[Signer]>,
        settings: &ProtocolSettings,
        persisting_block: Option<&neo_protocol::block::Block>,
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
                    default_native_services(),
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
                    && value.sign() == Sign::Plus
                {
                    balances.push((*account, value));
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
        let script_container: Arc<dyn Verifiable> = Arc::new(tx.clone());
        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            Some(script_container),
            Arc::new(snapshot.clone_cache()),
            persisting_block.cloned(),
            settings.clone(),
            max_gas,
            None,
            default_native_services(),
        )
        .map_err(|e| WalletError::TransactionCreationFailed(e.to_string()))?;
        engine
            .load_script(tx.script().to_vec(), CallFlags::ALL, None)
            .map_err(|e| WalletError::TransactionCreationFailed(e.to_string()))?;
        engine
            .execute()
            .map_err(|e| WalletError::TransactionCreationFailed(e.to_string()))?;
        if engine.state() == VmState::FAULT {
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
    ///
    /// # Errors
    ///
    /// Returns [`WalletError`] when accounts lack funds or the transaction cannot
    /// be assembled / priced.
    #[allow(clippy::too_many_arguments)]
    pub fn make_transaction(
        wallet: &dyn Wallet,
        snapshot: &DataCache,
        script: &[u8],
        sender: Option<UInt160>,
        cosigners: Option<&[Signer]>,
        attributes: Option<&[TransactionAttribute]>,
        settings: &ProtocolSettings,
        persisting_block: Option<&neo_protocol::block::Block>,
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

            let script_container: Arc<dyn Verifiable> = Arc::new(tx.clone());
            let mut engine = ApplicationEngine::new(
                TriggerType::Application,
                Some(script_container),
                Arc::new(snapshot.clone_cache()),
                persisting_block.cloned(),
                settings.clone(),
                max_gas,
                None,
                default_native_services(),
            )
            .map_err(|e| WalletError::TransactionCreationFailed(e.to_string()))?;
            engine
                .load_script(tx.script().to_vec(), CallFlags::ALL, None)
                .map_err(|e| WalletError::TransactionCreationFailed(e.to_string()))?;
            engine
                .execute()
                .map_err(|e| WalletError::TransactionCreationFailed(e.to_string()))?;
            if engine.state() == VmState::FAULT {
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
    // `Transaction::script_hashes_for_verifying` is exactly its signer accounts
    // (see `neo_ledger::validation::resolver`); use the protocol accessor
    // directly so `neo-wallets` need not depend on `neo-ledger`.
    let hashes = tx.signer_accounts();

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

        if witness_script.is_none()
            && let Some(witness) = tx.witnesses().get(index)
        {
            if witness.verification_script.is_empty() {
                invocation_script = Some(witness.invocation_script.clone());
            } else {
                witness_script = Some(witness.verification_script.clone());
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
                    format!(
                        "The smart contract {} haven't got verify method",
                        contract.hash
                    )
                })?
                .clone();

            if method.return_type != ContractParameterType::Boolean {
                return Err("The verify method doesn't return boolean value.".to_string());
            }

            if !method.parameters.is_empty() && invocation_script.is_none() {
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
                default_native_services(),
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
                    "Smart contract {contract_hash} verification fault."
                ));
            }
            let result_item = engine.result_stack().peek(0).map_err(|e| e.to_string())?;
            if !result_item.as_bool().unwrap_or(false) {
                return Err(format!(
                    "Smart contract {contract_hash} verification fault."
                ));
            }

            remaining_execution_cost =
                remaining_execution_cost.saturating_sub(engine.fee_consumed());
            if remaining_execution_cost <= 0 {
                return Err("Insufficient GAS.".to_string());
            }
            network_fee = network_fee.saturating_add(engine.fee_consumed());
            continue;
        }

        if ContractHelper::is_signature_contract(&witness_script) {
            size = size.saturating_add(
                (get_var_size(SIGNATURE_INVOCATION_SCRIPT_LENGTH as u64)
                    + SIGNATURE_INVOCATION_SCRIPT_LENGTH
                    + get_var_size_bytes(&witness_script)) as i64,
            );
            let sig_cost =
                exec_fee_factor.saturating_mul(ContractHelper::signature_contract_cost());
            network_fee = network_fee.saturating_add(sig_cost);
        } else if let Some((m, keys)) = ContractHelper::parse_multi_sig_contract(&witness_script) {
            let n = keys.len();
            let invocation_len = SIGNATURE_INVOCATION_SCRIPT_LENGTH.saturating_mul(m);
            size = size.saturating_add(
                (get_var_size(invocation_len as u64)
                    + invocation_len
                    + get_var_size_bytes(&witness_script)) as i64,
            );
            let multisig_cost = exec_fee_factor.saturating_mul(
                ContractHelper::multi_signature_contract_cost(m as i32, n as i32),
            );
            network_fee = network_fee.saturating_add(multisig_cost);
        } else {
            return Err(format!(
                "Contract-based verification for script hash {} is not yet supported in this build.",
                hex::encode(hash.to_array())
            ));
        }
    }

    network_fee = network_fee.saturating_add(size.saturating_mul(fee_per_byte));
    for attribute in tx.attributes() {
        network_fee = network_fee.saturating_add(attribute_network_fee(attribute, snapshot, tx));
    }

    Ok(network_fee)
}

/// Computes the network fee contributed by a single transaction attribute.
///
/// Reproduces `neo-core`'s `TransactionAttribute::calculate_network_fee`: the
/// policy base fee for the attribute type, scaled by the signer count for
/// `Conflicts` and by `nkeys + 1` for `NotaryAssisted`. Implemented locally
/// because `neo-protocol`'s attribute types cannot depend on the policy
/// contract without introducing a crate cycle.
fn attribute_network_fee(
    attribute: &TransactionAttribute,
    snapshot: &DataCache,
    tx: &Transaction,
) -> i64 {
    let policy = PolicyContract::new();
    let base_fee = policy
        .get_attribute_fee_snapshot(snapshot, attribute.type_id())
        .unwrap_or(PolicyContract::DEFAULT_ATTRIBUTE_FEE as i64);

    match attribute {
        TransactionAttribute::Conflicts(_) => tx.signers().len() as i64 * base_fee,
        TransactionAttribute::NotaryAssisted(attr) => (attr.nkeys as i64 + 1) * base_fee,
        _ => base_fee,
    }
}

/// Finds paying accounts for a required amount (C# `FindPayingAccounts`).
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

/// Base58Check decode extension (C# `Base58CheckDecode`).
pub trait Base58CheckDecode {
    /// Decodes a Base58Check-encoded string into raw bytes, verifying its checksum.
    fn base58_check_decode(&self) -> Result<Vec<u8>, String>;
}

impl Base58CheckDecode for str {
    fn base58_check_decode(&self) -> Result<Vec<u8>, String> {
        base58_check::decode_check(self).map_err(map_base58_check_decode_error)
    }
}

fn map_base58_check_decode_error(err: Base58CheckDecodeError) -> String {
    match err {
        Base58CheckDecodeError::InvalidBase58 { message } => {
            format!("Invalid Base58 string: {message}")
        }
        Base58CheckDecodeError::MissingChecksum => {
            "Invalid Base58Check format: decoded data length is too short (requires at least 4 checksum bytes).".to_string()
        }
        Base58CheckDecodeError::InvalidChecksum => {
            "Invalid Base58Check checksum: provided checksum does not match calculated checksum."
                .to_string()
        }
    }
}

fn map_address_error(err: AddressDecodeError) -> String {
    match err {
        AddressDecodeError::Base58(err) => map_base58_check_decode_error(err),
        AddressDecodeError::InvalidLength { actual, .. } => {
            format!(
                "Invalid address format: expected 21 bytes after Base58Check decoding, but got {actual} bytes. The address may be corrupted or in an invalid format."
            )
        }
        AddressDecodeError::InvalidVersion { expected, actual } => {
            format!(
                "Invalid address version: expected version {expected}, but got {actual}. The address may be for a different network."
            )
        }
    }
}

/// Resolves the verification script bytes for an account script hash, if available.
pub type AccountScriptResolver<'a> = dyn Fn(&UInt160) -> Option<Vec<u8>> + 'a;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_sig_hash_matches_known_interop_hash() {
        // System.Crypto.CheckSig => 0x27b3e756 (little-endian bytes).
        assert_eq!(Helper::check_sig_hash(), [0x56, 0xe7, 0xb3, 0x27]);
    }

    #[test]
    fn signature_redeem_script_has_expected_layout() {
        let pubkey = [0x02u8; 33];
        let script = Helper::signature_redeem_script(&pubkey);
        assert_eq!(script.len(), 40);
        assert_eq!(script[0], OP_PUSHDATA1);
        assert_eq!(script[1], 33);
        assert_eq!(&script[2..35], &pubkey[..]);
        assert_eq!(script[35], OP_SYSCALL);
        assert_eq!(&script[36..40], &Helper::check_sig_hash());
    }

    #[test]
    fn address_round_trip() {
        let script_hash = UInt160::from_script(&Helper::signature_redeem_script(&[0x03u8; 33]));
        let address = Helper::to_address(&script_hash, 0x35);
        let restored = Helper::to_script_hash(&address, 0x35).unwrap();
        assert_eq!(script_hash, restored);
    }

    #[test]
    fn xor_requires_equal_lengths() {
        assert!(!Helper::xor(&[1, 2], &[3]).unwrap_err().is_empty());
        assert_eq!(
            Helper::xor(&[0xff, 0x0f], &[0x0f, 0xff]).unwrap(),
            vec![0xf0, 0xf0]
        );
    }
}
