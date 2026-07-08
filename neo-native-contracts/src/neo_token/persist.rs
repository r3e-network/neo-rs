//! NeoToken block-persist hooks.
//!
//! Keeps committee refresh, committee reward, and voter-reward accrual logic
//! out of the contract root while preserving the C# hook ordering.

use super::{
    COMMITTEE_REWARD_RATIO, NEO_COMMITTEE_CHANGED_EVENT, NeoToken, VOTE_FACTOR, VOTER_REWARD_RATIO,
    candidate_signature_account,
};
use neo_config::Hardfork;
use neo_crypto::ECPoint;
use neo_error::{CoreError, CoreResult};
use neo_execution::ApplicationEngine;
use neo_runtime::sync_metrics::{self, NeoTokenOnPersistStage};
use neo_storage::StorageItem;
use num_bigint::BigInt;
use std::time::Instant;

impl NeoToken {
    /// C# `NeoToken.OnPersistAsync`: on a committee-refresh block
    /// (`index % CommitteeMembersCount == 0`) recompute the cached committee via
    /// `ComputeCommitteeMembers` and, from HF_Cockatrice, emit a
    /// `CommitteeChanged` notification when the member set changed.
    pub(super) fn on_persist_native(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        let total_start = Instant::now();
        let block_index =
            crate::support::engine::require_persisting_block(engine, "NeoToken::on_persist")?
                .index();
        let committee_count = engine.protocol_settings().committee_members_count();
        if committee_count == 0 {
            return Err(CoreError::invalid_operation(
                "NeoToken::on_persist requires a non-empty standby committee",
            ));
        }
        if !Self::should_refresh_committee(block_index, committee_count) {
            sync_metrics::record_neo_token_onpersist_stage(
                NeoTokenOnPersistStage::Skip,
                neo_runtime::time::elapsed_us(total_start.elapsed()),
            );
            return Ok(());
        }
        let refresh_start = Instant::now();
        let settings = engine.protocol_settings().clone();
        let snapshot = engine.snapshot_cache();

        // C# `GetAndChange(Prefix_Committee)!` - a missing cache faults.
        let stage_start = Instant::now();
        let prev_committee = self.read_committee_with_votes(&snapshot)?;
        sync_metrics::record_neo_token_onpersist_stage(
            NeoTokenOnPersistStage::ReadCachedCommittee,
            neo_runtime::time::elapsed_us(stage_start.elapsed()),
        );

        let stage_start = Instant::now();
        let new_committee = self.compute_committee_members(&snapshot, &settings)?;
        sync_metrics::record_neo_token_onpersist_stage(
            NeoTokenOnPersistStage::ComputeCommittee,
            neo_runtime::time::elapsed_us(stage_start.elapsed()),
        );

        let stage_start = Instant::now();
        snapshot.update(
            Self::committee_key(),
            StorageItem::from_bytes(Self::encode_committee(&new_committee)?),
        );
        sync_metrics::record_neo_token_onpersist_stage(
            NeoTokenOnPersistStage::WriteCommittee,
            neo_runtime::time::elapsed_us(stage_start.elapsed()),
        );

        // Hardfork check for https://github.com/neo-project/neo/pull/3158.
        let mut committee_changed = false;
        let stage_start = Instant::now();
        if engine.is_hardfork_enabled(Hardfork::HfCockatrice) {
            let prev_keys: Vec<&ECPoint> = prev_committee.iter().map(|(point, _)| point).collect();
            let new_keys: Vec<&ECPoint> = new_committee.iter().map(|(point, _)| point).collect();
            committee_changed = prev_keys != new_keys;
            if committee_changed {
                sync_metrics::record_neo_token_onpersist_stage(
                    NeoTokenOnPersistStage::CompareCommittee,
                    neo_runtime::time::elapsed_us(stage_start.elapsed()),
                );
                let stage_start = Instant::now();
                let prev_key_item = Self::points_to_stack_item(prev_keys.iter().copied())?;
                let new_key_item = Self::points_to_stack_item(new_keys.iter().copied())?;
                engine
                    .send_notification(
                        Self::script_hash(),
                        NEO_COMMITTEE_CHANGED_EVENT.to_owned(),
                        vec![prev_key_item, new_key_item],
                    )
                    .map_err(|e| {
                        CoreError::invalid_operation(format!(
                            "NeoToken::on_persist: {NEO_COMMITTEE_CHANGED_EVENT} notify: {e}"
                        ))
                    })?;
                sync_metrics::record_neo_token_onpersist_stage(
                    NeoTokenOnPersistStage::NotifyCommitteeChanged,
                    neo_runtime::time::elapsed_us(stage_start.elapsed()),
                );
            }
        }
        if !committee_changed {
            sync_metrics::record_neo_token_onpersist_stage(
                NeoTokenOnPersistStage::CompareCommittee,
                neo_runtime::time::elapsed_us(stage_start.elapsed()),
            );
        }
        sync_metrics::record_neo_token_onpersist_stage(
            NeoTokenOnPersistStage::RefreshTotal,
            neo_runtime::time::elapsed_us(refresh_start.elapsed()),
        );
        Ok(())
    }

    /// C# `NeoToken.PostPersistAsync`: every block mints
    /// `gasPerBlock * CommitteeRewardRatio / 100` GAS to the signature address of
    /// the committee member at `index % CommitteeMembersCount`; on refresh blocks
    /// it additionally accrues `Prefix_VoterRewardPerCommittee` for each
    /// committee member with votes -
    /// `voterRewardOfEachCommittee = gasPerBlock * VoterRewardRatio * VoteFactor
    /// * m / (m + n) / 100`, credited as `factor * that / votes` with factor 2
    /// for validators (`i < n`) and 1 otherwise.
    pub(super) fn post_persist_native(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        let block_index =
            crate::support::engine::require_persisting_block(engine, "NeoToken::post_persist")?
                .index();
        let committee_count = engine.protocol_settings().committee_members_count();
        let validators_count =
            usize::try_from(engine.protocol_settings().validators_count).unwrap_or(0);
        if committee_count == 0 {
            return Err(CoreError::invalid_operation(
                "NeoToken::post_persist requires a non-empty standby committee",
            ));
        }
        let snapshot = engine.snapshot_cache();
        // C# `GetGasPerBlock(snapshot)` reads the record effective at
        // `Ledger.CurrentIndex + 1`; during persistence the Ledger contract has
        // already advanced the current index to the persisting block, so this is
        // the record effective at `persistingIndex + 1` (a record written by a
        // setGasPerBlock in this very block already applies).
        let gas_per_block = self.gas_per_block_at(&snapshot, block_index.saturating_add(1));
        let member_index = (block_index % (committee_count as u32)) as usize;
        let is_refresh_block = Self::should_refresh_committee(block_index, committee_count);
        let committee = if is_refresh_block {
            Some(self.read_committee_with_votes(&snapshot)?)
        } else {
            None
        };
        let member = match committee.as_ref() {
            Some(committee) => committee
                .get(member_index)
                .map(|(member, _)| member.clone())
                .ok_or_else(|| {
                    CoreError::invalid_operation(
                        "NeoToken::post_persist: committee cache too small",
                    )
                })?,
            None => self.read_committee_member_at(&snapshot, member_index)?.0,
        };
        let account = candidate_signature_account(&member);
        let committee_reward = &gas_per_block * COMMITTEE_REWARD_RATIO / 100;
        crate::GasToken::new().gas_mint(engine, &account, &committee_reward, false)?;

        // Record the cumulative reward of the voters of the committee.
        if let Some(committee) = committee {
            let m = BigInt::from(committee_count as u64);
            let m_plus_n = BigInt::from((committee_count + validators_count) as u64);
            // Zoomed in by VoteFactor; consumers divide it back out.
            let voter_reward_of_each_committee =
                &gas_per_block * VOTER_REWARD_RATIO * VOTE_FACTOR * m / m_plus_n / 100;
            let snapshot = engine.snapshot_cache();
            // C# v3.10.1: from HF_Gorgon onward, refresh-time voter rewards
            // use `GetCandidateVote(snapshot, pubkey)` rather than the vote
            // count stored in the committee cache.
            for (index, (member, cached_votes)) in committee.iter().enumerate() {
                let votes = if engine.is_hardfork_enabled(Hardfork::HfGorgon) {
                    self.candidate_vote(&snapshot, member)?
                } else {
                    cached_votes.clone()
                };
                // Validator voters earn double.
                let factor = if index < validators_count { 2 } else { 1 };
                if votes > BigInt::from(0) {
                    let reward_per_neo = factor * &voter_reward_of_each_committee / &votes;
                    let key = Self::voter_reward_per_committee_key(member);
                    // C# `GetAndChange(key, () => new StorageItem(0)).Add(...)`.
                    let accumulated =
                        self.voter_reward_per_committee(&snapshot, member) + reward_per_neo;
                    snapshot.update(
                        key,
                        StorageItem::from_bytes(crate::bigint_to_storage_bytes(&accumulated)),
                    );
                }
            }
        }
        Ok(())
    }
}
