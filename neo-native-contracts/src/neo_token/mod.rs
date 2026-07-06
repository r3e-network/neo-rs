//! # neo-native-contracts::neo_token
//!
//! Native NEO token governance, voting, and committee behavior.
//!
//! ## Boundary
//!
//! This module belongs to `neo-native-contracts`. This execution-domain crate
//! owns native contract logic and storage codecs and must not own node startup,
//! RPC transport, or P2P sync.
//!
//! ## Contents
//!
//! - `fast_forward`: state-equivalent empty-block reward batching.
//! - `invoke`: native NEO invocation helpers.
//! - `metadata`: Native contract metadata and descriptor helpers.
//! - `storage`: Storage contexts, key builders, and storage item helpers for
//!   execution.
//! - `transfers`: wallet transfer RPC handlers.
//! - `tests`: Module-local tests and regression coverage.

use neo_config::{Hardfork, ProtocolSettings};
use neo_crypto::ECPoint;
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, Contract, NativeContract, NativeEvent, NativeMethod};
use neo_primitives::UInt160;
use neo_runtime::sync_metrics::{self, NeoTokenOnPersistStage};
use neo_storage::persistence::{DataCache, SeekDirection};
use neo_storage::{StorageItem, StorageKey};
use neo_vm::StackItem;
use neo_vm_rs::StackValue;
use num_bigint::BigInt;
use std::time::Instant;

use crate::hashes::NEO_TOKEN_HASH;

mod fast_forward;
mod invoke;
mod metadata;
mod storage;
mod transfers;

pub(crate) use storage::CachedCommittee;
#[allow(unused_imports)]
use storage::CandidateState;
use storage::NeoAccountStateView;
pub(crate) use storage::candidate_signature_account;

/// C# `NeoToken.Prefix_RegisterPrice`.
const PREFIX_REGISTER_PRICE: u8 = 13;
/// C# default candidate register price: 1000 GAS, in datoshi (1000 * 1e8).
const DEFAULT_REGISTER_PRICE: i64 = 1000 * 100_000_000;
/// C# `NeoToken.Prefix_GasPerBlock`.
const PREFIX_GAS_PER_BLOCK: u8 = 29;
/// C# default GAS-per-block at index 0: 5 GAS, in datoshi (5 * 1e8).
const DEFAULT_GAS_PER_BLOCK: i64 = 5 * 100_000_000;
/// C# `NeoToken.Prefix_Committee` — the cached `(pubkey, votes)` committee list.
const PREFIX_COMMITTEE: u8 = 14;
/// C# `NeoToken.Prefix_Candidate` — per-candidate `(Registered, Votes)` state.
const PREFIX_CANDIDATE: u8 = 33;
/// C# `NeoToken.Prefix_VoterRewardPerCommittee` — accumulated GAS-per-vote.
const PREFIX_VOTER_REWARD_PER_COMMITTEE: u8 = 23;
/// C# `NeoToken.Prefix_VotersCount` — total NEO that has voted (a BigInteger).
const PREFIX_VOTERS_COUNT: u8 = 1;
/// C# `NeoToken.NeoHolderRewardRatio` (10%).
const NEO_HOLDER_REWARD_RATIO: i64 = 10;
/// C# `NeoToken.CommitteeRewardRatio` (10%): the per-block GAS share minted to
/// the committee member selected by `index % committeeCount`.
const COMMITTEE_REWARD_RATIO: i64 = 10;
/// C# `NeoToken.VoterRewardRatio` (80%): the GAS share accrued (on committee
/// refresh blocks) to the voters of the committee.
const VOTER_REWARD_RATIO: i64 = 80;
/// C# `NeoToken.VoteFactor` (1e8): the zoom factor for per-vote GAS rewards.
const VOTE_FACTOR: i64 = 100_000_000;
/// C# `NeoToken.TotalAmount` = 100,000,000 NEO (decimals 0, so Factor = 1).
const NEO_TOTAL_AMOUNT: i64 = 100_000_000;
pub(crate) const NEO_CANDIDATE_STATE_CHANGED_EVENT: &str = "CandidateStateChanged";
pub(crate) const NEO_VOTE_EVENT: &str = "Vote";
pub(crate) const NEO_COMMITTEE_CHANGED_EVENT: &str = "CommitteeChanged";

native_contract_handle!(
    /// The NeoToken native contract.
    pub struct NeoToken {
        id: -5,
        contract_name: "NeoToken",
        hash: NEO_TOKEN_HASH,
    }
);

impl NeoToken {
    /// NEP-17 symbol (C# `NeoToken.Symbol => "NEO"`).
    pub const SYMBOL: &'static str = "NEO";
    /// NEP-17 decimals (C# `NeoToken.Decimals => 0`).
    pub const DECIMALS: u8 = 0;

    /// C# `GetNextBlockValidators`: the first `validators_count` committee members
    /// (in stored, vote-ranked order), then sorted ascending. Public so
    /// `GasToken::on_persist` can resolve the primary validator the block's
    /// network fees are minted to (C# GasToken.cs:55) and the blockchain service
    /// can build the extensible-witness whitelist (C# `Blockchain.
    /// UpdateExtensibleWitnessWhiteList`).
    pub fn next_block_validators(
        &self,
        snapshot: &DataCache,
        validators_count: usize,
    ) -> CoreResult<Vec<ECPoint>> {
        let mut points = self.read_committee_points(snapshot)?;
        points.truncate(validators_count);
        points.sort();
        Ok(points)
    }

    /// C# `NEO.ComputeNextBlockValidators(snapshot, settings)`: recompute the next
    /// committee from live votes, take `ValidatorsCount`, then sort ascending.
    pub fn compute_next_block_validators(
        &self,
        snapshot: &DataCache,
        settings: &neo_config::ProtocolSettings,
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
    pub fn next_consensus_address_for_block(
        &self,
        snapshot: &DataCache,
        settings: &neo_config::ProtocolSettings,
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

impl NativeContract for NeoToken {
    native_contract_identity!(NeoToken);

    fn methods(&self) -> &[NativeMethod] {
        &metadata::NEO_TOKEN_METHODS
    }

    fn supports_empty_block_fast_forward(&self) -> bool {
        true
    }

    /// C# `NeoToken.OnManifestCompose` (NeoToken.cs:112-122): NEO declares
    /// NEP-27 in addition to NEP-17 once HF_Echidna is enabled at the height.
    fn supported_standards(&self, settings: &ProtocolSettings, block_height: u32) -> Vec<String> {
        if settings.is_hardfork_enabled(Hardfork::HfEchidna, block_height) {
            crate::native_supported_standards(&[crate::NEP17_STANDARD, crate::NEP27_STANDARD])
        } else {
            crate::native_supported_standards(&[crate::NEP17_STANDARD])
        }
    }

    fn event_descriptors(&self) -> &[NativeEvent] {
        &metadata::NEO_TOKEN_EVENTS
    }

    /// C# `NeoToken.InitializeAsync(engine, hardfork)` for `hardfork == ActiveIn`
    /// (NEO is genesis-active, so this runs while persisting block 0): seed the
    /// committee cache with the standby committee (zero votes each), an empty
    /// voters count, the genesis 5-GAS gas-per-block record at index 0, the
    /// 1000-GAS register price, and mint `TotalAmount` NEO to the BFT address of
    /// the standby validators.
    fn initialize(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        let standby_committee = engine.protocol_settings().standby_committee.clone();
        let standby_validators = engine.protocol_settings().standby_validators();
        let snapshot = engine.snapshot_cache();
        let members: Vec<(ECPoint, BigInt)> = standby_committee
            .into_iter()
            .map(|point| (point, BigInt::from(0)))
            .collect();
        snapshot.add(
            Self::committee_key(),
            StorageItem::from_bytes(Self::encode_committee(&members)?),
        );
        // C# `new StorageItem(Array.Empty<byte>())` — BigInteger zero is stored
        // as empty bytes.
        snapshot.add(
            Self::voters_count_key(),
            StorageItem::from_bytes(Vec::new()),
        );
        snapshot.add(
            Self::gas_per_block_key(0),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_GAS_PER_BLOCK,
            ))),
        );
        snapshot.add(
            Self::register_price_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_REGISTER_PRICE,
            ))),
        );
        let bft = Self::bft_address(&standby_validators)?;
        self.neo_mint(engine, &bft, &BigInt::from(NEO_TOTAL_AMOUNT), false)
    }

    /// C# `NeoToken.OnPersistAsync`: on a committee-refresh block
    /// (`index % CommitteeMembersCount == 0`) recompute the cached committee via
    /// `ComputeCommitteeMembers` and, from HF_Cockatrice, emit a
    /// `CommitteeChanged` notification when the member set changed.
    fn on_persist(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
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

        // C# `GetAndChange(Prefix_Committee)!` — a missing cache faults.
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
    /// committee member with votes —
    /// `voterRewardOfEachCommittee = gasPerBlock * VoterRewardRatio * VoteFactor
    /// * m / (m + n) / 100`, credited as `factor * that / votes` with factor 2
    /// for validators (`i < n`) and 1 otherwise.
    fn post_persist(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
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
            for (index, (member, votes)) in committee.iter().enumerate() {
                // Validator voters earn double.
                let factor = if index < validators_count { 2 } else { 1 };
                if *votes > BigInt::from(0) {
                    let reward_per_neo = factor * &voter_reward_of_each_committee / votes;
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

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        self.invoke_native(engine, method, args)
    }

    /// C# `NEO.GetCommitteeAddress`, exposed through the native-contract seam so
    /// the engine's `check_committee_witness` can verify committee-gated writers
    /// without depending on `neo-native-contracts`.
    fn committee_address(&self, snapshot: &DataCache) -> CoreResult<Option<UInt160>> {
        Ok(Some(self.compute_committee_address(snapshot)?))
    }
}

#[cfg(test)]
#[path = "../tests/neo_token/mod.rs"]
mod tests;
