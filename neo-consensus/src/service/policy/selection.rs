//! Deterministic proposal-selection policy.
//!
//! The rules that decide which candidate transactions may enter a dBFT
//! `PrepareRequest` are pure consensus decisions: they depend only on
//! transaction summaries (hash, encoded size, system fee), the configured
//! block limits and the set of transactions already rejected by more than
//! `F` validators. Node infrastructure is reached exclusively through two
//! ports: [`ProposalCandidateSource`] for candidate ordering and
//! [`TransactionSummarySource`] for the transaction facts themselves (mempool
//! membership, TEE decryption state and durable storage stay with the node).

use neo_primitives::UInt256;
use std::collections::HashSet;

/// Infrastructure-free description of a candidate transaction.
///
/// The node adapter derives summaries from its concrete `Transaction` type so
/// this crate never depends on the protocol payload types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionSummary {
    /// Transaction hash used for ordering and deduplication.
    pub hash: UInt256,
    /// Encoded transaction size in bytes.
    pub size: usize,
    /// System fee charged by the transaction (may be negative for native fees).
    pub system_fee: i64,
}

impl TransactionSummary {
    /// Creates a new summary.
    #[must_use]
    pub fn new(hash: UInt256, size: usize, system_fee: i64) -> Self {
        Self {
            hash,
            size,
            system_fee,
        }
    }
}

/// Consensus-side limits applied to a single proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProposalLimits {
    /// Maximum number of transactions inside one block.
    pub max_transactions: usize,
    /// Maximum encoded block size in bytes (header + transactions).
    pub max_block_size: usize,
    /// Maximum aggregate system fee of the transactions in one block.
    pub max_block_system_fee: i64,
}

impl Default for ProposalLimits {
    fn default() -> Self {
        Self {
            max_transactions: crate::context::DEFAULT_MAX_TRANSACTIONS_PER_BLOCK,
            max_block_size: usize::MAX,
            max_block_system_fee: i64::MAX,
        }
    }
}

impl ProposalLimits {
    /// Creates explicit limits.
    #[must_use]
    pub fn new(max_transactions: usize, max_block_size: usize, max_block_system_fee: i64) -> Self {
        Self {
            max_transactions,
            max_block_size,
            max_block_system_fee,
        }
    }
}

/// Reason the selection loop stopped before exhausting the candidate stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposalStop {
    /// The next transaction would push the block over `max_block_size`.
    BlockSize,
    /// The next transaction would push the aggregate fee over the cap.
    SystemFee,
}

/// Outcome of [`select_proposal_transactions`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProposalSelection {
    /// Selected summaries, in candidate order.
    pub selected: Vec<TransactionSummary>,
    /// Sum of the encoded transaction sizes.
    pub total_size: usize,
    /// Aggregate system fee of the selected transactions.
    pub total_system_fee: i64,
    /// Candidates skipped because more than `F` validators rejected them.
    pub skipped_rejected: usize,
    /// Set when a limit (rather than the candidate stream) stopped selection.
    pub stop: Option<ProposalStop>,
}

impl ProposalSelection {
    /// Selected transaction hashes in proposal order.
    #[must_use]
    pub fn hashes(&self) -> Vec<UInt256> {
        self.selected.iter().map(|summary| summary.hash).collect()
    }

    /// Number of selected transactions.
    #[must_use]
    pub fn len(&self) -> usize {
        self.selected.len()
    }

    /// Returns `true` when no transaction could be selected.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.selected.is_empty()
    }
}

/// Port for transaction facts.
///
/// The policy never depends on the protocol `Transaction` type; it only needs
/// the hash, the encoded size and the system fee of a candidate. The node
/// adapter resolves those from its own transaction representation.
pub trait TransactionSummarySource {
    /// Returns the summary for `hash`, or `None` when the transaction is no
    /// longer available.
    fn transaction_summary(&self, hash: &UInt256) -> Option<TransactionSummary>;
}

/// Port for the ordered candidate stream owned by the node.
///
/// Implementations are expected to return candidates sorted by the node's
/// fee/priority policy; the returned hashes never exceed `limit` items. The
/// summaries are then resolved through [`TransactionSummarySource`], which
/// keeps ordering and transaction facts as separate responsibilities.
pub trait ProposalCandidateSource: TransactionSummarySource {
    /// Returns at most `limit` candidate hashes in node-defined priority
    /// order.
    fn proposal_candidate_hashes(&mut self, limit: usize) -> Vec<UInt256>;
}

/// Encoded block size for `transaction_count` transactions of `transaction_bytes`.
///
/// `block_overhead_size` carries the header and witness estimate produced by
/// the node (it depends on the validator set and the multi-sig contract).
#[must_use]
pub fn block_size_with_transactions(
    block_overhead_size: usize,
    transaction_count: usize,
    transaction_bytes: usize,
) -> usize {
    block_overhead_size
        + neo_io::serializable::helper::get_var_size_usize(transaction_count)
        + transaction_bytes
}

/// Selects the transactions that may enter a `PrepareRequest`.
///
/// Selection walks the candidates in order and stops at the first candidate
/// that would violate the block-size or aggregate-fee limit, matching the
/// reference implementation's "no reordering to fit" behaviour. Candidates
/// already rejected by more than `F` validators are skipped without stopping
/// the scan.
#[must_use]
pub fn select_proposal_transactions(
    candidates: impl IntoIterator<Item = TransactionSummary>,
    limits: &ProposalLimits,
    block_overhead_size: usize,
    rejected_above_f: &HashSet<UInt256>,
) -> ProposalSelection {
    let mut selection = ProposalSelection::default();

    for candidate in candidates {
        if selection.selected.len() >= limits.max_transactions {
            break;
        }
        if rejected_above_f.contains(&candidate.hash) {
            selection.skipped_rejected += 1;
            continue;
        }

        let size_with_candidate = block_size_with_transactions(
            block_overhead_size,
            selection.selected.len() + 1,
            selection.total_size + candidate.size,
        );
        if size_with_candidate > limits.max_block_size {
            selection.stop = Some(ProposalStop::BlockSize);
            break;
        }

        let fee_with_candidate = selection
            .total_system_fee
            .saturating_add(candidate.system_fee);
        if fee_with_candidate > limits.max_block_system_fee {
            selection.stop = Some(ProposalStop::SystemFee);
            break;
        }

        selection.total_size += candidate.size;
        selection.total_system_fee = fee_with_candidate;
        selection.selected.push(candidate);
    }

    selection
}

/// Selects the proposal subset by pulling ordered candidate hashes from
/// `source` and resolving each hash through its summary port.
///
/// Candidates whose summary disappeared between ordering and resolution are
/// skipped: a transaction that is no longer available cannot be proposed.
/// The source is asked for at most `min(limit, max_transactions)` hashes, so a
/// node can never be asked for more candidates than the block can hold.
#[must_use]
pub fn select_proposal_transactions_from_source(
    source: &mut dyn ProposalCandidateSource,
    limits: &ProposalLimits,
    block_overhead_size: usize,
    limit: usize,
    rejected_above_f: &HashSet<UInt256>,
) -> ProposalSelection {
    let hashes = source.proposal_candidate_hashes(limit.min(limits.max_transactions));
    let candidates = hashes
        .iter()
        .filter_map(|hash| source.transaction_summary(hash));
    select_proposal_transactions(candidates, limits, block_overhead_size, rejected_above_f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn summary(seed: u8, size: usize, system_fee: i64) -> TransactionSummary {
        TransactionSummary::new(UInt256::from([seed; 32]), size, system_fee)
    }

    #[test]
    fn skips_rejected_transactions_without_stopping_scan() {
        let candidates = vec![summary(1, 100, 1), summary(2, 100, 1), summary(3, 100, 1)];
        let rejected: HashSet<UInt256> = [UInt256::from([2u8; 32])].into_iter().collect();
        let limits = ProposalLimits::new(10, 10_000, 1_000);

        let selection = select_proposal_transactions(candidates, &limits, 0, &rejected);

        assert_eq!(selection.len(), 2);
        assert_eq!(selection.skipped_rejected, 1);
        assert_eq!(
            selection.hashes(),
            vec![UInt256::from([1u8; 32]), UInt256::from([3u8; 32])]
        );
        assert_eq!(selection.stop, None);
    }

    #[test]
    fn stops_at_block_size_limit_including_count_prefix() {
        // Each transaction is 100 bytes; the length prefix of the second
        // transaction makes the block 201 bytes, which exceeds a 200-byte cap.
        let candidates = vec![summary(1, 100, 1), summary(2, 100, 1)];
        let limits = ProposalLimits::new(10, 200, i64::MAX);

        let selection = select_proposal_transactions(candidates, &limits, 0, &HashSet::new());

        assert_eq!(selection.len(), 1);
        assert_eq!(selection.total_size, 100);
        assert_eq!(selection.stop, Some(ProposalStop::BlockSize));
    }

    #[test]
    fn stops_at_system_fee_cap() {
        let candidates = vec![summary(1, 10, 40), summary(2, 10, 40), summary(3, 10, 40)];
        let limits = ProposalLimits::new(10, usize::MAX, 100);

        let selection = select_proposal_transactions(candidates, &limits, 0, &HashSet::new());

        assert_eq!(selection.len(), 2);
        assert_eq!(selection.total_system_fee, 80);
        assert_eq!(selection.stop, Some(ProposalStop::SystemFee));
    }

    #[test]
    fn caps_transaction_count() {
        let candidates = vec![summary(1, 10, 0), summary(2, 10, 0)];
        let limits = ProposalLimits::new(1, usize::MAX, i64::MAX);

        let selection = select_proposal_transactions(candidates, &limits, 0, &HashSet::new());

        assert_eq!(selection.len(), 1);
        assert_eq!(selection.stop, None);
    }

    /// Candidate ordering and transaction summaries are separate ports: the
    /// order comes from the candidate source, the facts from the summary one.
    struct MapSource {
        order: Vec<UInt256>,
        summaries: HashMap<UInt256, TransactionSummary>,
    }

    impl TransactionSummarySource for MapSource {
        fn transaction_summary(&self, hash: &UInt256) -> Option<TransactionSummary> {
            self.summaries.get(hash).cloned()
        }
    }

    impl ProposalCandidateSource for MapSource {
        fn proposal_candidate_hashes(&mut self, limit: usize) -> Vec<UInt256> {
            self.order.iter().take(limit).copied().collect()
        }
    }

    #[test]
    fn source_selection_resolves_summaries_through_the_port() {
        let present = summary(1, 100, 5);
        let unavailable = summary(2, 100, 5);
        let mut source = MapSource {
            order: vec![present.hash, unavailable.hash],
            summaries: HashMap::from([(present.hash, present.clone())]),
        };
        let limits = ProposalLimits::new(10, 10_000, 1_000);

        let selection =
            select_proposal_transactions_from_source(&mut source, &limits, 0, 10, &HashSet::new());

        assert_eq!(selection.selected, vec![present]);
        assert_eq!(selection.total_system_fee, 5);
    }

    #[test]
    fn source_selection_never_asks_for_more_than_the_transaction_cap() {
        struct CountingSource {
            requested: usize,
            summaries: HashMap<UInt256, TransactionSummary>,
        }

        impl TransactionSummarySource for CountingSource {
            fn transaction_summary(&self, hash: &UInt256) -> Option<TransactionSummary> {
                self.summaries.get(hash).cloned()
            }
        }

        impl ProposalCandidateSource for CountingSource {
            fn proposal_candidate_hashes(&mut self, limit: usize) -> Vec<UInt256> {
                self.requested = limit;
                self.summaries.keys().take(limit).copied().collect()
            }
        }

        let mut source = CountingSource {
            requested: 0,
            summaries: (0u8..8)
                .map(|seed| {
                    let item = summary(seed, 10, 0);
                    (item.hash, item)
                })
                .collect(),
        };
        let limits = ProposalLimits::new(3, usize::MAX, i64::MAX);

        let selection =
            select_proposal_transactions_from_source(&mut source, &limits, 0, 99, &HashSet::new());

        assert_eq!(source.requested, 3, "the cap bounds the candidate request");
        assert_eq!(selection.len(), 3);
    }
}
