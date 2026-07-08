//! Empty-block fast-forward reward accounting for NEO.
//!
//! The blockchain fast-forward path skips per-block replay artifacts, but still
//! needs byte-equivalent storage effects for committee refreshes, voter reward
//! accumulators, and per-block committee GAS rewards.

use super::{
    COMMITTEE_REWARD_RATIO, NeoToken, VOTE_FACTOR, VOTER_REWARD_RATIO, candidate_signature_account,
};
use neo_config::Hardfork;
use neo_crypto::ECPoint;
use neo_error::{CoreError, CoreResult};
use neo_primitives::UInt160;
use neo_storage::StorageItem;
use neo_storage::persistence::DataCache;
use num_bigint::BigInt;

impl NeoToken {
    /// Applies the state-only NEO empty-block persist effects for a fast-forward
    /// run.
    ///
    /// The blockchain fast-forward gate disables replay artifacts/events before
    /// calling this helper. This method therefore writes only consensus-visible
    /// storage: committee-cache refreshes, voter reward accumulators, and the
    /// per-block committee GAS reward minted to
    /// `cached_committee[height % committee_count]`. Integer division is kept at
    /// the single-block boundary before multiplying repeated rewards, matching
    /// normal `on_persist`/`post_persist`.
    pub fn fast_forward_empty_block_rewards(
        &self,
        snapshot: &DataCache,
        settings: &neo_config::ProtocolSettings,
        start: u32,
        end: u32,
    ) -> CoreResult<()> {
        let committee_count = settings.committee_members_count();
        if committee_count == 0 {
            return Err(CoreError::invalid_operation(
                "NeoToken::fast_forward_empty_block_rewards requires a non-empty standby committee",
            ));
        }
        if start > end {
            return Ok(());
        }
        let mut committee = self.read_committee_with_votes(snapshot)?;
        let mut committee_accounts = committee_signature_accounts(&committee);
        let refresh_heights = (start..=end)
            .filter(|height| Self::should_refresh_committee(*height, committee_count))
            .collect::<Vec<_>>();
        let mut refresh_index = 0usize;
        let mut gas_change_points = self
            .sorted_gas_records(snapshot, end.saturating_add(1))
            .into_iter()
            .map(|(index, _)| index)
            .filter(|index| *index > start.saturating_add(1))
            .collect::<Vec<_>>();
        gas_change_points.reverse();
        let mut gas_change_index =
            gas_change_points.partition_point(|index| *index <= start.saturating_add(1));

        let mut rewards = std::collections::BTreeMap::<UInt160, BigInt>::new();
        let mut height = start;
        while height <= end {
            if refresh_heights
                .get(refresh_index)
                .is_some_and(|refresh| *refresh == height)
            {
                let refreshed = self.compute_committee_members(snapshot, settings)?;
                snapshot.update(
                    Self::committee_key(),
                    StorageItem::from_bytes(Self::encode_committee(&refreshed)?),
                );
                committee = refreshed;
                committee_accounts = committee_signature_accounts(&committee);
                self.fast_forward_voter_reward_refreshes(
                    snapshot,
                    settings,
                    height,
                    usize::try_from(settings.validators_count).unwrap_or(0),
                    &committee,
                    &self.fast_forward_refresh_reward(snapshot, settings, committee_count, height),
                )?;
                refresh_index += 1;
            }
            while gas_change_index < gas_change_points.len()
                && gas_change_points[gas_change_index] <= height.saturating_add(1)
            {
                gas_change_index += 1;
            }
            let gas_per_block = self.gas_per_block_at(snapshot, height.saturating_add(1));
            let next_change = gas_change_points.get(gas_change_index).copied();
            let mut segment_end = next_change
                .map(|index| index.saturating_sub(2))
                .map_or(end, |candidate| candidate.min(end));
            if let Some(refresh) = refresh_heights
                .get(refresh_index)
                .copied()
                .filter(|refresh| *refresh > height)
            {
                segment_end = segment_end.min(refresh - 1);
            }
            let committee_reward = &gas_per_block * COMMITTEE_REWARD_RATIO / 100;
            if committee_reward != BigInt::from(0) {
                for (member_index, account) in committee_accounts.iter().enumerate() {
                    let count = count_heights_with_residue(
                        height,
                        segment_end,
                        committee_count as u32,
                        member_index as u32,
                    );
                    if count == 0 {
                        continue;
                    }
                    *rewards.entry(*account).or_insert_with(|| BigInt::from(0)) +=
                        &committee_reward * count;
                }
            }
            height = segment_end.saturating_add(1);
        }

        let gas = crate::GasToken::new();
        for (account, amount) in rewards {
            gas.fast_forward_mint_state(snapshot, &account, &amount)?;
        }
        Ok(())
    }

    fn fast_forward_voter_reward_refreshes(
        &self,
        snapshot: &DataCache,
        settings: &neo_config::ProtocolSettings,
        height: u32,
        validators_count: usize,
        committee: &[(ECPoint, BigInt)],
        refresh_reward: &BigInt,
    ) -> CoreResult<()> {
        // C# v3.10.1 `NeoToken.PostPersistAsync`: from HF_Gorgon onward,
        // voter-reward refreshes use `GetCandidateVote(snapshot, pubkey)`
        // instead of the votes cached in `Prefix_Committee`.
        let gorgon_enabled = settings.is_hardfork_enabled(Hardfork::HfGorgon, height);
        for (index, (member, cached_votes)) in committee.iter().enumerate() {
            let votes = if gorgon_enabled {
                self.candidate_vote(snapshot, member)?
            } else {
                cached_votes.clone()
            };
            if votes > BigInt::from(0) {
                let factor = if index < validators_count { 2 } else { 1 };
                let accumulated_delta = factor * refresh_reward / &votes;
                let key = Self::voter_reward_per_committee_key(member);
                let accumulated =
                    self.voter_reward_per_committee(snapshot, member) + accumulated_delta;
                snapshot.update(
                    key,
                    StorageItem::from_bytes(crate::bigint_to_storage_bytes(&accumulated)),
                );
            }
        }
        Ok(())
    }

    fn fast_forward_refresh_reward(
        &self,
        snapshot: &DataCache,
        settings: &neo_config::ProtocolSettings,
        committee_count: usize,
        refresh_height: u32,
    ) -> BigInt {
        let validators_count = usize::try_from(settings.validators_count).unwrap_or(0);
        let committee = BigInt::from(committee_count as u64);
        let committee_plus_validators = BigInt::from((committee_count + validators_count) as u64);
        let gas_per_block = self.gas_per_block_at(snapshot, refresh_height.saturating_add(1));
        &gas_per_block * VOTER_REWARD_RATIO * VOTE_FACTOR * &committee
            / &committee_plus_validators
            / 100
    }
}

fn committee_signature_accounts(committee: &[(ECPoint, BigInt)]) -> Vec<UInt160> {
    committee
        .iter()
        .map(|(member, _)| candidate_signature_account(member))
        .collect()
}

fn count_heights_with_residue(start: u32, end: u32, modulus: u32, residue: u32) -> u64 {
    if start > end || modulus == 0 {
        return 0;
    }
    let offset = (residue + modulus - (start % modulus)) % modulus;
    let first = start.saturating_add(offset);
    if first > end {
        return 0;
    }
    u64::from((end - first) / modulus + 1)
}
