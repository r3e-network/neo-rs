use super::*;
/// Bounded review-quarantine policy for this runtime.
///
/// Mirrors `neo-mempool`'s node-local quarantine policy, but this crate is
/// deliberately free of workspace-internal dependencies and keys its evidence by
/// a hex transaction hash, so it keeps its own bounded DTO instead of importing
/// the mempool and its ledger/storage/VM dependency graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuarantineConfig {
    pub automatic_enabled: bool,
    pub max_entries: usize,
    pub max_ttl_ms: u64,
    pub selection_scan_budget: usize,
}

/// Evidence-side mirror of the mempool's quarantine record, keyed by a hex hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuarantineMetadata {
    pub transaction_hash: String,
    pub evidence_refs: Vec<String>,
    pub context: String,
    pub created_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub rule_version: String,
    pub episode: u8,
    pub cumulative_active_ms: u64,
}

#[doc(hidden)]
pub struct EvidenceRegistry {
    capacity: usize,
    order: VecDeque<String>,
    pub records: HashMap<String, EvidenceRecord>,
}

impl EvidenceRegistry {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            order: VecDeque::with_capacity(capacity.max(1)),
            records: HashMap::with_capacity(capacity.max(1)),
        }
    }

    pub fn insert(&mut self, record: EvidenceRecord) {
        if !self.records.contains_key(&record.id) {
            while self.records.len() >= self.capacity {
                if let Some(oldest) = self.order.pop_front() {
                    self.records.remove(&oldest);
                }
            }
            self.order.push_back(record.id.clone());
        }
        self.records.insert(record.id.clone(), record);
    }

    pub(super) fn get(&self, id: &str) -> Option<&EvidenceRecord> {
        self.records.get(id)
    }
}

pub(super) struct ExecutionRepetition {
    pub(super) signature: String,
    pub(super) evidence_ids: Vec<String>,
    pub(super) candidate_emitted: bool,
}

/// Accumulated deterministic transaction-validation failure evidence for one
/// transaction under the current rule version. The bounded local mempool
/// quarantine escalates only after repeated deterministic failures or
/// cross-height reproduction (plan §43), never from a single failure.
pub(super) struct ValidationFailure {
    pub(super) rule_version: String,
    pub(super) deterministic_failures: u32,
    pub(super) heights: BTreeSet<u32>,
}

/// Deterministic failures (same transaction, same rule version) required
/// before the bounded local mempool quarantine may escalate.
pub(super) const QUARANTINE_EVIDENCE_FAILURES: u32 = 2;
