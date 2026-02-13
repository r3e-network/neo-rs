//
// bonus.rs - GAS reward and bonus calculations
//

use super::*;

impl NeoToken {
    pub fn unclaimed_gas<S>(&self, snapshot: &S, account: &UInt160, end: u32) -> CoreResult<BigInt>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let Some(state) = self.get_account_state(snapshot, account)? else {
            return Ok(BigInt::zero());
        };
        self.calculate_bonus(snapshot, &state, end)
    }

    pub fn balance_of_snapshot<S>(&self, snapshot: &S, account: &UInt160) -> CoreResult<BigInt>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let state = self.get_account_state(snapshot, account)?;
        Ok(state
            .map(|account_state| account_state.balance().clone())
            .unwrap_or_else(BigInt::zero))
    }

    pub(super) fn get_account_state<S>(
        &self,
        snapshot: &S,
        account: &UInt160,
    ) -> CoreResult<Option<NeoAccountState>>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let key = StorageKey::create_with_uint160(Self::ID, PREFIX_ACCOUNT, account);
        let Some(item) = snapshot.try_get(&key) else {
            return Ok(None);
        };
        NeoAccountState::from_storage_item(&item)
            .map(Some)
            .map_err(CoreError::native_contract)
    }

    pub(super) fn calculate_bonus<S>(
        &self,
        snapshot: &S,
        state: &NeoAccountState,
        end: u32,
    ) -> CoreResult<BigInt>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        if state.balance().is_zero() {
            return Ok(BigInt::zero());
        }
        if state.balance().sign() == num_bigint::Sign::Minus {
            return Err(CoreError::native_contract(
                "account balance cannot be negative".to_string(),
            ));
        }

        let ledger = LedgerContract::new();
        let expect_end = ledger.current_index(snapshot)? + 1;
        if expect_end != end {
            return Err(CoreError::native_contract(
                "end height must equal current height + 1".to_string(),
            ));
        }
        if state.balance_height() >= end {
            return Ok(BigInt::zero());
        }

        let neo_holder_reward = self.calculate_neo_holder_reward(
            snapshot,
            state.balance(),
            state.balance_height(),
            end,
        )?;
        if let Some(vote_to) = state.vote_to() {
            let latest = self.latest_gas_per_vote(snapshot, vote_to);
            let delta = latest - state.last_gas_per_vote();
            let mut reward = state.balance() * delta;
            reward /= BigInt::from(Self::DATOSHI_FACTOR);
            Ok(neo_holder_reward + reward)
        } else {
            Ok(neo_holder_reward)
        }
    }

    fn calculate_neo_holder_reward<S>(
        &self,
        snapshot: &S,
        value: &BigInt,
        start: u32,
        mut end: u32,
    ) -> CoreResult<BigInt>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        if start >= end {
            return Ok(BigInt::zero());
        }

        let mut sum = BigInt::zero();
        let records = self.get_sorted_gas_records(snapshot, end.saturating_sub(1));
        for (index, gas_per_block) in records {
            if index > start {
                let diff = BigInt::from(end - index);
                sum += gas_per_block * diff;
                end = index;
            } else {
                let diff = BigInt::from(end - start);
                sum += gas_per_block * diff;
                break;
            }
        }

        if sum.is_zero() {
            return Ok(BigInt::zero());
        }

        let numerator =
            value * sum * BigInt::from(Self::NEO_HOLDER_REWARD_RATIO) / BigInt::from(100);
        Ok(numerator / BigInt::from(Self::TOTAL_SUPPLY))
    }

    pub(super) fn latest_gas_per_vote<S>(&self, snapshot: &S, vote_to: &ECPoint) -> BigInt
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let key = StorageKey::create_with_bytes(
            Self::ID,
            Self::PREFIX_VOTER_REWARD_PER_COMMITTEE,
            vote_to.as_bytes(),
        );
        snapshot
            .try_get(&key)
            .map(|item| item.to_bigint())
            .unwrap_or_else(BigInt::zero)
    }

    pub(super) fn get_sorted_gas_records<S>(&self, snapshot: &S, end: u32) -> Vec<(u32, BigInt)>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let prefix = StorageKey::create(Self::ID, Self::PREFIX_GAS_PER_BLOCK);
        let mut records = Vec::with_capacity(8);
        for (key, item) in snapshot.find(Some(&prefix), SeekDirection::Backward) {
            if key.id != Self::ID {
                continue;
            }
            let suffix = key.suffix();
            if suffix.first().copied() != Some(Self::PREFIX_GAS_PER_BLOCK) || suffix.len() < 5 {
                continue;
            }
            let idx_bytes = &suffix[suffix.len() - 4..];
            let index =
                u32::from_be_bytes([idx_bytes[0], idx_bytes[1], idx_bytes[2], idx_bytes[3]]);
            if index > end {
                continue;
            }
            records.push((index, item.to_bigint()));
        }
        records.sort_by(|a, b| b.0.cmp(&a.0));
        records
    }
}
