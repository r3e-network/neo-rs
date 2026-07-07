//! # neo-native-contracts::neo_token::storage
//!
//! Storage contexts, key builders, and storage item helpers for execution.
//!
//! ## Boundary
//!
//! This module belongs to `neo-native-contracts`. This execution-domain crate
//! owns native contract logic and storage codecs and must not own node startup,
//! RPC transport, or P2P sync.
//!
//! ## Contents
//!
//! - `account`: NEO account-state codecs and balance reads.
//! - `candidates`: NEO candidate storage codecs.
//! - `committee`: committee cache readers, address derivation, and validator
//!   key helpers.
//! - `keys`: NEO storage key constructors.
//! - `points`: EC-point return encoders for public committee/validator arrays.
//! - `views`: native contract storage read views.

use super::*;
use neo_error::CoreError;
use num_traits::ToPrimitive;

mod account;
mod candidates;
mod committee;
mod keys;
mod points;
mod views;

pub(crate) use candidates::candidate_signature_account;
pub(crate) use views::CachedCommittee;
pub(super) use views::{CandidateState, NeoAccountStateView};

impl NeoToken {
    /// C# `GetRegisterPrice` = `(long)(BigInteger)snapshot[_registerPrice]`.
    pub(super) fn register_price(&self, snapshot: &DataCache) -> CoreResult<i64> {
        let key = Self::register_price_key();
        let Some(item) = snapshot.get(&key) else {
            return Err(CoreError::invalid_operation(
                "NeoToken RegisterPrice storage is missing",
            ));
        };
        BigInt::from_signed_bytes_le(&item.value_bytes())
            .to_i64()
            .ok_or_else(|| CoreError::invalid_operation("NeoToken RegisterPrice is out of range"))
    }

    /// C# `SetRegisterPrice` storage effect: overwrite `Prefix_RegisterPrice` as a
    /// `BigInteger` (`GetAndChange(_registerPrice).Set(registerPrice)`).
    pub(super) fn put_register_price(&self, snapshot: &DataCache, price: i64) -> CoreResult<()> {
        let key = Self::register_price_key();
        if snapshot.get(&key).is_none() {
            return Err(CoreError::invalid_operation(
                "NeoToken RegisterPrice storage is missing",
            ));
        }
        snapshot.update(
            key,
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(price))),
        );
        Ok(())
    }

    /// C# `SetGasPerBlock` storage effect: write a `Prefix_GasPerBlock` record at
    /// `index` (a big-endian `uint` key suffix), overwriting any record already at
    /// that index (`GetAndChange(key, factory).Set(gasPerBlock)`). `update` upserts
    /// (a brand-new index key is tracked as Changed), which commits to the same
    /// stored key/value as the C# Added path — only the resulting store contents
    /// feed the state root.
    pub(super) fn put_gas_per_block(
        &self,
        snapshot: &DataCache,
        index: u32,
        gas_per_block: &BigInt,
    ) {
        let key = Self::gas_per_block_key(index);
        snapshot.update(
            key,
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(gas_per_block)),
        );
    }

    /// Returns the GAS-per-block effective at `index`: the most recent
    /// `Prefix_GasPerBlock` record whose record index is ≤ `index` (C#
    /// `GetSortedGasRecords(...).First().GasPerBlock`), defaulting to 5 GAS.
    pub(super) fn gas_per_block_at(&self, snapshot: &DataCache, index: u32) -> BigInt {
        let prefix = Self::gas_per_block_prefix_key();
        for (key, item) in snapshot.find(Some(&prefix), SeekDirection::Backward) {
            let key_bytes = key.key();
            if key_bytes.len() >= 5 {
                let record_index =
                    u32::from_be_bytes([key_bytes[1], key_bytes[2], key_bytes[3], key_bytes[4]]);
                if record_index <= index {
                    return BigInt::from_signed_bytes_le(&item.value_bytes());
                }
            }
        }
        BigInt::from(DEFAULT_GAS_PER_BLOCK)
    }

    /// Reads the total voted NEO (`Prefix_VotersCount`), defaulting to zero.
    pub(super) fn read_voters_count(&self, snapshot: &DataCache) -> BigInt {
        snapshot
            .get(&Self::voters_count_key())
            .map(|item| BigInt::from_signed_bytes_le(&item.value_bytes()))
            .unwrap_or_else(|| BigInt::from(0))
    }

    /// Writes the total voted NEO (`Prefix_VotersCount`).
    pub(super) fn write_voters_count(&self, snapshot: &DataCache, value: &BigInt) {
        snapshot.update(
            Self::voters_count_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(value)),
        );
    }

    /// C# `GetSortedGasRecords(snapshot, end)`: the `Prefix_GasPerBlock` records with
    /// index ≤ `end`, descending by index.
    pub(super) fn sorted_gas_records(&self, snapshot: &DataCache, end: u32) -> Vec<(u32, BigInt)> {
        let prefix = Self::gas_per_block_prefix_key();
        let mut out = Vec::new();
        for (key, item) in snapshot.find(Some(&prefix), SeekDirection::Backward) {
            let key_bytes = key.key();
            if key_bytes.len() >= 5 {
                let index =
                    u32::from_be_bytes([key_bytes[1], key_bytes[2], key_bytes[3], key_bytes[4]]);
                if index <= end {
                    out.push((index, BigInt::from_signed_bytes_le(&item.value_bytes())));
                }
            }
        }
        out
    }

    /// Reads the accumulated GAS-per-vote for `pubkey` (`Prefix_VoterRewardPerCommittee`).
    pub(super) fn voter_reward_per_committee(
        &self,
        snapshot: &DataCache,
        pubkey: &ECPoint,
    ) -> BigInt {
        let key = Self::voter_reward_per_committee_key(pubkey);
        snapshot
            .get(&key)
            .map(|item| BigInt::from_signed_bytes_le(&item.value_bytes()))
            .unwrap_or_else(|| BigInt::from(0))
    }

    /// C# `NeoToken.CalculateBonus`: the unclaimed GAS for an account between
    /// `BalanceHeight` and `end` — the NEO-holder reward (`balance * Σ gasPerBlock *
    /// 10 / 100 / TotalAmount`) plus the vote reward (`balance * (latestGasPerVote -
    /// lastGasPerVote) / VoteFactor`).
    pub(super) fn calculate_bonus(
        &self,
        snapshot: &DataCache,
        state: &NeoAccountStateView,
        end: u32,
    ) -> CoreResult<BigInt> {
        if state.balance == BigInt::from(0) {
            return Ok(BigInt::from(0));
        }
        if state.balance < BigInt::from(0) {
            return Err(CoreError::invalid_operation(
                "NeoToken account balance cannot be negative",
            ));
        }
        if state.balance_height >= end {
            return Ok(BigInt::from(0));
        }

        // NEO-holder reward over [BalanceHeight, end), folding in each gas-per-block
        // change point (C# CalculateReward).
        let start = state.balance_height;
        let mut sum_gas_per_block = BigInt::from(0);
        let mut window_end = end;
        for (index, gas_per_block) in self.sorted_gas_records(snapshot, end.saturating_sub(1)) {
            if index > start {
                sum_gas_per_block += &gas_per_block * (window_end - index);
                window_end = index;
            } else {
                sum_gas_per_block += &gas_per_block * (window_end - start);
                break;
            }
        }
        let neo_holder_reward =
            &state.balance * &sum_gas_per_block * NEO_HOLDER_REWARD_RATIO / 100 / NEO_TOTAL_AMOUNT;

        // Vote reward (only when the account currently votes).
        let vote_reward = match &state.vote_to {
            Some(vote) => {
                let latest = self.voter_reward_per_committee(snapshot, vote);
                &state.balance * (latest - &state.last_gas_per_vote) / VOTE_FACTOR
            }
            None => BigInt::from(0),
        };

        Ok(neo_holder_reward + vote_reward)
    }
}
