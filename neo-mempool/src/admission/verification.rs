//! Transaction verification for mempool admission.
//!
//! Ports C# `Transaction.Verify` (`neo_csharp/src/Neo/Network/P2P/Payloads/
//! Transaction.cs:308`): [`verify_state_independent`] (Transaction.cs:371 -
//! size, strict script parse, standard single-sig / multisig fast-path
//! signature checks) followed by [`verify_state_dependent_with_native_provider`]
//! (Transaction.cs:323 — expiry window, blocked-account policy, sender GAS
//! balance, per-attribute verification + fees, fee-per-byte coverage, and
//! engine-based witness verification for non-standard witnesses).
//!
//! Admission runs the state-independent half before taking the pool write lock
//! and the state-dependent half while the pooled fee/conflict context is
//! frozen. [`verify_transaction_with_native_provider`] remains the combined
//! entry point for callers that need raw transaction verification without pool
//! admission.
//!
//! Policy/GAS/Notary/Oracle/Role/Ledger state is read through provider-style
//! seams so the admission path depends on capabilities, not concrete native
//! contract construction at every call site. Each direct storage constant
//! remains pinned to its C# definition.

use neo_config::{Hardfork, ProtocolSettings};
use neo_execution::helper::Helper;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_payloads::{MAX_TRANSACTION_SIZE, Transaction, TransactionAttribute};
use neo_primitives::{UInt160, VerifyResult};
use neo_storage::{CacheRead, DataCache, StorageKey};

#[path = "../verification/attributes.rs"]
mod attributes;
use attributes::verify_attribute;
use neo_error::CoreResult;
use num_bigint::BigInt;
use std::sync::Arc;

use super::ledger_provider::{AdmissionLedgerProvider, NativeAdmissionLedgerProvider};
use super::native_provider::{AdmissionNativeProvider, NativeAdmissionProvider};
const POLICY_CONTRACT_ID: i32 = -7;
const POLICY_PREFIX_ATTRIBUTE_FEE: u8 = 20;
const DEFAULT_ATTRIBUTE_FEE: i64 = 0;

/// C# v3.10.1 `MemoryPool.GetPayer` balance side: Notary-sponsored
/// transactions (`Sender == Notary.Hash` and a second signer exists) spend the
/// second signer's Notary deposit. Ordinary transactions spend the sender's GAS
/// balance.
fn fee_payer_balance<B: CacheRead>(
    snapshot: &DataCache<B>,
    tx: &Transaction,
    native_provider: &impl AdmissionNativeProvider,
) -> CoreResult<Option<BigInt>> {
    let Some(sender) = sender(tx) else {
        return Ok(None);
    };
    if sender == native_provider.notary_hash()? && tx.signers().len() >= 2 {
        let payer = tx.signers()[1].account;
        Ok(Some(native_provider.notary_balance(snapshot, &payer)?))
    } else {
        Ok(Some(native_provider.gas_balance(snapshot, &sender)?))
    }
}

/// C# `Transaction.Sender` — `Signers[0].Account`.
fn sender(tx: &Transaction) -> Option<UInt160> {
    tx.signers().first().map(|s| s.account)
}

/// C# `IVerifiable.GetSignData(network)` — `network (u32 LE) ‖ hash`. Single
/// canonical preimage builder lives in `neo_payloads`.
fn sign_data(tx: &Transaction, network: u32) -> Option<Vec<u8>> {
    neo_payloads::get_sign_data_vec(tx, network).ok()
}

/// C# `Transaction.IsSingleSignatureInvocationScript` —
/// `PUSHDATA1 64 ‖ 64-byte signature`, exactly 66 bytes.
///
/// Delegates to the single canonical parser in `neo-vm` so the mempool cannot
/// drift from the shared `PUSHDATA1 0x40 <64-byte sig>` shape check.
fn single_signature_invocation(invocation: &[u8]) -> Option<&[u8]> {
    neo_vm::script_builder::signature_from_invocation(invocation)
}

/// The full C# `Transaction.Verify` using an explicit native-contract provider.
pub fn verify_transaction_with_native_provider<B, P>(
    tx: &Transaction,
    snapshot: &DataCache<B>,
    settings: &ProtocolSettings,
    pooled_sender_fee: &BigInt,
    oracle_duplicate: bool,
    native_contract_provider: Arc<P>,
) -> VerifyResult
where
    B: CacheRead,
    P: NativeContractProvider + 'static,
{
    let result = verify_state_independent(tx, settings);
    if result != VerifyResult::Succeed {
        return result;
    }
    verify_state_dependent_with_native_provider(
        tx,
        snapshot,
        settings,
        pooled_sender_fee,
        oracle_duplicate,
        native_contract_provider,
    )
}

/// C# `Transaction.VerifyStateIndependent` (Transaction.cs:371).
pub fn verify_state_independent(tx: &Transaction, settings: &ProtocolSettings) -> VerifyResult {
    use neo_io::Serializable;

    // Size check (C# `Size > MaxTransactionSize`).
    if tx.size() > MAX_TRANSACTION_SIZE {
        return VerifyResult::OverSize;
    }

    // Strict script parse (C# `new Script(Script, true)` throwing
    // BadScriptException).
    if neo_vm::validate_strict_script(tx.script()).is_err() {
        return VerifyResult::InvalidScript;
    }

    // Standard-signature fast paths. C# indexes `Witnesses[i]` for every
    // signer hash; a missing witness throws (surfacing as Invalid).
    let hashes: Vec<UInt160> = tx.signers().iter().map(|s| s.account).collect();
    let witnesses = tx.witnesses();
    if witnesses.len() < hashes.len() {
        return VerifyResult::Invalid;
    }
    let Some(message) = sign_data(tx, settings.network) else {
        return VerifyResult::Invalid;
    };

    for (hash, witness) in hashes.iter().zip(witnesses.iter()) {
        let verification = witness.verification_script();
        let invocation = witness.invocation_script();
        if neo_vm::script_builder::redeem_script::RedeemScript::is_signature_contract(verification)
        {
            let Some(signature) = single_signature_invocation(invocation) else {
                continue; // not the fast-path shape: verified state-dependently
            };
            if *hash != witness.script_hash() {
                return VerifyResult::Invalid;
            }
            let pubkey = &verification[2..35];
            match neo_crypto::ecc::EcdsaVerify::verify_signature_secp256r1(
                pubkey, &message, signature,
            ) {
                Ok(true) => {}
                Ok(false) => return VerifyResult::InvalidSignature,
                Err(_) => return VerifyResult::Invalid,
            }
        } else if let Some((m, points)) =
            neo_vm::script_builder::redeem_script::RedeemScript::parse_multi_sig_contract(
                verification,
            )
        {
            let Some(signatures) =
                neo_vm::script_builder::redeem_script::RedeemScript::parse_multi_sig_invocation(
                    invocation, m,
                )
            else {
                continue;
            };
            if *hash != witness.script_hash() {
                return VerifyResult::Invalid;
            }
            let n = points.len();
            let (mut x, mut y) = (0usize, 0usize);
            while x < m && y < n {
                match neo_crypto::ecc::EcdsaVerify::verify_signature_secp256r1(
                    &points[y],
                    &message,
                    &signatures[x],
                ) {
                    Ok(true) => x += 1,
                    Ok(false) => {}
                    Err(_) => return VerifyResult::Invalid,
                }
                y += 1;
                if m - x > n - y {
                    return VerifyResult::InvalidSignature;
                }
            }
        }
    }
    VerifyResult::Succeed
}

/// C# `Transaction.VerifyStateDependent` (Transaction.cs:323) using an explicit
/// native-contract provider for engine-based witness verification.
pub fn verify_state_dependent_with_native_provider<B, P>(
    tx: &Transaction,
    snapshot: &DataCache<B>,
    settings: &ProtocolSettings,
    pooled_sender_fee: &BigInt,
    oracle_duplicate: bool,
    native_contract_provider: Arc<P>,
) -> VerifyResult
where
    B: CacheRead,
    P: NativeContractProvider + 'static,
{
    let ledger_provider = NativeAdmissionLedgerProvider::new();
    let admission_native_provider = NativeAdmissionProvider::new(native_contract_provider.clone());
    verify_state_dependent_with_providers(
        tx,
        snapshot,
        settings,
        pooled_sender_fee,
        oracle_duplicate,
        native_contract_provider,
        &ledger_provider,
        &admission_native_provider,
    )
}

/// Runs state-dependent verification against the canonical ledger provider
/// selected by node composition.
pub(crate) fn verify_state_dependent_with_ledger_provider<B, P, L>(
    tx: &Transaction,
    snapshot: &DataCache<B>,
    settings: &ProtocolSettings,
    pooled_sender_fee: &BigInt,
    oracle_duplicate: bool,
    native_contract_provider: Arc<P>,
    ledger_provider: &L,
) -> VerifyResult
where
    B: CacheRead,
    P: NativeContractProvider + 'static,
    L: AdmissionLedgerProvider,
{
    let admission_native_provider = NativeAdmissionProvider::new(native_contract_provider.clone());
    verify_state_dependent_with_providers(
        tx,
        snapshot,
        settings,
        pooled_sender_fee,
        oracle_duplicate,
        native_contract_provider,
        ledger_provider,
        &admission_native_provider,
    )
}

fn verify_state_dependent_with_providers<B, P>(
    tx: &Transaction,
    snapshot: &DataCache<B>,
    settings: &ProtocolSettings,
    pooled_sender_fee: &BigInt,
    oracle_duplicate: bool,
    native_contract_provider: Arc<P>,
    ledger_provider: &impl AdmissionLedgerProvider,
    admission_native_provider: &impl AdmissionNativeProvider,
) -> VerifyResult
where
    B: CacheRead,
    P: NativeContractProvider + 'static,
{
    use neo_io::Serializable;

    let Ok(height) = ledger_provider.current_index(snapshot) else {
        return VerifyResult::UnableToVerify;
    };

    // Validity window. C# v3.10.1 `Transaction.VerifyStateDependent` splits the
    // two failure modes: an already-passed `ValidUntilBlock` is `Expired`, while
    // one more than `MaxValidUntilBlockIncrement` ahead of the tip is
    // `NotYetValid`. The accept range (`height < VUB <= height + increment`) is
    // unchanged; only the rejection classification differs.
    let Ok(max_increment) =
        admission_native_provider.max_valid_until_block_increment(snapshot, settings)
    else {
        return VerifyResult::UnableToVerify;
    };
    if tx.valid_until_block() <= height {
        return VerifyResult::Expired;
    }
    if tx.valid_until_block() > height.saturating_add(max_increment) {
        return VerifyResult::NotYetValid;
    }

    // Blocked accounts.
    let hashes: Vec<UInt160> = tx.signers().iter().map(|s| s.account).collect();
    for hash in &hashes {
        match admission_native_provider.policy_is_blocked(snapshot, hash) {
            Ok(true) => return VerifyResult::PolicyFail,
            Ok(false) => {}
            Err(_) => return VerifyResult::UnableToVerify,
        }
    }

    // Sender GAS balance (C# TransactionVerificationContext.CheckTransaction;
    // `pooled_sender_fee` already carries the pooled-conflict fee rebate
    // applied by `MemoryPool::add_transaction`'s CheckConflicts).
    let balance = match fee_payer_balance(snapshot, tx, admission_native_provider) {
        Ok(Some(balance)) => balance,
        Ok(None) => return VerifyResult::Invalid,
        Err(_) => return VerifyResult::UnableToVerify,
    };
    let expected_fee =
        BigInt::from(tx.system_fee()) + BigInt::from(tx.network_fee()) + pooled_sender_fee;
    if balance < expected_fee {
        return VerifyResult::InsufficientFunds;
    }
    if oracle_duplicate {
        return VerifyResult::InsufficientFunds;
    }

    // Attributes: hardfork gating, per-attribute verification, fees.
    let mut attributes_fee: i64 = 0;
    for attribute in tx.attributes() {
        if matches!(attribute, TransactionAttribute::NotaryAssisted(_))
            && !settings.is_hardfork_enabled(Hardfork::HfEchidna, height)
        {
            return VerifyResult::InvalidAttribute;
        }
        if !verify_attribute(
            ledger_provider,
            admission_native_provider,
            snapshot,
            tx,
            attribute,
            height,
        ) {
            return VerifyResult::InvalidAttribute;
        }
        attributes_fee =
            attributes_fee.saturating_add(attribute_network_fee(snapshot, tx, attribute));
    }

    // Net fee left for witness verification.
    let Ok(fee_per_byte) = admission_native_provider
        .fee_per_byte(snapshot)
        .map(i64::from)
    else {
        return VerifyResult::UnableToVerify;
    };
    let mut net_fee = tx.network_fee() - (tx.size() as i64) * fee_per_byte - attributes_fee;
    if net_fee < 0 {
        return VerifyResult::InsufficientFunds;
    }
    if net_fee > Helper::MAX_VERIFICATION_GAS {
        net_fee = Helper::MAX_VERIFICATION_GAS;
    }

    let Ok(exec_fee_factor) = admission_native_provider
        .exec_fee_factor(snapshot, settings, height)
        .map(i64::from)
    else {
        return VerifyResult::UnableToVerify;
    };
    let witnesses = tx.witnesses();
    if witnesses.len() < hashes.len() {
        return VerifyResult::Invalid;
    }
    for (hash, witness) in hashes.iter().zip(witnesses.iter()) {
        let verification = witness.verification_script();
        let invocation = witness.invocation_script();
        let is_single = neo_vm::script_builder::redeem_script::RedeemScript::is_signature_contract(
            verification,
        ) && single_signature_invocation(invocation).is_some();
        let multi = neo_vm::script_builder::redeem_script::RedeemScript::parse_multi_sig_contract(
            verification,
        )
        .and_then(|(m, points)| {
            neo_vm::script_builder::redeem_script::RedeemScript::parse_multi_sig_invocation(
                invocation, m,
            )
            .map(|_| (m, points.len()))
        });
        if is_single {
            // `Helper::signature_contract_cost` returns C# OpCodePrices
            // execution units (PUSHDATA1×2 + SYSCALL + CheckSigPrice);
            // C# Transaction.cs:350 multiplies by ExecFeeFactor for the
            // datoshi cost.
            net_fee -= exec_fee_factor * Helper::signature_contract_cost();
        } else if let Some((m, n)) = multi {
            net_fee -= exec_fee_factor * Helper::multi_signature_contract_cost(m as i32, n as i32);
        } else {
            match Helper::verify_witness_with_native_provider(
                tx,
                settings,
                snapshot,
                hash,
                witness,
                net_fee,
                native_contract_provider.clone(),
            ) {
                Ok(fee) => net_fee -= fee,
                Err(_) => return VerifyResult::Invalid,
            }
        }
        if net_fee < 0 {
            return VerifyResult::InsufficientFunds;
        }
    }
    VerifyResult::Succeed
}

/// C# `TransactionAttribute.CalculateNetworkFee` dispatch.
fn attribute_network_fee<B: CacheRead>(
    snapshot: &DataCache<B>,
    tx: &Transaction,
    attribute: &TransactionAttribute,
) -> i64 {
    let key = StorageKey::new(
        POLICY_CONTRACT_ID,
        vec![POLICY_PREFIX_ATTRIBUTE_FEE, attribute.type_id().to_byte()],
    );
    let base = snapshot
        .get(&key)
        .and_then(|item| i64::try_from(BigInt::from_signed_bytes_le(&item.value_bytes())).ok())
        .unwrap_or(DEFAULT_ATTRIBUTE_FEE);
    match attribute {
        TransactionAttribute::Conflicts(_) => tx.signers().len() as i64 * base,
        TransactionAttribute::NotaryAssisted(attr) => (i64::from(attr.nkeys) + 1) * base,
        _ => base,
    }
}

#[cfg(test)]
#[path = "../tests/admission/verification.rs"]
mod tests;
