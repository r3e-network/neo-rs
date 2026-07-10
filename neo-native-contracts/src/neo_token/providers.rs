//! NeoToken provider helpers used by consensus, GAS persistence, and node code.
//!
//! Keeps cross-component validator and next-consensus read APIs out of the
//! contract root while preserving their public surface for callers.

use super::NeoToken;
use neo_config::ProtocolSettings;
use neo_crypto::ECPoint;
use neo_error::{CoreError, CoreResult};
use neo_primitives::UInt160;
use neo_storage::persistence::DataCache;

impl NeoToken {
    /// C# `GetNextBlockValidators`: the first `validators_count` committee members
    /// (in stored, vote-ranked order), then sorted ascending. Public so
    /// `GasToken::on_persist` can resolve the primary validator the block's
    /// network fees are minted to (C# GasToken.cs:55) and the blockchain service
    /// can build the extensible-witness whitelist (C# `Blockchain.
    /// UpdateExtensibleWitnessWhiteList`).
    pub fn next_block_validators<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        validators_count: usize,
    ) -> CoreResult<Vec<ECPoint>> {
        let mut points = self.read_committee_points(snapshot)?;
        points.truncate(validators_count);
        points.sort();
        Ok(points)
    }

    /// C# `NEO.ComputeNextBlockValidators(snapshot, settings)`: recompute the next
    /// committee from live votes, take `ValidatorsCount`, then sort ascending.
    pub fn compute_next_block_validators<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> CoreResult<Vec<ECPoint>> {
        let validators_count = usize::try_from(settings.validators_count).unwrap_or(0);
        let mut points: Vec<ECPoint> = self
            .compute_committee_members(snapshot, settings)?
            .into_iter()
            .map(|(point, _)| point)
            .take(validators_count)
            .collect();
        points.sort();
        Ok(points)
    }

    /// C# DBFT `ConsensusContext.Reset(0)` header `NextConsensus` rule.
    ///
    /// At committee-refresh heights the header signs over the BFT address of
    /// `ComputeNextBlockValidators`; otherwise it signs over the cached
    /// `GetNextBlockValidators` set. The active validators for the current round are
    /// still `GetNextBlockValidators`.
    pub fn next_consensus_address_for_block<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
        block_index: u32,
    ) -> CoreResult<UInt160> {
        let committee_count = settings.committee_members_count();
        if committee_count == 0 {
            return Err(CoreError::invalid_operation(
                "NextConsensus requires a non-empty standby committee",
            ));
        }
        let validators_count = usize::try_from(settings.validators_count).unwrap_or(0);
        let validators = if Self::should_refresh_committee(block_index, committee_count) {
            self.compute_next_block_validators(snapshot, settings)?
        } else {
            self.next_block_validators(snapshot, validators_count)?
        };
        Self::bft_address(&validators)
    }
}
