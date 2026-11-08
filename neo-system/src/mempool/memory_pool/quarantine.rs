use super::MemoryPool;
use crate::mempool::pool_item::PoolItem;
use neo_primitives::UInt256;
use neo_protocol::payloads::Transaction;
use std::sync::Arc;
use thiserror::Error;

pub const QUARANTINE_TTL_MS: u64 = 30_000;
pub const QUARANTINE_MAX_EPISODES: u8 = 2;
pub const QUARANTINE_MAX_CUMULATIVE_MS: u64 = 60_000;
pub const QUARANTINE_ABSOLUTE_IMPACT_LIMIT: usize = 1_024;
pub const QUARANTINE_PROPOSAL_SCAN_BUDGET: usize = 2_048;

/// Bounded configuration for node-local proposal quarantine metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuarantineConfig {
    /// Explicitly enables automatic local proposal quarantine.
    pub automatic_enabled: bool,
    /// Maximum number of live metadata records.
    pub max_entries: usize,
    /// Maximum lifetime accepted for one episode, capped at 30 seconds.
    pub max_ttl_ms: u64,
    /// Maximum verified transactions inspected per local proposal selection.
    pub selection_scan_budget: usize,
}

impl Default for QuarantineConfig {
    fn default() -> Self {
        Self {
            automatic_enabled: false,
            max_entries: QUARANTINE_ABSOLUTE_IMPACT_LIMIT,
            max_ttl_ms: QUARANTINE_TTL_MS,
            selection_scan_budget: QUARANTINE_PROPOSAL_SCAN_BUDGET,
        }
    }
}

/// Independent node-local quarantine metadata for a transaction hash.
///
/// This record never changes the transaction's verified or unverified state and
/// is ignored by all network receive, synchronization, and replay paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuarantineMetadata {
    /// Transaction hash bound to the evidence.
    pub transaction_hash: UInt256,
    /// Deterministic evidence identifiers supporting the quarantine.
    pub evidence_refs: Vec<String>,
    /// Bounded, non-secret policy context.
    pub context: String,
    /// Record creation time.
    pub created_at_unix_ms: u64,
    /// Automatic expiry time.
    pub expires_at_unix_ms: u64,
    /// Deterministic rule version that produced the record.
    pub rule_version: String,
    /// One-based episode number assigned by the pool.
    pub episode: u8,
    /// Total bounded lifetime committed across all episodes for this hash.
    pub cumulative_active_ms: u64,
}

/// Bounded, non-blocking review candidate emitted when an episode expires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuarantineExpiryReview {
    /// Transaction restored to local proposal eligibility.
    pub transaction_hash: UInt256,
    /// Deterministic evidence identifiers from the expired episode.
    pub evidence_refs: Vec<String>,
    /// Bounded policy context from the expired episode.
    pub context: String,
    /// Deterministic rule version from the expired episode.
    pub rule_version: String,
    /// Scheduled expiry time of the episode.
    pub expired_at_unix_ms: u64,
    /// One-based episode number.
    pub episode: u8,
    /// Cumulative active lifetime after this episode.
    pub cumulative_active_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct QuarantineHistory {
    pub(super) episodes: u8,
    pub(super) cumulative_ms: u64,
}

/// Quarantine metadata validation or capacity failure.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum QuarantineError {
    /// Automatic quarantine is an explicit opt-in capability.
    #[error("automatic transaction quarantine is disabled")]
    Disabled,
    /// Configuration values must all be non-zero and bounded.
    #[error("quarantine limits and scan budget must be non-zero and bounded")]
    InvalidConfig,
    /// The record is expired, unbounded, or missing deterministic evidence.
    #[error("quarantine metadata is invalid")]
    InvalidMetadata,
    /// The configured metadata bound was reached.
    #[error("quarantine metadata capacity reached")]
    CapacityReached,
    /// The one-percent local impact ceiling was reached.
    #[error("quarantine impact ceiling reached")]
    ImpactLimitReached,
    /// A live episode cannot be renewed or replaced.
    #[error("quarantine episode is already active")]
    AlreadyActive,
    /// The transaction exhausted its episode or cumulative lifetime budget.
    #[error("quarantine cumulative lifetime exhausted")]
    LifetimeExhausted,
}

impl MemoryPool {
    /// Applies bounded local quarantine settings and removes excess stale state.
    pub fn configure_quarantine(
        &mut self,
        config: QuarantineConfig,
        now_unix_ms: u64,
    ) -> Result<(), QuarantineError> {
        if config.max_entries == 0
            || config.max_entries > QUARANTINE_ABSOLUTE_IMPACT_LIMIT
            || config.max_ttl_ms == 0
            || config.selection_scan_budget == 0
            || config.selection_scan_budget > QUARANTINE_PROPOSAL_SCAN_BUDGET
        {
            return Err(QuarantineError::InvalidConfig);
        }
        self.purge_expired_quarantine(now_unix_ms);
        self.quarantine_config = QuarantineConfig {
            max_ttl_ms: config.max_ttl_ms.min(QUARANTINE_TTL_MS),
            ..config
        };
        if !config.automatic_enabled {
            self.quarantine.clear();
            self.proposal_scan_cursor = None;
            return Ok(());
        }
        if self.quarantine.len() > config.max_entries {
            let mut records = self
                .quarantine
                .iter()
                .map(|(hash, metadata)| (*hash, metadata.expires_at_unix_ms))
                .collect::<Vec<_>>();
            records.sort_unstable_by_key(|(_, expiry)| *expiry);
            for (hash, _) in records
                .into_iter()
                .take(self.quarantine.len() - config.max_entries)
            {
                self.quarantine.remove(&hash);
            }
        }
        Ok(())
    }

    /// Starts one bounded automatic quarantine episode.
    ///
    /// The requested expiry is clamped to the configured limit and the absolute
    /// 30-second ceiling. Active labels cannot be renewed or replaced.
    pub fn quarantine_transaction(
        &mut self,
        mut metadata: QuarantineMetadata,
        now_unix_ms: u64,
    ) -> Result<(), QuarantineError> {
        self.purge_expired_quarantine(now_unix_ms);
        if !self.quarantine_config.automatic_enabled {
            return Err(QuarantineError::Disabled);
        }
        let requested_ttl = metadata.expires_at_unix_ms.saturating_sub(now_unix_ms);
        let ttl = requested_ttl
            .min(self.quarantine_config.max_ttl_ms)
            .min(QUARANTINE_TTL_MS);
        let valid = metadata.created_at_unix_ms <= now_unix_ms
            && requested_ttl > 0
            && ttl > 0
            && metadata.episode == 0
            && metadata.cumulative_active_ms == 0
            && self.verified.contains_key(&metadata.transaction_hash)
            && !metadata.evidence_refs.is_empty()
            && metadata.evidence_refs.len() <= 32
            && metadata
                .evidence_refs
                .iter()
                .all(|value| !value.is_empty() && value.len() <= 128)
            && !metadata.context.is_empty()
            && metadata.context.len() <= 512
            && !metadata.rule_version.is_empty()
            && metadata.rule_version.len() <= 128;
        if !valid {
            return Err(QuarantineError::InvalidMetadata);
        }
        if self.quarantine.contains_key(&metadata.transaction_hash) {
            return Err(QuarantineError::AlreadyActive);
        }
        if self.quarantine.len() >= self.quarantine_config.max_entries {
            return Err(QuarantineError::CapacityReached);
        }
        let pool_len = self.verified.len();
        let percentage_limit = (pool_len / 100).max(1);
        let impact_limit = percentage_limit
            .min(QUARANTINE_ABSOLUTE_IMPACT_LIMIT)
            .min(self.quarantine_config.max_entries)
            .min(pool_len.saturating_sub(1));
        if self.quarantine.len() >= impact_limit {
            return Err(QuarantineError::ImpactLimitReached);
        }
        let history = self
            .quarantine_history
            .entry(metadata.transaction_hash)
            .or_default();
        if history.episodes >= QUARANTINE_MAX_EPISODES
            || history.cumulative_ms.saturating_add(ttl) > QUARANTINE_MAX_CUMULATIVE_MS
        {
            return Err(QuarantineError::LifetimeExhausted);
        }
        history.episodes = history.episodes.saturating_add(1);
        history.cumulative_ms = history.cumulative_ms.saturating_add(ttl);
        metadata.created_at_unix_ms = now_unix_ms;
        metadata.expires_at_unix_ms = now_unix_ms.saturating_add(ttl);
        metadata.episode = history.episodes;
        metadata.cumulative_active_ms = history.cumulative_ms;
        self.quarantine.insert(metadata.transaction_hash, metadata);
        Ok(())
    }

    /// Removes a node-local quarantine label without touching transaction state.
    pub fn release_quarantine(&mut self, hash: &UInt256) -> bool {
        self.quarantine.remove(hash).is_some()
    }

    /// Returns live metadata, automatically removing an expired record.
    pub fn quarantine_metadata(
        &mut self,
        hash: &UInt256,
        now_unix_ms: u64,
    ) -> Option<&QuarantineMetadata> {
        self.purge_expired_quarantine(now_unix_ms);
        self.quarantine.get(hash)
    }

    /// Returns whether a transaction has a live local quarantine record.
    /// Expired metadata is removed as part of the lookup.
    pub fn is_quarantined(&mut self, hash: &UInt256, now_unix_ms: u64) -> bool {
        self.quarantine_metadata(hash, now_unix_ms).is_some()
    }

    /// Returns the number of live quarantine records.
    pub fn quarantine_count(&mut self, now_unix_ms: u64) -> usize {
        self.purge_expired_quarantine(now_unix_ms);
        self.quarantine.len()
    }

    /// Drains at most `limit` bounded reviews scheduled by automatic expiry.
    pub fn take_quarantine_expiry_reviews(
        &mut self,
        now_unix_ms: u64,
        limit: usize,
    ) -> Vec<QuarantineExpiryReview> {
        self.purge_expired_quarantine(now_unix_ms);
        let count = limit.min(self.quarantine_expiry_reviews.len());
        self.quarantine_expiry_reviews.drain(..count).collect()
    }

    /// Selects transactions only for this node's local-primary proposal path.
    ///
    /// The bounded rotating scan supplies replacements for filtered entries and
    /// avoids permanently favoring one prefix when the scan budget is smaller
    /// than the pool. External proposals and replay use the ordinary pool views.
    pub fn local_proposal_transactions(
        &mut self,
        limit: usize,
        now_unix_ms: u64,
    ) -> Vec<Arc<Transaction>> {
        #[cfg(test)]
        {
            self.proposal_last_scan_count = 0;
        }
        self.purge_expired_quarantine(now_unix_ms);
        if limit == 0 || self.verified.is_empty() {
            if self.verified.is_empty() {
                self.proposal_scan_cursor = None;
            }
            return Vec::new();
        }
        if !self.quarantine_config.automatic_enabled {
            self.proposal_scan_cursor = None;
            return self.get_sorted_verified_transactions(limit);
        }
        let pool_len = self.verified.sorted_len();
        let scan_budget = self.quarantine_config.selection_scan_budget.min(pool_len);
        let cursor = self.proposal_scan_cursor.clone();
        let mut selected = Vec::with_capacity(limit.min(scan_budget));
        let mut inspected = 0usize;
        let mut last_inspected = None;
        let quarantine = &self.quarantine;
        let inspect = |item: &PoolItem,
                       selected: &mut Vec<Arc<Transaction>>,
                       last_inspected: &mut Option<PoolItem>| {
            *last_inspected = Some(item.clone());
            if !quarantine.contains_key(&item.transaction.hash()) {
                selected.push(Arc::clone(&item.transaction));
            }
            selected.len() == limit
        };

        // The stored key gives each BTreeSet range an O(log n) start. A wrap uses
        // at most the remaining budget, so both ranges inspect no more than the
        // configured bound in total and never walk an uncounted prefix.
        if let Some(cursor) = cursor.as_ref() {
            for item in self
                .verified
                .by_priority_descending_before(cursor)
                .take(scan_budget)
            {
                inspected += 1;
                if inspect(item, &mut selected, &mut last_inspected) {
                    break;
                }
            }
            if selected.len() < limit && inspected < scan_budget {
                for item in self
                    .verified
                    .by_priority_descending_from(cursor)
                    .take(scan_budget - inspected)
                {
                    inspected += 1;
                    if inspect(item, &mut selected, &mut last_inspected) {
                        break;
                    }
                }
            }
        } else {
            for item in self.verified.by_priority_descending().take(scan_budget) {
                inspected += 1;
                if inspect(item, &mut selected, &mut last_inspected) {
                    break;
                }
            }
        }
        debug_assert!(inspected <= scan_budget);
        if let Some(last) = last_inspected {
            self.proposal_scan_cursor = Some(last);
        }
        #[cfg(test)]
        {
            self.proposal_last_scan_count = inspected;
        }
        selected
    }

    fn purge_expired_quarantine(&mut self, now_unix_ms: u64) {
        let expired = self
            .quarantine
            .iter()
            .filter(|(_, metadata)| metadata.expires_at_unix_ms <= now_unix_ms)
            .map(|(hash, metadata)| (*hash, metadata.clone()))
            .collect::<Vec<_>>();
        for (hash, metadata) in expired {
            self.quarantine.remove(&hash);
            let review_limit = self
                .quarantine_config
                .max_entries
                .min(QUARANTINE_ABSOLUTE_IMPACT_LIMIT);
            while self.quarantine_expiry_reviews.len() >= review_limit {
                self.quarantine_expiry_reviews.pop_front();
            }
            self.quarantine_expiry_reviews
                .push_back(QuarantineExpiryReview {
                    transaction_hash: hash,
                    evidence_refs: metadata.evidence_refs,
                    context: metadata.context,
                    rule_version: metadata.rule_version,
                    expired_at_unix_ms: metadata.expires_at_unix_ms,
                    episode: metadata.episode,
                    cumulative_active_ms: metadata.cumulative_active_ms,
                });
        }
    }
}
