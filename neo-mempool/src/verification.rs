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
//! Policy/Gas/Ledger state is read through the `neo-native-contracts`
//! readers where they exist (`LedgerContract`, `OracleContract`,
//! `RoleManagement`, `NeoToken::committee_address`) and through the
//! documented storage layouts where they do not (Policy fee-per-byte /
//! exec-fee-factor / blocked accounts / attribute fees, GAS balances) —
//! each such constant is pinned to its C# `PolicyContract` definition.

use neo_config::{Hardfork, ProtocolSettings};
use neo_execution::helper::Helper;
use neo_native_contracts::ledger_contract::LedgerContract;
use neo_native_contracts::{GasToken, NeoToken, Notary, OracleContract, RoleManagement};
use neo_payloads::{MAX_TRANSACTION_SIZE, OracleResponse, Transaction, TransactionAttribute};
use neo_primitives::{UInt160, VerifyResult};
// `invocation_script`/`verification_script` on `Witness` are trait methods.
use neo_primitives::Witness as _;
use neo_storage::DataCache;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

/// C# `PolicyContract.Prefix_FeePerByte` (10).
const POLICY_PREFIX_FEE_PER_BYTE: u8 = 10;
/// C# `PolicyContract.Prefix_BlockedAccount` (15).
const POLICY_PREFIX_BLOCKED_ACCOUNT: u8 = 15;
/// C# `PolicyContract.Prefix_ExecFeeFactor` (18).
const POLICY_PREFIX_EXEC_FEE_FACTOR: u8 = 18;
/// C# `PolicyContract.Prefix_AttributeFee` (20).
const POLICY_PREFIX_ATTRIBUTE_FEE: u8 = 20;
/// C# `PolicyContract.Prefix_MaxValidUntilBlockIncrement` (22, HF_Echidna).
const POLICY_PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT: u8 = 22;
/// C# `PolicyContract.Prefix_MaxTraceableBlocks`.
const POLICY_PREFIX_MAX_TRACEABLE_BLOCKS: u8 = 23;
/// C# `PolicyContract.ID` (-7).
const POLICY_CONTRACT_ID: i32 = -7;
/// C# `PolicyContract.DefaultFeePerByte` (1000 datoshi).
const DEFAULT_FEE_PER_BYTE: i64 = 1_000;
/// C# `PolicyContract.DefaultExecFeeFactor` (30).
const DEFAULT_EXEC_FEE_FACTOR: i64 = 30;
/// C# `PolicyContract.DefaultAttributeFee` (0).
const DEFAULT_ATTRIBUTE_FEE: i64 = 0;
/// Shared NEP-17 `Prefix_Account` (20) — the GAS balance records.
const NEP17_PREFIX_ACCOUNT: u8 = 20;

/// Stateless reader of `PolicyContract` storage and the derived protocol
/// limits the mempool needs during transaction admission.
pub struct PolicyReader;

impl PolicyReader {
    /// Reads a Policy storage value as a little-endian `BigInteger`,
    /// falling back to the C# default when the key is absent (C#
    /// `snapshot.TryGet(key, out item) ? (long)(BigInteger)item : default`).
    fn policy_int(snapshot: &DataCache, key: Vec<u8>, default: i64) -> i64 {
        snapshot
            .get(&neo_storage::StorageKey::new(POLICY_CONTRACT_ID, key))
            .map(|item| BigInt::from_signed_bytes_le(&item.value_bytes()))
            .and_then(|v| v.to_i64())
            .unwrap_or(default)
    }

    /// C# `PolicyContract.IsBlocked` — key existence under
    /// `Prefix_BlockedAccount + account`.
    fn policy_is_blocked(snapshot: &DataCache, account: &UInt160) -> bool {
        let mut key = vec![POLICY_PREFIX_BLOCKED_ACCOUNT];
        key.extend_from_slice(&account.to_bytes());
        snapshot
            .get(&neo_storage::StorageKey::new(POLICY_CONTRACT_ID, key))
            .is_some()
    }

    /// C# `NeoSystemExtensions.GetMaxValidUntilBlockIncrement(snapshot,
    /// settings)`: before HF_Echidna the protocol setting, after it the
    /// Policy storage value (falling back to the setting when the key has
    /// not been initialized yet).
    fn max_valid_until_block_increment(
        snapshot: &DataCache,
        settings: &ProtocolSettings,
        height: u32,
    ) -> u32 {
        if !settings.is_hardfork_enabled(Hardfork::HfEchidna, height) {
            return settings.max_valid_until_block_increment;
        }
        let stored = PolicyReader::policy_int(
            snapshot,
            vec![POLICY_PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT],
            i64::from(settings.max_valid_until_block_increment),
        );
        u32::try_from(stored).unwrap_or(settings.max_valid_until_block_increment)
    }

    /// The effective `MaxTraceableBlocks` (C# `NeoSystem.GetMaxTraceableBlocks`):
    /// the static protocol setting before `HF_Echidna`, the committee-adjustable
    /// Policy storage value (prefix 23) from `HF_Echidna` onward.
    fn max_traceable_blocks_effective(
        snapshot: &DataCache,
        settings: &ProtocolSettings,
        height: u32,
    ) -> u32 {
        if !settings.is_hardfork_enabled(Hardfork::HfEchidna, height) {
            return settings.max_traceable_blocks;
        }
        let stored = PolicyReader::policy_int(
            snapshot,
            vec![POLICY_PREFIX_MAX_TRACEABLE_BLOCKS],
            i64::from(settings.max_traceable_blocks),
        );
        u32::try_from(stored).unwrap_or(settings.max_traceable_blocks)
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

/// C# `Transaction.Sender` — `Signers[0].Account`.
fn sender(tx: &Transaction) -> Option<UInt160> {
    tx.signers().first().map(|s| s.account)
}

/// C# `IVerifiable.GetSignData(network)` — `network (u32 LE) ‖ hash`.
fn sign_data(tx: &Transaction, network: u32) -> Option<Vec<u8>> {
    let hash = tx.try_hash().ok()?;
    let mut data = Vec::with_capacity(36);
    data.extend_from_slice(&network.to_le_bytes());
    data.extend_from_slice(&hash.to_bytes());
    Some(data)
}

/// C# `Transaction.IsSingleSignatureInvocationScript` —
/// `PUSHDATA1 64 ‖ 64-byte signature`, exactly 66 bytes.
fn single_signature_invocation(invocation: &[u8]) -> Option<&[u8]> {
    if invocation.len() != 66 {
        return None;
    }
    if invocation[0] != neo_vm_rs::OpCode::PUSHDATA1.byte() || invocation[1] != 64 {
        return None;
    }
    Some(&invocation[2..66])
}

/// The full C# `Transaction.Verify`: state-independent first, then
/// state-dependent. `pooled_sender_fee` is the verification-context
/// sender-fee total from the memory pool (C#
/// `TransactionVerificationContext._senderFee`); `oracle_duplicate`
/// reports whether the pool already holds a transaction with the same
/// `OracleResponse` id (C# `_oracleResponses`).
pub fn verify_transaction(
    tx: &Transaction,
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    pooled_sender_fee: &BigInt,
    oracle_duplicate: bool,
) -> VerifyResult {
    let result = verify_state_independent(tx, settings);
    if result != VerifyResult::Succeed {
        return result;
    }
    verify_state_dependent(tx, snapshot, settings, pooled_sender_fee, oracle_duplicate)
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
    use neo_io::Serializable;

    let ledger = LedgerContract::new();
    let Ok(height) = ledger.current_index(snapshot) else {
        return VerifyResult::UnableToVerify;
    };

    // Validity window. C# v3.10.0 `Transaction.VerifyStateDependent` splits the
    // two failure modes: an already-passed `ValidUntilBlock` is `Expired`, while
    // one more than `MaxValidUntilBlockIncrement` ahead of the tip is
    // `NotYetValid`. The accept range (`height < VUB <= height + increment`) is
    // unchanged; only the rejection classification differs.
    let max_increment = PolicyReader::max_valid_until_block_increment(snapshot, settings, height);
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

    // C# Blockchain.OnNewTransaction: a traceable on-chain conflict record for
    // this hash, registered by a persisted transaction sharing at least one
    // signer, blocks admission (`ContainsConflictHash` -> HasConflicts).
    let mtb = PolicyReader::max_traceable_blocks_effective(snapshot, settings, height);
    if LedgerContract::new()
        .contains_conflict_hash(snapshot, &tx.hash(), &hashes, mtb)
        .unwrap_or(false)
    {
        return VerifyResult::HasConflicts;
    }

    // Sender GAS balance (C# TransactionVerificationContext.CheckTransaction;
    // `pooled_sender_fee` already carries the pooled-conflict fee rebate
    // applied by `MemoryPool::try_add`'s CheckConflicts).
    let Some(tx_sender) = sender(tx) else {
        return VerifyResult::Invalid;
    };
    let expected_fee =
        BigInt::from(tx.system_fee()) + BigInt::from(tx.network_fee()) + pooled_sender_fee;
    if gas_balance_of(snapshot, &tx_sender) < expected_fee {
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
        if !verify_attribute(snapshot, tx, attribute, height) {
            return VerifyResult::InvalidAttribute;
        }
        attributes_fee =
            attributes_fee.saturating_add(attribute_network_fee(snapshot, tx, attribute));
    }

    // Net fee left for witness verification.
    let fee_per_byte = PolicyReader::policy_int(
        snapshot,
        vec![POLICY_PREFIX_FEE_PER_BYTE],
        DEFAULT_FEE_PER_BYTE,
    );
    let mut net_fee = tx.network_fee() - (tx.size() as i64) * fee_per_byte - attributes_fee;
    if net_fee < 0 {
        return VerifyResult::InsufficientFunds;
    }
    if net_fee > Helper::MAX_VERIFICATION_GAS {
        net_fee = Helper::MAX_VERIFICATION_GAS;
    }

    let exec_fee_factor = PolicyReader::policy_int(
        snapshot,
        vec![POLICY_PREFIX_EXEC_FEE_FACTOR],
        DEFAULT_EXEC_FEE_FACTOR,
    );
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
            match Helper::verify_witness(tx, settings, snapshot, hash, witness, net_fee) {
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

/// C# `TransactionAttribute.Verify` dispatch.
fn verify_attribute(
    snapshot: &DataCache,
    tx: &Transaction,
    attribute: &TransactionAttribute,
    height: u32,
) -> bool {
    match attribute {
        // C# HighPriorityAttribute.Verify: a signer must be the committee
        // multisig address.
        TransactionAttribute::HighPriority => {
            let committee =
                neo_execution::NativeContract::committee_address(&NeoToken::new(), snapshot);
            match committee {
                Ok(Some(committee)) => tx.signers().iter().any(|s| s.account == committee),
                _ => false,
            }
        }
        // C# NotValidBefore.Verify: `CurrentIndex >= Height`.
        TransactionAttribute::NotValidBefore(attr) => height >= attr.height,
        // C# v3.10.0 Conflicts.Verify: reject if the transaction carries
        // duplicate Conflicts attributes referencing the same hash, then
        // require the conflicting hash not be an on-chain transaction.
        TransactionAttribute::Conflicts(attr) => {
            let mut seen = std::collections::HashSet::new();
            let has_duplicate = tx
                .attributes()
                .iter()
                .filter_map(|a| match a {
                    TransactionAttribute::Conflicts(c) => Some(c.hash),
                    _ => None,
                })
                .any(|hash| !seen.insert(hash));
            if has_duplicate {
                return false;
            }
            !LedgerContract::new()
                .contains_transaction(snapshot, &attr.hash)
                .unwrap_or(true)
        }
        // C# OracleResponse.Verify.
        TransactionAttribute::OracleResponse(attr) => {
            if tx
                .signers()
                .iter()
                .any(|s| s.scopes != neo_primitives::WitnessScope::NONE)
            {
                return false;
            }
            let fixed_script = OracleResponse::get_fixed_script();
            if tx.script() != fixed_script.as_slice() {
                return false;
            }
            let Ok(Some(request)) = OracleContract::new().get_request(snapshot, attr.id) else {
                return false;
            };
            if tx.network_fee() + tx.system_fee() != request.gas_for_response {
                return false;
            }
            let Ok(oracles) = RoleManagement::new().get_designated_by_role_at(
                snapshot,
                neo_native_contracts::Role::Oracle,
                height + 1,
            ) else {
                return false;
            };
            let Some(oracle_account) = bft_address(&oracles) else {
                return false;
            };
            tx.signers().iter().any(|s| s.account == oracle_account)
        }
        // C# NotaryAssisted.Verify: the Notary hash must sign; when it is
        // the sender there must be exactly two signers (payer second).
        TransactionAttribute::NotaryAssisted(_) => {
            let notary = Notary::script_hash();
            if sender(tx) == Some(notary) {
                return tx.signers().len() == 2;
            }
            tx.signers().iter().any(|s| s.account == notary)
        }
    }
}

/// C# `TransactionAttribute.CalculateNetworkFee` dispatch: the Policy
/// per-type attribute fee (default 0), times the signer count for
/// `Conflicts` and times `NKeys + 1` for `NotaryAssisted`.
fn attribute_network_fee(
    snapshot: &DataCache,
    tx: &Transaction,
    attribute: &TransactionAttribute,
) -> i64 {
    let base = PolicyReader::policy_int(
        snapshot,
        vec![
            POLICY_PREFIX_ATTRIBUTE_FEE,
            attribute.attribute_type() as u8,
        ],
        DEFAULT_ATTRIBUTE_FEE,
    );
    match attribute {
        TransactionAttribute::Conflicts(_) => base.saturating_mul(tx.signers().len() as i64),
        TransactionAttribute::NotaryAssisted(attr) => {
            base.saturating_mul(i64::from(attr.nkeys) + 1)
        }
        _ => base,
    }
}

/// C# `Contract.GetBFTAddress(pubkeys)` — `m = n - (n - 1) / 3` multisig
/// script hash; `None` for an empty designation.
fn bft_address(pubkeys: &[neo_crypto::ECPoint]) -> Option<UInt160> {
    if pubkeys.is_empty() {
        return None;
    }
    let m = pubkeys.len() - (pubkeys.len() - 1) / 3;
    let script =
        neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(
            m, pubkeys,
        )
        .ok()?;
    Some(UInt160::from_script(&script))
}
