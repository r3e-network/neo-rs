//! Lock-free sync metrics, shared across crates.
//!
//! The block-persist hot path (in neo-blockchain) calls [`record_block`] to
//! feed these atomics; the telemetry layer (in neo-node) reads them for the
//! Prometheus /metrics endpoint. Using a global avoids any dependency cycle
//! (neo-blockchain → neo-runtime ← neo-node).

use std::sync::atomic::{AtomicU64, Ordering};

/// Global best-known live chain tip height reported by connected peers.
/// Set from the network layer (neo-network session) when peers report their
/// chain height in the version handshake. Read by the blockchain service to
/// gate expensive operations (witness verification, indexer, StateService MPT)
/// during catch-up.
static PEER_LIVE_TIP: AtomicU64 = AtomicU64::new(0);

/// Update the global peer-reported live tip height. Called from the network
/// layer when a peer reports its chain height via the version handshake.
pub fn set_peer_live_tip(height: u64) {
    PEER_LIVE_TIP.fetch_max(height, Ordering::Relaxed);
}

/// Read the global peer-reported live tip height.
pub fn peer_live_tip() -> u64 {
    PEER_LIVE_TIP.load(Ordering::Relaxed)
}

static BLOCKS_PERSISTED: AtomicU64 = AtomicU64::new(0);
static HEIGHT: AtomicU64 = AtomicU64::new(0);
static AVG_TOTAL_US: AtomicU64 = AtomicU64::new(0);
static AVG_VERIFY_US: AtomicU64 = AtomicU64::new(0);
static AVG_PERSIST_US: AtomicU64 = AtomicU64::new(0);
static AVG_COMMIT_US: AtomicU64 = AtomicU64::new(0);
static NATIVE_PERSIST_BLOCKS: AtomicU64 = AtomicU64::new(0);
static NATIVE_PERSIST_HEIGHT: AtomicU64 = AtomicU64::new(0);
static NATIVE_PERSIST_AVG_TOTAL_US: AtomicU64 = AtomicU64::new(0);
static NATIVE_PERSIST_AVG_ONPERSIST_US: AtomicU64 = AtomicU64::new(0);
static NATIVE_PERSIST_AVG_TX_US: AtomicU64 = AtomicU64::new(0);
static NATIVE_PERSIST_AVG_POSTPERSIST_US: AtomicU64 = AtomicU64::new(0);
static NATIVE_PERSIST_AVG_COMMIT_US: AtomicU64 = AtomicU64::new(0);
static NATIVE_PERSIST_AVG_TX_COUNT: AtomicU64 = AtomicU64::new(0);
static NEO_TOKEN_ONPERSIST_STAGES: [NativeHookMetricSlot; 7] = [
    NativeHookMetricSlot::new(),
    NativeHookMetricSlot::new(),
    NativeHookMetricSlot::new(),
    NativeHookMetricSlot::new(),
    NativeHookMetricSlot::new(),
    NativeHookMetricSlot::new(),
    NativeHookMetricSlot::new(),
];
static NEO_TOKEN_COMMITTEE_COMPUTE_STAGES: [NativeHookMetricSlot; 7] = [
    NativeHookMetricSlot::new(),
    NativeHookMetricSlot::new(),
    NativeHookMetricSlot::new(),
    NativeHookMetricSlot::new(),
    NativeHookMetricSlot::new(),
    NativeHookMetricSlot::new(),
    NativeHookMetricSlot::new(),
];
static NEO_TOKEN_COMMITTEE_CANDIDATE_COUNTS: [CountMetricSlot; 7] = [
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
    CountMetricSlot::new(),
];

const STANDARD_NATIVE_CONTRACTS: [NativeContractMetricLabel; 11] = [
    NativeContractMetricLabel::new(-1, "ContractManagement"),
    NativeContractMetricLabel::new(-2, "StdLib"),
    NativeContractMetricLabel::new(-3, "CryptoLib"),
    NativeContractMetricLabel::new(-4, "LedgerContract"),
    NativeContractMetricLabel::new(-5, "NeoToken"),
    NativeContractMetricLabel::new(-6, "GasToken"),
    NativeContractMetricLabel::new(-7, "PolicyContract"),
    NativeContractMetricLabel::new(-8, "RoleManagement"),
    NativeContractMetricLabel::new(-9, "OracleContract"),
    NativeContractMetricLabel::new(-10, "Notary"),
    NativeContractMetricLabel::new(-11, "Treasury"),
];

static NATIVE_ONPERSIST_HOOKS: [NativeHookMetricSlot; 11] = [
    NativeHookMetricSlot::new(),
    NativeHookMetricSlot::new(),
    NativeHookMetricSlot::new(),
    NativeHookMetricSlot::new(),
    NativeHookMetricSlot::new(),
    NativeHookMetricSlot::new(),
    NativeHookMetricSlot::new(),
    NativeHookMetricSlot::new(),
    NativeHookMetricSlot::new(),
    NativeHookMetricSlot::new(),
    NativeHookMetricSlot::new(),
];

static NATIVE_POSTPERSIST_HOOKS: [NativeHookMetricSlot; 11] = [
    NativeHookMetricSlot::new(),
    NativeHookMetricSlot::new(),
    NativeHookMetricSlot::new(),
    NativeHookMetricSlot::new(),
    NativeHookMetricSlot::new(),
    NativeHookMetricSlot::new(),
    NativeHookMetricSlot::new(),
    NativeHookMetricSlot::new(),
    NativeHookMetricSlot::new(),
    NativeHookMetricSlot::new(),
    NativeHookMetricSlot::new(),
];

/// Native persistence hook phase recorded for per-contract timing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativePersistHook {
    /// The C# `NativeContract.OnPersistAsync` phase.
    OnPersist,
    /// The C# `NativeContract.PostPersistAsync` phase.
    PostPersist,
}

impl NativePersistHook {
    fn label(self) -> &'static str {
        match self {
            Self::OnPersist => "onpersist",
            Self::PostPersist => "postpersist",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct NativeContractMetricLabel {
    id: i32,
    name: &'static str,
}

impl NativeContractMetricLabel {
    const fn new(id: i32, name: &'static str) -> Self {
        Self { id, name }
    }
}

#[derive(Debug)]
struct NativeHookMetricSlot {
    calls: AtomicU64,
    avg_us: AtomicU64,
}

impl NativeHookMetricSlot {
    const fn new() -> Self {
        Self {
            calls: AtomicU64::new(0),
            avg_us: AtomicU64::new(0),
        }
    }
}

#[derive(Debug)]
struct CountMetricSlot {
    samples: AtomicU64,
    total: AtomicU64,
    avg: AtomicU64,
}

impl CountMetricSlot {
    const fn new() -> Self {
        Self {
            samples: AtomicU64::new(0),
            total: AtomicU64::new(0),
            avg: AtomicU64::new(0),
        }
    }
}

/// Snapshot of one native-contract hook metric series.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeContractHookStats {
    /// Hook trigger label, either `onpersist` or `postpersist`.
    pub trigger: &'static str,
    /// Stable native contract id from the Neo N3 standard native catalog.
    pub contract_id: i32,
    /// Stable native contract name from the Neo N3 standard native catalog.
    pub contract: &'static str,
    /// Total hook calls recorded since process start.
    pub calls: u64,
    /// EWMA hook duration in microseconds.
    pub avg_us: u64,
}

const NEO_TOKEN_ONPERSIST_STAGE_ORDER: [NeoTokenOnPersistStage; 7] = [
    NeoTokenOnPersistStage::Skip,
    NeoTokenOnPersistStage::ReadCachedCommittee,
    NeoTokenOnPersistStage::ComputeCommittee,
    NeoTokenOnPersistStage::WriteCommittee,
    NeoTokenOnPersistStage::CompareCommittee,
    NeoTokenOnPersistStage::NotifyCommitteeChanged,
    NeoTokenOnPersistStage::RefreshTotal,
];
const NEO_TOKEN_COMMITTEE_COMPUTE_STAGE_ORDER: [NeoTokenCommitteeComputeStage; 7] = [
    NeoTokenCommitteeComputeStage::ReadVotersCount,
    NeoTokenCommitteeComputeStage::StandbyLookup,
    NeoTokenCommitteeComputeStage::CandidateScanTotal,
    NeoTokenCommitteeComputeStage::CandidatePubkeyDecode,
    NeoTokenCommitteeComputeStage::CandidateStateDecode,
    NeoTokenCommitteeComputeStage::CandidateBlockedLookup,
    NeoTokenCommitteeComputeStage::TopCandidateMaintenance,
];
const NEO_TOKEN_COMMITTEE_CANDIDATE_COUNT_ORDER: [NeoTokenCommitteeCandidateCount; 7] = [
    NeoTokenCommitteeCandidateCount::StorageEntries,
    NeoTokenCommitteeCandidateCount::MalformedKeys,
    NeoTokenCommitteeCandidateCount::DecodedEntries,
    NeoTokenCommitteeCandidateCount::RegisteredEntries,
    NeoTokenCommitteeCandidateCount::BlockedRegistered,
    NeoTokenCommitteeCandidateCount::EligibleCandidates,
    NeoTokenCommitteeCandidateCount::TopCandidates,
];

/// Fine-grained stages inside `NeoToken.OnPersist`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NeoTokenOnPersistStage {
    /// Non-refresh block fast path.
    Skip,
    /// Read and deserialize the previous cached committee.
    ReadCachedCommittee,
    /// Recompute the next committee from voters/candidates.
    ComputeCommittee,
    /// Encode and write the refreshed committee cache.
    WriteCommittee,
    /// Compare old/new committee keys and decide whether to notify.
    CompareCommittee,
    /// Build and emit `CommitteeChanged`.
    NotifyCommitteeChanged,
    /// Total time for a refresh block after the fast-path check.
    RefreshTotal,
}

impl NeoTokenOnPersistStage {
    fn label(self) -> &'static str {
        match self {
            Self::Skip => "skip",
            Self::ReadCachedCommittee => "read_cached_committee",
            Self::ComputeCommittee => "compute_committee",
            Self::WriteCommittee => "write_committee",
            Self::CompareCommittee => "compare_committee",
            Self::NotifyCommitteeChanged => "notify_committee_changed",
            Self::RefreshTotal => "refresh_total",
        }
    }

    fn slot_index(self) -> usize {
        match self {
            Self::Skip => 0,
            Self::ReadCachedCommittee => 1,
            Self::ComputeCommittee => 2,
            Self::WriteCommittee => 3,
            Self::CompareCommittee => 4,
            Self::NotifyCommitteeChanged => 5,
            Self::RefreshTotal => 6,
        }
    }
}

/// Snapshot of one `NeoToken.OnPersist` stage metric series.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NeoTokenOnPersistStageStats {
    /// Stage label used in Prometheus output.
    pub stage: &'static str,
    /// Total stage observations recorded since process start.
    pub calls: u64,
    /// EWMA stage duration in microseconds.
    pub avg_us: u64,
}

/// Fine-grained stages inside `NeoToken.ComputeCommitteeMembers`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NeoTokenCommitteeComputeStage {
    /// Read `Prefix_VotersCount`.
    ReadVotersCount,
    /// Read standby-candidate votes for a fallback committee.
    StandbyLookup,
    /// Total scan over `Prefix_Candidate`.
    CandidateScanTotal,
    /// Decode candidate pubkeys from storage-key suffixes.
    CandidatePubkeyDecode,
    /// Decode `CandidateState` values.
    CandidateStateDecode,
    /// Check the Policy blocked-account list for registered candidates.
    CandidateBlockedLookup,
    /// Maintain the top-M candidate vector.
    TopCandidateMaintenance,
}

impl NeoTokenCommitteeComputeStage {
    fn label(self) -> &'static str {
        match self {
            Self::ReadVotersCount => "read_voters_count",
            Self::StandbyLookup => "standby_lookup",
            Self::CandidateScanTotal => "candidate_scan_total",
            Self::CandidatePubkeyDecode => "candidate_pubkey_decode",
            Self::CandidateStateDecode => "candidate_state_decode",
            Self::CandidateBlockedLookup => "candidate_blocked_lookup",
            Self::TopCandidateMaintenance => "top_candidate_maintenance",
        }
    }

    fn slot_index(self) -> usize {
        match self {
            Self::ReadVotersCount => 0,
            Self::StandbyLookup => 1,
            Self::CandidateScanTotal => 2,
            Self::CandidatePubkeyDecode => 3,
            Self::CandidateStateDecode => 4,
            Self::CandidateBlockedLookup => 5,
            Self::TopCandidateMaintenance => 6,
        }
    }
}

/// Candidate-scan item counters recorded per `ComputeCommitteeMembers` scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NeoTokenCommitteeCandidateCount {
    /// Raw storage entries returned by the candidate-prefix seek.
    StorageEntries,
    /// Candidate keys that cannot yield a valid 33-byte EC point.
    MalformedKeys,
    /// Candidate state values successfully decoded.
    DecodedEntries,
    /// Entries whose `CandidateState.Registered` flag is true.
    RegisteredEntries,
    /// Registered candidates blocked by `PolicyContract`.
    BlockedRegistered,
    /// Registered, non-blocked candidates eligible for committee selection.
    EligibleCandidates,
    /// Final top-M candidates kept after the scan.
    TopCandidates,
}

impl NeoTokenCommitteeCandidateCount {
    fn label(self) -> &'static str {
        match self {
            Self::StorageEntries => "storage_entries",
            Self::MalformedKeys => "malformed_keys",
            Self::DecodedEntries => "decoded_entries",
            Self::RegisteredEntries => "registered_entries",
            Self::BlockedRegistered => "blocked_registered",
            Self::EligibleCandidates => "eligible_candidates",
            Self::TopCandidates => "top_candidates",
        }
    }

    fn slot_index(self) -> usize {
        match self {
            Self::StorageEntries => 0,
            Self::MalformedKeys => 1,
            Self::DecodedEntries => 2,
            Self::RegisteredEntries => 3,
            Self::BlockedRegistered => 4,
            Self::EligibleCandidates => 5,
            Self::TopCandidates => 6,
        }
    }
}

/// Snapshot of one `NeoToken.ComputeCommitteeMembers` stage metric series.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NeoTokenCommitteeComputeStageStats {
    /// Stage label used in Prometheus output.
    pub stage: &'static str,
    /// Total stage observations recorded since process start.
    pub calls: u64,
    /// EWMA stage duration in microseconds.
    pub avg_us: u64,
}

/// Snapshot of one candidate-scan count metric series.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NeoTokenCommitteeCandidateCountStats {
    /// Count label used in Prometheus output.
    pub kind: &'static str,
    /// Number of scans that have recorded this count.
    pub samples: u64,
    /// Cumulative item count since process start.
    pub total: u64,
    /// EWMA item count per scan.
    pub avg: u64,
}

/// Record a block persist with per-stage timing. Called from the
/// blockchain-service hot path. Lock-free.
pub fn record_block(height: u64, verify_us: u64, persist_us: u64, commit_us: u64, total_us: u64) {
    BLOCKS_PERSISTED.fetch_add(1, Ordering::Relaxed);
    HEIGHT.store(height, Ordering::Relaxed);
    ewma(&AVG_TOTAL_US, total_us);
    ewma(&AVG_VERIFY_US, verify_us);
    ewma(&AVG_PERSIST_US, persist_us);
    ewma(&AVG_COMMIT_US, commit_us);
}

/// Record the native-contract persistence sub-stages for one block.
///
/// This splits the broad [`record_block`] persist bucket into the C#
/// `Blockchain.Persist` phases: native `OnPersist`, per-transaction
/// Application-trigger execution, native `PostPersist`, and the staged cache
/// merge back into the caller snapshot.
pub fn record_native_persist(
    height: u64,
    tx_count: u64,
    onpersist_us: u64,
    tx_us: u64,
    postpersist_us: u64,
    cache_commit_us: u64,
    total_us: u64,
) {
    NATIVE_PERSIST_BLOCKS.fetch_add(1, Ordering::Relaxed);
    NATIVE_PERSIST_HEIGHT.store(height, Ordering::Relaxed);
    ewma(&NATIVE_PERSIST_AVG_TOTAL_US, total_us);
    ewma(&NATIVE_PERSIST_AVG_ONPERSIST_US, onpersist_us);
    ewma(&NATIVE_PERSIST_AVG_TX_US, tx_us);
    ewma(&NATIVE_PERSIST_AVG_POSTPERSIST_US, postpersist_us);
    ewma(&NATIVE_PERSIST_AVG_COMMIT_US, cache_commit_us);
    ewma(&NATIVE_PERSIST_AVG_TX_COUNT, tx_count);
}

/// Record one standard native-contract persistence hook call.
///
/// The C# native-contract catalog uses consecutive negative ids from `-1`
/// through `-11`; using that fixed protocol id keeps the block hot path free of
/// maps, strings, and allocations while still exposing human-readable labels.
pub fn record_native_contract_hook(hook: NativePersistHook, contract_id: i32, elapsed_us: u64) {
    let Some(slot_index) = standard_native_contract_slot(contract_id) else {
        return;
    };
    let slot = match hook {
        NativePersistHook::OnPersist => &NATIVE_ONPERSIST_HOOKS[slot_index],
        NativePersistHook::PostPersist => &NATIVE_POSTPERSIST_HOOKS[slot_index],
    };
    slot.calls.fetch_add(1, Ordering::Relaxed);
    ewma(&slot.avg_us, elapsed_us);
}

/// Record one fine-grained `NeoToken.OnPersist` stage.
pub fn record_neo_token_onpersist_stage(stage: NeoTokenOnPersistStage, elapsed_us: u64) {
    let slot = &NEO_TOKEN_ONPERSIST_STAGES[stage.slot_index()];
    slot.calls.fetch_add(1, Ordering::Relaxed);
    ewma(&slot.avg_us, elapsed_us);
}

/// Record one fine-grained `NeoToken.ComputeCommitteeMembers` stage.
pub fn record_neo_token_committee_compute_stage(
    stage: NeoTokenCommitteeComputeStage,
    elapsed_us: u64,
) {
    let slot = &NEO_TOKEN_COMMITTEE_COMPUTE_STAGES[stage.slot_index()];
    slot.calls.fetch_add(1, Ordering::Relaxed);
    ewma(&slot.avg_us, elapsed_us);
}

/// Record one candidate-scan item count for `NeoToken.ComputeCommitteeMembers`.
pub fn record_neo_token_committee_candidate_count(
    kind: NeoTokenCommitteeCandidateCount,
    count: u64,
) {
    let slot = &NEO_TOKEN_COMMITTEE_CANDIDATE_COUNTS[kind.slot_index()];
    slot.samples.fetch_add(1, Ordering::Relaxed);
    slot.total.fetch_add(count, Ordering::Relaxed);
    ewma(&slot.avg, count);
}

/// Current node height.
pub fn height() -> u64 {
    HEIGHT.load(Ordering::Relaxed)
}

/// Total blocks persisted since startup.
pub fn blocks_persisted() -> u64 {
    BLOCKS_PERSISTED.load(Ordering::Relaxed)
}

/// EWMA per-block total time (microseconds).
pub fn avg_total_us() -> u64 {
    AVG_TOTAL_US.load(Ordering::Relaxed)
}

/// EWMA witness-verification time (microseconds).
pub fn avg_verify_us() -> u64 {
    AVG_VERIFY_US.load(Ordering::Relaxed)
}

/// EWMA native-contract-execution time (microseconds).
pub fn avg_persist_us() -> u64 {
    AVG_PERSIST_US.load(Ordering::Relaxed)
}

/// EWMA RocksDB-commit time (microseconds).
pub fn avg_commit_us() -> u64 {
    AVG_COMMIT_US.load(Ordering::Relaxed)
}

/// Total native persistence records since startup.
pub fn native_persist_blocks() -> u64 {
    NATIVE_PERSIST_BLOCKS.load(Ordering::Relaxed)
}

/// Latest block height observed by native persistence metrics.
pub fn native_persist_height() -> u64 {
    NATIVE_PERSIST_HEIGHT.load(Ordering::Relaxed)
}

/// EWMA total `persist_block_natives` time (microseconds).
pub fn native_persist_avg_total_us() -> u64 {
    NATIVE_PERSIST_AVG_TOTAL_US.load(Ordering::Relaxed)
}

/// EWMA native OnPersist hook time (microseconds).
pub fn native_persist_avg_onpersist_us() -> u64 {
    NATIVE_PERSIST_AVG_ONPERSIST_US.load(Ordering::Relaxed)
}

/// EWMA per-transaction Application stage time (microseconds).
pub fn native_persist_avg_tx_us() -> u64 {
    NATIVE_PERSIST_AVG_TX_US.load(Ordering::Relaxed)
}

/// EWMA native PostPersist hook time (microseconds).
pub fn native_persist_avg_postpersist_us() -> u64 {
    NATIVE_PERSIST_AVG_POSTPERSIST_US.load(Ordering::Relaxed)
}

/// EWMA staged cache merge time inside native persistence (microseconds).
pub fn native_persist_avg_cache_commit_us() -> u64 {
    NATIVE_PERSIST_AVG_COMMIT_US.load(Ordering::Relaxed)
}

/// EWMA transaction count per native persistence call.
pub fn native_persist_avg_tx_count() -> u64 {
    NATIVE_PERSIST_AVG_TX_COUNT.load(Ordering::Relaxed)
}

/// Snapshot per-contract native hook metrics for telemetry rendering.
pub fn native_contract_hook_stats() -> Vec<NativeContractHookStats> {
    let mut stats = Vec::with_capacity(STANDARD_NATIVE_CONTRACTS.len() * 2);
    append_native_contract_hook_stats(
        &mut stats,
        NativePersistHook::OnPersist,
        &NATIVE_ONPERSIST_HOOKS,
    );
    append_native_contract_hook_stats(
        &mut stats,
        NativePersistHook::PostPersist,
        &NATIVE_POSTPERSIST_HOOKS,
    );
    stats
}

/// Snapshot fine-grained `NeoToken.OnPersist` stage metrics.
pub fn neo_token_onpersist_stage_stats() -> Vec<NeoTokenOnPersistStageStats> {
    NEO_TOKEN_ONPERSIST_STAGE_ORDER
        .iter()
        .map(|stage| {
            let slot = &NEO_TOKEN_ONPERSIST_STAGES[stage.slot_index()];
            NeoTokenOnPersistStageStats {
                stage: stage.label(),
                calls: slot.calls.load(Ordering::Relaxed),
                avg_us: slot.avg_us.load(Ordering::Relaxed),
            }
        })
        .collect()
}

/// Snapshot fine-grained `NeoToken.ComputeCommitteeMembers` stage metrics.
pub fn neo_token_committee_compute_stage_stats() -> Vec<NeoTokenCommitteeComputeStageStats> {
    NEO_TOKEN_COMMITTEE_COMPUTE_STAGE_ORDER
        .iter()
        .map(|stage| {
            let slot = &NEO_TOKEN_COMMITTEE_COMPUTE_STAGES[stage.slot_index()];
            NeoTokenCommitteeComputeStageStats {
                stage: stage.label(),
                calls: slot.calls.load(Ordering::Relaxed),
                avg_us: slot.avg_us.load(Ordering::Relaxed),
            }
        })
        .collect()
}

/// Snapshot candidate-scan item count metrics for `NeoToken.ComputeCommitteeMembers`.
pub fn neo_token_committee_candidate_count_stats() -> Vec<NeoTokenCommitteeCandidateCountStats> {
    NEO_TOKEN_COMMITTEE_CANDIDATE_COUNT_ORDER
        .iter()
        .map(|kind| {
            let slot = &NEO_TOKEN_COMMITTEE_CANDIDATE_COUNTS[kind.slot_index()];
            NeoTokenCommitteeCandidateCountStats {
                kind: kind.label(),
                samples: slot.samples.load(Ordering::Relaxed),
                total: slot.total.load(Ordering::Relaxed),
                avg: slot.avg.load(Ordering::Relaxed),
            }
        })
        .collect()
}

fn append_native_contract_hook_stats(
    stats: &mut Vec<NativeContractHookStats>,
    hook: NativePersistHook,
    slots: &[NativeHookMetricSlot; 11],
) {
    for (label, slot) in STANDARD_NATIVE_CONTRACTS.iter().zip(slots.iter()) {
        stats.push(NativeContractHookStats {
            trigger: hook.label(),
            contract_id: label.id,
            contract: label.name,
            calls: slot.calls.load(Ordering::Relaxed),
            avg_us: slot.avg_us.load(Ordering::Relaxed),
        });
    }
}

fn standard_native_contract_slot(contract_id: i32) -> Option<usize> {
    if (-11..=-1).contains(&contract_id) {
        Some((-contract_id - 1) as usize)
    } else {
        None
    }
}

fn ewma(slot: &AtomicU64, sample: u64) {
    let prev = slot.load(Ordering::Relaxed);
    let updated = if prev == 0 {
        sample
    } else {
        let diff = (sample as i64 - prev as i64) / 16;
        (prev as i64 + diff).max(0) as u64
    };
    slot.store(updated, Ordering::Relaxed);
}
