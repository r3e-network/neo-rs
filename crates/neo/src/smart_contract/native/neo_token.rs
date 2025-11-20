use super::{
    fungible_token::PREFIX_ACCOUNT,
    native_contract::{NativeContract, NativeMethod},
};
use crate::cryptography::crypto_utils::ECPoint;
use crate::error::{CoreError, CoreResult};
use crate::persistence::{i_read_only_store::IReadOnlyStoreGeneric, seek_direction::SeekDirection};
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::binary_serializer::BinarySerializer;
use crate::smart_contract::helper::Helper;
use crate::smart_contract::native::ledger_contract::LedgerContract;
use crate::smart_contract::storage_key::StorageKey;
use crate::smart_contract::StorageItem;
use crate::UInt160;
use lazy_static::lazy_static;
use neo_vm::{stack_item::StackItem, ExecutionEngineLimits};
use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero};
use std::any::Any;

lazy_static! {
    static ref NEO_HASH: UInt160 = Helper::get_contract_hash(&UInt160::zero(), 0, "NeoToken");
}

/// Simplified representation of the NEO native contract exposing the canonical
/// identifiers used throughout the node. Full voting and reward distribution
/// logic will be introduced once the surrounding infrastructure is ported.
pub struct NeoToken {
    methods: Vec<NativeMethod>,
}

impl Default for NeoToken {
    fn default() -> Self {
        Self::new()
    }
}

impl NeoToken {
    const ID: i32 = -5;
    const SYMBOL: &'static str = "NEO";
    const DECIMALS: u8 = 0;
    const NAME: &'static str = "NeoToken";
    const TOTAL_SUPPLY: i64 = 100_000_000;
    const PREFIX_COMMITTEE: u8 = 14;
    const PREFIX_VOTER_REWARD_PER_COMMITTEE: u8 = 23;
    const PREFIX_GAS_PER_BLOCK: u8 = 29;
    const NEO_HOLDER_REWARD_RATIO: i64 = 10;
    const DATOSHI_FACTOR: i64 = 100_000_000;

    pub fn new() -> Self {
        let methods = vec![
            NativeMethod::safe("symbol".to_string(), 1),
            NativeMethod::safe("decimals".to_string(), 1),
            NativeMethod::safe("totalSupply".to_string(), 1),
        ];

        Self { methods }
    }

    fn total_supply_bytes() -> Vec<u8> {
        let mut bytes = BigInt::from(Self::TOTAL_SUPPLY).to_signed_bytes_le();
        if bytes.is_empty() {
            bytes.push(0);
        }
        bytes
    }

    fn invoke_method(&self, method: &str) -> CoreResult<Vec<u8>> {
        match method {
            "symbol" => Ok(Self::SYMBOL.as_bytes().to_vec()),
            "decimals" => Ok(vec![Self::DECIMALS]),
            "totalSupply" => Ok(Self::total_supply_bytes()),
            _ => Err(CoreError::native_contract(format!(
                "Method not implemented: {}",
                method
            ))),
        }
    }

    pub fn symbol(&self) -> &'static str {
        Self::SYMBOL
    }

    pub fn decimals(&self) -> u8 {
        Self::DECIMALS
    }

    pub fn total_supply(&self) -> BigInt {
        BigInt::from(Self::TOTAL_SUPPLY)
    }

    /// Attempts to read the current committee from the snapshot-backed storage used by the
    /// native NEO contract. Returns `None` when the committee cache has not been populated yet.
    pub fn committee_from_snapshot<S>(&self, snapshot: &S) -> Option<Vec<ECPoint>>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let key = StorageKey::create(Self::ID, Self::PREFIX_COMMITTEE);
        let item = snapshot.try_get(&key)?;
        let bytes = item.get_value();
        let stack_item =
            BinarySerializer::deserialize(&bytes, &ExecutionEngineLimits::default(), None).ok()?;

        Self::decode_committee_stack_item(stack_item).ok()
    }

    fn decode_committee_stack_item(item: StackItem) -> Result<Vec<ECPoint>, String> {
        use neo_vm::stack_item::StackItem as VmStackItem;

        fn stack_item_to_bytes(item: &VmStackItem) -> Option<Vec<u8>> {
            match item {
                VmStackItem::ByteString(bytes) => Some(bytes.clone()),
                VmStackItem::Buffer(buffer) => Some(buffer.data().to_vec()),
                _ => None,
            }
        }

        fn decode_entry(entry: &VmStackItem) -> Result<Option<ECPoint>, String> {
            let elements: Vec<VmStackItem> = match entry {
                VmStackItem::Struct(structure) => structure.items().to_vec(),
                VmStackItem::Array(array) => array.items().to_vec(),
                _ => return Ok(None),
            };

            let first = elements
                .first()
                .ok_or_else(|| "committee entry missing public key".to_string())?;
            let key_bytes = stack_item_to_bytes(first)
                .ok_or_else(|| "committee entry public key must be byte array".to_string())?;
            let point = ECPoint::from_bytes(&key_bytes)
                .map_err(|e| format!("invalid committee public key: {e}"))?;
            Ok(Some(point))
        }

        match item {
            VmStackItem::Array(array) => {
                let mut committee = Vec::with_capacity(array.len());
                for entry in array.items() {
                    if let Some(point) = decode_entry(entry)? {
                        committee.push(point);
                    }
                }
                if committee.is_empty() {
                    Err("committee cache empty".to_string())
                } else {
                    Ok(committee)
                }
            }
            VmStackItem::Struct(structure) => {
                let mut committee = Vec::with_capacity(structure.len());
                for entry in structure.items() {
                    if let Some(point) = decode_entry(entry)? {
                        committee.push(point);
                    }
                }
                if committee.is_empty() {
                    Err("committee cache empty".to_string())
                } else {
                    Ok(committee)
                }
            }
            _ => Err("unexpected committee cache format".to_string()),
        }
    }
}

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

    fn get_account_state<S>(
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

    fn calculate_bonus<S>(
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

    fn latest_gas_per_vote<S>(&self, snapshot: &S, vote_to: &ECPoint) -> BigInt
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

    fn get_sorted_gas_records<S>(&self, snapshot: &S, end: u32) -> Vec<(u32, BigInt)>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let prefix = StorageKey::create(Self::ID, Self::PREFIX_GAS_PER_BLOCK);
        let mut records = Vec::new();
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

impl NativeContract for NeoToken {
    fn id(&self) -> i32 {
        Self::ID
    }

    fn hash(&self) -> UInt160 {
        *NEO_HASH
    }

    fn name(&self) -> &str {
        Self::NAME
    }

    fn methods(&self) -> &[NativeMethod] {
        &self.methods
    }

    fn is_active(&self, _settings: &ProtocolSettings, _block_height: u32) -> bool {
        true
    }

    fn invoke(
        &self,
        _engine: &mut ApplicationEngine,
        method: &str,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        self.invoke_method(method)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Clone, Debug)]
struct NeoAccountState {
    balance: BigInt,
    balance_height: u32,
    vote_to: Option<ECPoint>,
    last_gas_per_vote: BigInt,
}

impl NeoAccountState {
    fn from_storage_item(item: &StorageItem) -> Result<Self, String> {
        let bytes = item.get_value();
        let stack_item =
            BinarySerializer::deserialize(&bytes, &ExecutionEngineLimits::default(), None)
                .map_err(|err| format!("failed to deserialize NeoAccountState: {}", err))?;
        Self::from_stack_item(stack_item)
    }

    fn from_stack_item(item: StackItem) -> Result<Self, String> {
        let structure = match item {
            StackItem::Struct(structure) => structure,
            other => {
                return Err(format!(
                    "expected NeoAccountState struct, found {:?}",
                    other.stack_item_type()
                ))
            }
        };
        let entries = structure.items();
        if entries.len() < 4 {
            return Err("NeoAccountState struct missing fields".to_string());
        }

        let balance = entries[0]
            .as_int()
            .map_err(|err| format!("invalid balance: {}", err))?;
        let balance_height_big = entries[1]
            .as_int()
            .map_err(|err| format!("invalid balance height: {}", err))?;
        let balance_height = balance_height_big
            .to_u32()
            .ok_or_else(|| "balance height out of range".to_string())?;

        let vote_to = if entries[2].is_null() {
            None
        } else {
            let bytes = match &entries[2] {
                StackItem::ByteString(data) => data.clone(),
                StackItem::Buffer(buf) => buf.data().to_vec(),
                other => {
                    return Err(format!(
                        "vote target must be byte array, found {:?}",
                        other.stack_item_type()
                    ))
                }
            };
            Some(
                ECPoint::from_bytes(&bytes)
                    .map_err(|err| format!("invalid vote public key: {}", err))?,
            )
        };

        let last_gas_per_vote = entries[3]
            .as_int()
            .map_err(|err| format!("invalid last gas per vote: {}", err))?;

        Ok(Self {
            balance,
            balance_height,
            vote_to,
            last_gas_per_vote,
        })
    }

    fn balance(&self) -> &BigInt {
        &self.balance
    }

    fn balance_height(&self) -> u32 {
        self.balance_height
    }

    fn vote_to(&self) -> Option<&ECPoint> {
        self.vote_to.as_ref()
    }

    fn last_gas_per_vote(&self) -> &BigInt {
        &self.last_gas_per_vote
    }
}
