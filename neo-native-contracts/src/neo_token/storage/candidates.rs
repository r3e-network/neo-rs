use super::*;
use neo_runtime::sync_metrics::{self, NeoTokenCommitteeCandidateCount};
use std::{
    collections::{HashMap, HashSet},
    sync::{OnceLock, RwLock},
    time::Instant,
};

static CANDIDATE_SIGNATURE_ACCOUNT_CACHE: OnceLock<RwLock<HashMap<ECPoint, UInt160>>> =
    OnceLock::new();

#[derive(Default)]
pub(super) struct CandidateScanCounts {
    /// Candidate storage rows visited under `Prefix_Candidate`.
    pub(super) storage_entries: u64,
    /// Rows whose key is too short, or whose registered candidate public key
    /// cannot be decompressed.
    pub(super) malformed_keys: u64,
    /// Rows whose candidate state decoded successfully. The committee scan
    /// decodes state before public keys so unregistered rows do not pay the
    /// ECPoint decompression cost.
    pub(super) decoded_entries: u64,
    /// Decoded rows flagged as registered before blocked-account filtering.
    pub(super) registered_entries: u64,
    /// Registered candidate rows skipped because their signature account is
    /// blocked by the policy contract.
    pub(super) blocked_registered: u64,
    /// Registered and unblocked candidates considered for committee ranking.
    pub(super) eligible_candidates: u64,
}

impl CandidateScanCounts {
    pub(super) fn record(&self, top_candidates: u64) {
        sync_metrics::record_neo_token_committee_candidate_count(
            NeoTokenCommitteeCandidateCount::StorageEntries,
            self.storage_entries,
        );
        sync_metrics::record_neo_token_committee_candidate_count(
            NeoTokenCommitteeCandidateCount::MalformedKeys,
            self.malformed_keys,
        );
        sync_metrics::record_neo_token_committee_candidate_count(
            NeoTokenCommitteeCandidateCount::DecodedEntries,
            self.decoded_entries,
        );
        sync_metrics::record_neo_token_committee_candidate_count(
            NeoTokenCommitteeCandidateCount::RegisteredEntries,
            self.registered_entries,
        );
        sync_metrics::record_neo_token_committee_candidate_count(
            NeoTokenCommitteeCandidateCount::BlockedRegistered,
            self.blocked_registered,
        );
        sync_metrics::record_neo_token_committee_candidate_count(
            NeoTokenCommitteeCandidateCount::EligibleCandidates,
            self.eligible_candidates,
        );
        sync_metrics::record_neo_token_committee_candidate_count(
            NeoTokenCommitteeCandidateCount::TopCandidates,
            top_candidates,
        );
    }
}

pub(super) fn candidate_is_blocked(snapshot: &DataCache, pubkey: &ECPoint) -> bool {
    let account = candidate_signature_account(pubkey);
    snapshot
        .get(&crate::PolicyContract::blocked_account_key(&account))
        .is_some()
}

pub(super) fn candidate_is_blocked_in(
    blocked_accounts: &HashSet<UInt160>,
    pubkey: &ECPoint,
) -> bool {
    blocked_accounts.contains(&candidate_signature_account(pubkey))
}

pub(in crate::neo_token) fn candidate_signature_account(pubkey: &ECPoint) -> UInt160 {
    let cache = CANDIDATE_SIGNATURE_ACCOUNT_CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    if let Some(account) = cache.read().unwrap_or_else(|e| e.into_inner()).get(pubkey) {
        return *account;
    }
    let account = UInt160::from_script(&Contract::create_signature_redeem_script(pubkey.clone()));
    *cache
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .entry(pubkey.clone())
        .or_insert(account)
}

pub(super) fn elapsed_us(start: Instant) -> u64 {
    start.elapsed().as_micros().min(u64::MAX as u128) as u64
}

pub(super) fn push_top_committee_candidate(
    top: &mut Vec<(ECPoint, BigInt)>,
    limit: usize,
    candidate: (ECPoint, BigInt),
) {
    if limit == 0 {
        return;
    }
    if top.len() == limit
        && !committee_candidate_order(&candidate, top.last().expect("top is full")).is_lt()
    {
        return;
    }

    let insert_at = top
        .binary_search_by(|existing| committee_candidate_order(existing, &candidate))
        .unwrap_or_else(|index| index);
    top.insert(insert_at, candidate);
    if top.len() > limit {
        top.pop();
    }
}

fn committee_candidate_order(a: &(ECPoint, BigInt), b: &(ECPoint, BigInt)) -> std::cmp::Ordering {
    b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0))
}
