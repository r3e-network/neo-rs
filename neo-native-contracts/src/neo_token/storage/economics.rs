//! NEO economic storage records and unclaimed-GAS calculation.

use super::*;
use neo_error::CoreError;
use num_traits::ToPrimitive;

impl NeoToken {
    /// C# `GetRegisterPrice` = `(long)(BigInteger)snapshot[_registerPrice]`.
    pub(in crate::neo_token) fn register_price<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> CoreResult<i64> {
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
    pub(in crate::neo_token) fn put_register_price<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        price: i64,
    ) -> CoreResult<()> {
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
    pub(in crate::neo_token) fn put_gas_per_block<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
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
    pub(in crate::neo_token) fn gas_per_block_at<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        index: u32,
    ) -> BigInt {
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
    pub(in crate::neo_token) fn read_voters_count<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> BigInt {
        snapshot
            .get(&Self::voters_count_key())
            .map(|item| BigInt::from_signed_bytes_le(&item.value_bytes()))
            .unwrap_or_else(|| BigInt::from(0))
    }

    /// Writes the total voted NEO (`Prefix_VotersCount`).
    pub(in crate::neo_token) fn write_voters_count<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        value: &BigInt,
    ) {
        snapshot.update(
            Self::voters_count_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(value)),
        );
    }

    /// C# `GetSortedGasRecords(snapshot, end)`: the `Prefix_GasPerBlock` records with
    /// index ≤ `end`, descending by index.
    pub(in crate::neo_token) fn sorted_gas_records<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        end: u32,
    ) -> Vec<(u32, BigInt)> {
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
    pub(in crate::neo_token) fn voter_reward_per_committee<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
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
    pub(in crate::neo_token) fn calculate_bonus<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
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
