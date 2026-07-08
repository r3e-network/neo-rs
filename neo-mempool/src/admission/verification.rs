//! Transaction verification for mempool admission.
//!
//! Ports C# `Transaction.Verify` (`neo_csharp/src/Neo/Network/P2P/Payloads/
//! Transaction.cs:308`): [`verify_state_independent`] (Transaction.cs:371 —
//! size, strict script parse, standard single-sig / multisig fast-path
//! signature checks) followed by [`verify_state_dependent`]
//! (Transaction.cs:323 — expiry window, blocked-account policy, sender GAS
//! balance, per-attribute verification + fees, fee-per-byte coverage, and
//! engine-based witness verification for non-standard witnesses).
//!
//! In C# the state-independent half runs in `TransactionRouter` (the
//! parallel preverifier) and the state-dependent half inside
//! `MemoryPool.TryAdd`; the observable behavior of relayed transactions is
//! the combination, which is what [`verify_transaction`] produces for the
//! single-threaded admission path here.
//!
//! Policy/Gas/Ledger state is read through provider-style seams or the
//! canonical `neo-native-contracts` readers where they exist. The Ledger reads
//! are centralized behind [`AdmissionLedgerProvider`] so the admission path
//! depends on capabilities, not concrete Ledger construction at every call
//! site. Each direct storage constant remains pinned to its C# definition.

use neo_config::{Hardfork, ProtocolSettings};
use neo_execution::helper::Helper;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_native_contracts::{GasToken, Notary, PolicyContract, StandardNativeProvider};
use neo_payloads::{MAX_TRANSACTION_SIZE, Transaction, TransactionAttribute};
use neo_primitives::{UInt160, VerifyResult};
// `invocation_script`/`verification_script` on `Witness` are trait methods.
use neo_primitives::Witness as _;
use neo_storage::DataCache;

#[path = "../verification/attributes.rs"]
mod attributes;
use attributes::verify_attribute;
use num_bigint::BigInt;
use std::sync::Arc;

use super::ledger_provider::{AdmissionLedgerProvider, NativeAdmissionLedgerProvider};

/// Stateless reader of `PolicyContract` storage and the derived protocol
/// limits the mempool needs during transaction admission.
pub struct PolicyReader;

impl PolicyReader {
    /// C# `PolicyContract.IsBlocked` — key existence under
    /// `Prefix_BlockedAccount + account`.
    fn policy_is_blocked(snapshot: &DataCache, account: &UInt160) -> bool {
        PolicyContract::is_blocked_snapshot(snapshot, account)
    }

    /// C# `NeoSystemExtensions.GetMaxValidUntilBlockIncrement(snapshot,
    /// settings)`: before HF_Echidna the protocol setting, after it the
    /// Policy storage value (falling back to the setting when the key has
    /// not been initialized yet).
    fn max_valid_until_block_increment(
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> neo_error::CoreResult<u32> {
        PolicyContract::new().get_max_valid_until_block_increment_snapshot(snapshot, settings)
    }
}

/// C# `NativeContract.GAS.BalanceOf(snapshot, account)`: the first field of the
/// interoperable NEP-17 `AccountState` struct stored under
/// `Prefix_Account + account`; an absent or undecodable record is zero.
///
/// Delegates to the single canonical decode in `neo-native-contracts` so the
/// mempool fee check cannot drift from the contract's own balance reader.
pub fn gas_balance_of(snapshot: &DataCache, account: &UInt160) -> BigInt {
    GasToken::balance_of(snapshot, account).unwrap_or_else(|_| BigInt::from(0))
}

/// C# v3.10.1 `MemoryPool.GetPayer` balance side: Notary-sponsored
/// transactions (`Sender == Notary.Hash` and a second signer exists) spend the
/// second signer's Notary deposit. Ordinary transactions spend the sender's GAS
/// balance.
fn fee_payer_balance(snapshot: &DataCache, tx: &Transaction) -> Option<BigInt> {
    let sender = sender(tx)?;
    if sender == Notary::script_hash() && tx.signers().len() >= 2 {
        let payer = tx.signers()[1].account;
        Some(Notary::balance_of(snapshot, &payer).unwrap_or_else(|_| BigInt::from(0)))
    } else {
        Some(gas_balance_of(snapshot, &sender))
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

/// The full C# `Transaction.Verify`: state-independent first, then
/// state-dependent. `pooled_sender_fee` is the verification-context
/// sender-fee total from the memory pool (C#
/// v3.10.1 `TransactionVerificationContext._senderFee` payer-tuple total);
/// `oracle_duplicate`
/// reports whether the pool already holds a transaction with the same
/// `OracleResponse` id (C# `_oracleResponses`).
pub fn verify_transaction(
    tx: &Transaction,
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    pooled_sender_fee: &BigInt,
    oracle_duplicate: bool,
) -> VerifyResult {
    verify_transaction_with_native_provider(
        tx,
        snapshot,
        settings,
        pooled_sender_fee,
        oracle_duplicate,
        Arc::new(StandardNativeProvider::new()),
    )
}

/// The full C# `Transaction.Verify` using an explicit native-contract provider.
pub fn verify_transaction_with_native_provider(
    tx: &Transaction,
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    pooled_sender_fee: &BigInt,
    oracle_duplicate: bool,
    native_contract_provider: Arc<dyn NativeContractProvider>,
) -> VerifyResult {
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

/// Runs only [`verify_state_dependent`], skipping
/// [`verify_state_independent`]. Used when the state-independent
/// result is already cached (e.g. from `TransactionRouter::preverify`
/// → `PreverifyCompleted::cached_state_independent`), avoiding
/// redundant ECDSA signature verification. C# achieves the same
/// by caching `Transaction.VerificationResult` which `MemoryPool.
/// TryAdd` reads before performing state-dependent checks only.
pub fn verify_transaction_dependent_only(
    tx: &Transaction,
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    pooled_sender_fee: &BigInt,
    oracle_duplicate: bool,
) -> VerifyResult {
    verify_transaction_dependent_only_with_native_provider(
        tx,
        snapshot,
        settings,
        pooled_sender_fee,
        oracle_duplicate,
        Arc::new(StandardNativeProvider::new()),
    )
}

/// Runs only [`verify_state_dependent_with_native_provider`], skipping
/// [`verify_state_independent`], using an explicit native-contract provider.
pub fn verify_transaction_dependent_only_with_native_provider(
    tx: &Transaction,
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    pooled_sender_fee: &BigInt,
    oracle_duplicate: bool,
    native_contract_provider: Arc<dyn NativeContractProvider>,
) -> VerifyResult {
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
    if neo_vm_rs::validate_strict_script(tx.script()).is_err() {
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

/// C# `Transaction.VerifyStateDependent` (Transaction.cs:323).
pub fn verify_state_dependent(
    tx: &Transaction,
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    pooled_sender_fee: &BigInt,
    oracle_duplicate: bool,
) -> VerifyResult {
    verify_state_dependent_with_native_provider(
        tx,
        snapshot,
        settings,
        pooled_sender_fee,
        oracle_duplicate,
        Arc::new(StandardNativeProvider::new()),
    )
}

/// C# `Transaction.VerifyStateDependent` (Transaction.cs:323) using an explicit
/// native-contract provider for engine-based witness verification.
pub fn verify_state_dependent_with_native_provider(
    tx: &Transaction,
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    pooled_sender_fee: &BigInt,
    oracle_duplicate: bool,
    native_contract_provider: Arc<dyn NativeContractProvider>,
) -> VerifyResult {
    let ledger_provider = NativeAdmissionLedgerProvider::new();
    verify_state_dependent_with_providers(
        tx,
        snapshot,
        settings,
        pooled_sender_fee,
        oracle_duplicate,
        native_contract_provider,
        &ledger_provider,
    )
}

fn verify_state_dependent_with_providers(
    tx: &Transaction,
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    pooled_sender_fee: &BigInt,
    oracle_duplicate: bool,
    native_contract_provider: Arc<dyn NativeContractProvider>,
    ledger_provider: &impl AdmissionLedgerProvider,
) -> VerifyResult {
    use neo_io::Serializable;

    let Ok(height) = ledger_provider.current_index(snapshot) else {
        return VerifyResult::UnableToVerify;
    };

    // Validity window. C# v3.10.0 `Transaction.VerifyStateDependent` splits the
    // two failure modes: an already-passed `ValidUntilBlock` is `Expired`, while
    // one more than `MaxValidUntilBlockIncrement` ahead of the tip is
    // `NotYetValid`. The accept range (`height < VUB <= height + increment`) is
    // unchanged; only the rejection classification differs.
    let Ok(max_increment) = PolicyReader::max_valid_until_block_increment(snapshot, settings)
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
        if PolicyReader::policy_is_blocked(snapshot, hash) {
            return VerifyResult::PolicyFail;
        }
    }

    // Sender GAS balance (C# TransactionVerificationContext.CheckTransaction;
    // `pooled_sender_fee` already carries the pooled-conflict fee rebate
    // applied by `MemoryPool::try_add`'s CheckConflicts).
    let Some(balance) = fee_payer_balance(snapshot, tx) else {
        return VerifyResult::Invalid;
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
        if !verify_attribute(ledger_provider, snapshot, tx, attribute, height) {
            return VerifyResult::InvalidAttribute;
        }
        attributes_fee =
            attributes_fee.saturating_add(attribute_network_fee(snapshot, tx, attribute));
    }

    // Net fee left for witness verification.
    let policy = PolicyContract::new();
    let Ok(fee_per_byte) = policy.get_fee_per_byte_snapshot(snapshot).map(i64::from) else {
        return VerifyResult::UnableToVerify;
    };
    let mut net_fee = tx.network_fee() - (tx.size() as i64) * fee_per_byte - attributes_fee;
    if net_fee < 0 {
        return VerifyResult::InsufficientFunds;
    }
    if net_fee > Helper::MAX_VERIFICATION_GAS {
        net_fee = Helper::MAX_VERIFICATION_GAS;
    }

    let Ok(exec_fee_factor) = policy
        .get_exec_fee_factor_snapshot(snapshot, settings, height)
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
                Some(Arc::clone(&native_contract_provider)),
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
fn attribute_network_fee(
    snapshot: &DataCache,
    tx: &Transaction,
    attribute: &TransactionAttribute,
) -> i64 {
    attribute.calculate_network_fee(snapshot, tx)
}

#[cfg(test)]
#[path = "../tests/admission/verification.rs"]
mod tests;
