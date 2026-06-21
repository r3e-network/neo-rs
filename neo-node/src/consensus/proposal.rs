use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_consensus::messages::{ConsensusPayload, PrepareRequestMessage};
use neo_consensus::{ChangeViewReason, ConsensusMessageType, ValidatorInfo};
use neo_crypto::ECPoint;
use neo_io::{Serializable, serializable::helper::SerializeHelper};
use neo_mempool::{MemoryPool, PoolItem, verify_transaction};
use neo_native_contracts::{LedgerContract, PolicyContract};
use neo_payloads::{Transaction, TransactionAttribute, Witness};
use neo_primitives::{UInt160, UInt256, VerifyResult};
use neo_storage::persistence::DataCache;
use neo_vm::script_builder::RedeemScript;
use num_bigint::BigInt;
use tracing::warn;

use super::DBFT_MAX_BLOCK_SYSTEM_FEE;

/// Resolves the full transactions for `hashes`, in block order, from the
/// proposal cache then the live mempool. Returns `None` if any is missing.
pub(super) fn resolve_transactions(
    hashes: &[UInt256],
    cache: &HashMap<UInt256, Arc<Transaction>>,
    mempool: &MemoryPool,
) -> Option<Vec<Transaction>> {
    let mut out = Vec::with_capacity(hashes.len());
    for hash in hashes {
        if let Some(tx) = cache.get(hash) {
            out.push((**tx).clone());
        } else if let Some(item) = mempool.get(hash) {
            out.push((*item.transaction).clone());
        } else {
            return None;
        }
    }
    Some(out)
}

#[derive(Default)]
struct ProposalVerificationContext {
    transactions: HashMap<UInt256, Arc<Transaction>>,
    sender_fees: HashMap<UInt160, BigInt>,
    oracle_responses: HashSet<u64>,
}

impl ProposalVerificationContext {
    fn add_transaction(&mut self, tx: Arc<Transaction>) {
        let hash = tx.hash();
        if let Some(sender) = tx.signers().first().map(|signer| signer.account) {
            let fee = BigInt::from(tx.system_fee()) + BigInt::from(tx.network_fee());
            self.sender_fees
                .entry(sender)
                .and_modify(|total| *total += &fee)
                .or_insert(fee);
        }
        if let Some(id) = oracle_response_id(&tx) {
            self.oracle_responses.insert(id);
        }
        self.transactions.insert(hash, tx);
    }

    fn sender_fee(&self, tx: &Transaction) -> BigInt {
        tx.signers()
            .first()
            .and_then(|signer| self.sender_fees.get(&signer.account))
            .cloned()
            .unwrap_or_default()
    }

    fn has_oracle_response(&self, tx: &Transaction) -> bool {
        oracle_response_id(tx).is_some_and(|id| self.oracle_responses.contains(&id))
    }
}

#[derive(Debug, Default)]
pub(super) struct ProposalTransactionAvailability {
    pub(super) available: Vec<UInt256>,
    pub(super) rejection_reason: Option<ChangeViewReason>,
}

/// The BFT threshold `M = N - (N-1)/3` used by C# dBFT.
fn dbft_bft_threshold(n: usize) -> usize {
    RedeemScript::bft_threshold(n)
}

fn dbft_multisig_verification_script(validators: &[ValidatorInfo]) -> Vec<u8> {
    if validators.is_empty() {
        return Vec::new();
    }

    let keys: Vec<ECPoint> = validators
        .iter()
        .map(|validator| validator.public_key.clone())
        .collect();
    RedeemScript::multi_sig_redeem_script_from_points(dbft_bft_threshold(keys.len()), &keys)
        .expect("valid dBFT validator set")
}

/// Mirrors C# `ConsensusContext.GetExpectedBlockSizeWithoutTransactions`.
pub(super) fn expected_dbft_block_size_without_transactions(
    expected_transactions: usize,
    validators: &[ValidatorInfo],
) -> usize {
    let witness =
        Witness::new_with_scripts(Vec::new(), dbft_multisig_verification_script(validators));
    4 + 32
        + 32
        + 8
        + 8
        + 4
        + 1
        + 20
        + 1
        + witness.size()
        + SerializeHelper::get_var_size_usize(expected_transactions)
}

fn proposed_block_policy_rejection(
    hashes: &[UInt256],
    cache: &HashMap<UInt256, Arc<Transaction>>,
    validators: &[ValidatorInfo],
    settings: &ProtocolSettings,
) -> Option<ChangeViewReason> {
    let mut block_size = expected_dbft_block_size_without_transactions(hashes.len(), validators);
    let mut system_fee = 0i128;

    for hash in hashes {
        let tx = cache.get(hash)?;
        block_size = block_size.saturating_add(<Transaction as Serializable>::size(tx.as_ref()));
        system_fee += i128::from(tx.system_fee());
    }

    if block_size > settings.max_block_size as usize {
        warn!(
            target: "neo",
            block_size,
            max_block_size = settings.max_block_size,
            "rejected PrepareRequest: expected block size exceeds dBFT policy"
        );
        return Some(ChangeViewReason::BlockRejectedByPolicy);
    }

    if system_fee > i128::from(DBFT_MAX_BLOCK_SYSTEM_FEE) {
        warn!(
            target: "neo",
            system_fee,
            max_block_system_fee = DBFT_MAX_BLOCK_SYSTEM_FEE,
            "rejected PrepareRequest: expected block system fee exceeds dBFT policy"
        );
        return Some(ChangeViewReason::BlockRejectedByPolicy);
    }

    None
}

pub(super) fn select_primary_proposal_transactions(
    candidates: Vec<PoolItem>,
    max_count: usize,
    cache: &mut HashMap<UInt256, Arc<Transaction>>,
    validators: &[ValidatorInfo],
    settings: &ProtocolSettings,
) -> Vec<UInt256> {
    let candidates: Vec<PoolItem> = candidates.into_iter().take(max_count).collect();
    let mut block_size =
        expected_dbft_block_size_without_transactions(candidates.len(), validators);
    let mut system_fee = 0i128;
    let mut hashes = Vec::with_capacity(candidates.len());

    for item in candidates {
        let next_block_size = block_size.saturating_add(<Transaction as Serializable>::size(
            item.transaction.as_ref(),
        ));
        if next_block_size > settings.max_block_size as usize {
            break;
        }

        let next_system_fee = system_fee + i128::from(item.transaction.system_fee());
        if next_system_fee > i128::from(DBFT_MAX_BLOCK_SYSTEM_FEE) {
            break;
        }

        block_size = next_block_size;
        system_fee = next_system_fee;
        let hash = item.hash();
        cache.insert(hash, Arc::clone(&item.transaction));
        hashes.push(hash);
    }

    hashes
}

fn conflict_hashes(tx: &Transaction) -> impl Iterator<Item = UInt256> + '_ {
    tx.attributes()
        .iter()
        .filter_map(|attribute| match attribute {
            TransactionAttribute::Conflicts(conflict) => Some(conflict.hash),
            _ => None,
        })
}

fn oracle_response_id(tx: &Transaction) -> Option<u64> {
    tx.attributes()
        .iter()
        .find_map(|attribute| match attribute {
            TransactionAttribute::OracleResponse(response) => Some(response.id),
            _ => None,
        })
}

fn verify_unverified_proposal_transaction(
    tx: &Transaction,
    proposal_hashes: &HashSet<UInt256>,
    context: &ProposalVerificationContext,
    snapshot: &DataCache,
    settings: &ProtocolSettings,
) -> VerifyResult {
    if conflict_hashes(tx).any(|hash| proposal_hashes.contains(&hash)) {
        return VerifyResult::HasConflicts;
    }
    if context
        .transactions
        .values()
        .any(|accepted| conflict_hashes(accepted).any(|hash| hash == tx.hash()))
    {
        return VerifyResult::HasConflicts;
    }

    let sender_fee = context.sender_fee(tx);
    verify_transaction(
        tx,
        snapshot,
        settings,
        &sender_fee,
        context.has_oracle_response(tx),
    )
}

pub(super) fn proposal_rejection_reason(result: VerifyResult) -> ChangeViewReason {
    if result == VerifyResult::PolicyFail {
        ChangeViewReason::TxRejectedByPolicy
    } else {
        ChangeViewReason::TxInvalid
    }
}

/// Caches the full transactions named by a primary proposal and returns the
/// subset currently available locally.
///
/// C# DBFT `OnPrepareRequestReceived` first accepts already-verified mempool
/// transactions, then re-verifies unverified matches with the proposal-local
/// `TransactionVerificationContext` (`AddTransaction(tx, true)`). That context
/// catches proposal-internal conflicts, duplicated oracle responses, and sender
/// fee exhaustion across transactions before the backup reports availability.
pub(super) fn cache_available_proposal_transactions(
    hashes: &[UInt256],
    cache: &mut HashMap<UInt256, Arc<Transaction>>,
    mempool: &MemoryPool,
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    validators: &[ValidatorInfo],
) -> ProposalTransactionAvailability {
    let proposal_hashes: HashSet<UInt256> = hashes.iter().copied().collect();
    let mut context = ProposalVerificationContext::default();
    let mut unverified = Vec::new();
    let mut result = ProposalTransactionAvailability {
        available: Vec::with_capacity(hashes.len()),
        rejection_reason: None,
    };

    for hash in hashes {
        if let Some(item) = mempool.get_verified(hash) {
            cache.insert(*hash, Arc::clone(&item.transaction));
            result.available.push(*hash);
            context.add_transaction(item.transaction);
        } else if let Some(item) = mempool.get(hash) {
            unverified.push((*hash, item.transaction));
        }
    }

    for (hash, tx) in unverified {
        let verify_result = verify_unverified_proposal_transaction(
            &tx,
            &proposal_hashes,
            &context,
            snapshot,
            settings,
        );
        if verify_result != VerifyResult::Succeed {
            warn!(
                target: "neo",
                %hash,
                ?verify_result,
                "unverified PrepareRequest transaction failed proposal-context verification"
            );
            result.rejection_reason = Some(proposal_rejection_reason(verify_result));
            return result;
        }
        cache.insert(hash, Arc::clone(&tx));
        result.available.push(hash);
        context.add_transaction(tx);
    }

    if result.available.len() == hashes.len() {
        result.rejection_reason =
            proposed_block_policy_rejection(hashes, cache, validators, settings);
        if result.rejection_reason.is_some() {
            result.available.clear();
        }
    }

    result
}

/// C# DBFT `OnPrepareRequestReceived` rejects proposals that name a transaction
/// already persisted in Ledger, and rejects available local transactions whose
/// hash has a traceable on-chain conflict record.
pub(super) fn prepare_request_passes_ledger_guards(
    payload: &ConsensusPayload,
    snapshot: &DataCache,
    mempool: &MemoryPool,
    settings: &ProtocolSettings,
) -> bool {
    if payload.message_type != ConsensusMessageType::PrepareRequest {
        return true;
    }

    let request = match PrepareRequestMessage::deserialize_body(
        &payload.data,
        payload.block_index,
        payload.view_number,
        payload.validator_index,
    ) {
        Ok(request) => request,
        Err(_) => return true,
    };

    let ledger = LedgerContract::new();
    for hash in &request.transaction_hashes {
        match ledger.contains_transaction(snapshot, hash) {
            Ok(true) => {
                warn!(target: "neo", %hash, "rejected PrepareRequest: transaction already exists on-chain");
                return false;
            }
            Ok(false) => {}
            Err(error) => {
                warn!(target: "neo", %hash, %error, "failed to check PrepareRequest transaction existence");
                return false;
            }
        }
    }

    let max_traceable_blocks = match PolicyContract::new()
        .get_max_traceable_blocks_snapshot(snapshot, settings)
    {
        Ok(value) => value,
        Err(error) => {
            warn!(target: "neo", %error, "failed to read MaxTraceableBlocks for PrepareRequest guard");
            return false;
        }
    };

    for hash in &request.transaction_hashes {
        let Some(item) = mempool.get(hash) else {
            continue;
        };
        let signers: Vec<UInt160> = item
            .transaction
            .signers()
            .iter()
            .map(|signer| signer.account)
            .collect();
        match ledger.contains_conflict_hash(snapshot, hash, &signers, max_traceable_blocks) {
            Ok(true) => {
                warn!(target: "neo", %hash, "rejected PrepareRequest: transaction has on-chain conflict");
                return false;
            }
            Ok(false) => {}
            Err(error) => {
                warn!(target: "neo", %hash, %error, "failed to check PrepareRequest transaction conflict");
                return false;
            }
        }
    }

    true
}
