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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::DataCache;
    use crate::smart_contract::application_engine::ApplicationEngine;
    use crate::smart_contract::binary_serializer::BinarySerializer;
    use crate::smart_contract::native::NativeContract;
    use crate::smart_contract::trigger_type::TriggerType;
    use crate::UInt256;
    use std::sync::Arc;

    const PREFIX_CURRENT_BLOCK: u8 = 12;
    const TEST_GAS_LIMIT: i64 = 400_000_000;

    fn seed_ledger_current_index(snapshot: &DataCache, index: u32) {
        let key = StorageKey::create(LedgerContract::ID, PREFIX_CURRENT_BLOCK);
        let mut bytes = UInt256::zero().to_bytes().to_vec();
        bytes.extend_from_slice(&index.to_le_bytes());
        snapshot.add(key, StorageItem::from_bytes(bytes));
    }

    fn seed_neo_account(snapshot: &DataCache, account: &UInt160, state: NeoAccountState) {
        let key = StorageKey::create_with_uint160(NeoToken::ID, PREFIX_ACCOUNT, account);
        let bytes =
            BinarySerializer::serialize(&state.to_stack_item(), &ExecutionEngineLimits::default())
                .expect("serialize NeoAccountState");
        snapshot.add(key, StorageItem::from_bytes(bytes));
    }

    fn seed_gas_per_block(snapshot: &DataCache, index: u32, value: BigInt) {
        let mut suffix = vec![NeoToken::PREFIX_GAS_PER_BLOCK];
        suffix.extend_from_slice(&index.to_be_bytes());
        let key = StorageKey::new(NeoToken::ID, suffix);
        snapshot.add(key, StorageItem::from_bytes(value.to_signed_bytes_le()));
    }

    #[test]
    fn unclaimed_gas_helper_allows_future_end_heights_like_csharp_calculate_bonus() {
        let snapshot = DataCache::new(false);
        let neo = NeoToken::new();
        let account = UInt160::zero();

        seed_ledger_current_index(&snapshot, 1);
        seed_neo_account(
            &snapshot,
            &account,
            NeoAccountState {
                balance: BigInt::from(NeoToken::TOTAL_SUPPLY),
                balance_height: 0,
                vote_to: None,
                last_gas_per_vote: BigInt::zero(),
            },
        );
        seed_gas_per_block(&snapshot, 0, BigInt::from(5i64 * NeoToken::DATOSHI_FACTOR));

        let bonus = neo
            .unclaimed_gas(&snapshot, &account, 12)
            .expect("future end height should be accepted");

        assert_eq!(bonus, BigInt::from(6i64 * NeoToken::DATOSHI_FACTOR));
    }

    #[test]
    fn unclaimed_gas_contract_method_still_requires_expected_end_height() {
        let snapshot = Arc::new(DataCache::new(false));
        let neo = NeoToken::new();
        let account = UInt160::zero();

        seed_ledger_current_index(snapshot.as_ref(), 1);
        seed_neo_account(
            snapshot.as_ref(),
            &account,
            NeoAccountState {
                balance: BigInt::from(NeoToken::TOTAL_SUPPLY),
                balance_height: 0,
                vote_to: None,
                last_gas_per_vote: BigInt::zero(),
            },
        );
        seed_gas_per_block(
            snapshot.as_ref(),
            0,
            BigInt::from(5i64 * NeoToken::DATOSHI_FACTOR),
        );

        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            None,
            Arc::clone(&snapshot),
            None,
            ProtocolSettings::default(),
            TEST_GAS_LIMIT,
            None,
        )
        .expect("engine");

        let err = engine
            .call_native_contract(
                neo.hash(),
                "unclaimedGas",
                &[account.to_bytes(), 12u32.to_le_bytes().to_vec()],
            )
            .expect_err("contract method should reject unexpected end height");

        assert!(err.to_string().contains("end"));
    }
}
