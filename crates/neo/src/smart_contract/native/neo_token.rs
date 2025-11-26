use super::{
    fungible_token::PREFIX_ACCOUNT,
    helpers::NativeHelpers,
    native_contract::{NativeContract, NativeMethod},
};
use crate::cryptography::crypto_utils::ECPoint;
use crate::error::{CoreError, CoreResult};
use crate::neo_config::SECONDS_PER_BLOCK;
use crate::persistence::{i_read_only_store::IReadOnlyStoreGeneric, seek_direction::SeekDirection};
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::binary_serializer::BinarySerializer;
use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::helper::Helper;
use crate::smart_contract::native::ledger_contract::LedgerContract;
use crate::smart_contract::storage_context::StorageContext;
use crate::smart_contract::storage_key::StorageKey;
use crate::smart_contract::Contract;
use crate::smart_contract::StorageItem;
use crate::UInt160;
use lazy_static::lazy_static;
use neo_vm::{stack_item::StackItem, ExecutionEngineLimits};
use num_bigint::BigInt;
use num_traits::{Signed, ToPrimitive, Zero};
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
    const PREFIX_CANDIDATE: u8 = 33;
    const PREFIX_VOTER_REWARD_PER_COMMITTEE: u8 = 23;
    const PREFIX_GAS_PER_BLOCK: u8 = 29;
    const PREFIX_REGISTER_PRICE: u8 = 13;
    const NEO_HOLDER_REWARD_RATIO: i64 = 10;
    const DATOSHI_FACTOR: i64 = 100_000_000;
    /// Default register price: 1000 GAS (in Datoshi)
    const DEFAULT_REGISTER_PRICE: i64 = 1000_00000000;

    pub fn new() -> Self {
        // Method registrations matching C# NeoToken exactly
        let methods = vec![
            // NEP-17 standard methods
            NativeMethod::safe("symbol".to_string(), 0),
            NativeMethod::safe("decimals".to_string(), 0),
            NativeMethod::safe("totalSupply".to_string(), 1 << 4),
            NativeMethod::safe("balanceOf".to_string(), 1 << 4),
            NativeMethod::unsafe_method(
                "transfer".to_string(),
                1 << SECONDS_PER_BLOCK,
                CallFlags::ALL.bits(),
            ),
            // Governance query methods (safe)
            NativeMethod::safe("unclaimedGas".to_string(), 1 << 4),
            NativeMethod::safe("getAccountState".to_string(), 1 << 4),
            NativeMethod::safe("getCandidates".to_string(), 1 << 22),
            NativeMethod::safe("getAllCandidates".to_string(), 1 << 22),
            NativeMethod::safe("getCandidateVote".to_string(), 1 << 4),
            NativeMethod::safe("getCommittee".to_string(), 1 << 4),
            NativeMethod::safe("getNextBlockValidators".to_string(), 1 << 4),
            NativeMethod::safe("getGasPerBlock".to_string(), 1 << 4),
            NativeMethod::safe("getRegisterPrice".to_string(), 1 << 4),
            // Governance write methods (unsafe - require witness/committee)
            NativeMethod::unsafe_method(
                "registerCandidate".to_string(),
                1 << SECONDS_PER_BLOCK,
                CallFlags::STATES.bits(),
            ),
            NativeMethod::unsafe_method(
                "unregisterCandidate".to_string(),
                1 << SECONDS_PER_BLOCK,
                CallFlags::STATES.bits(),
            ),
            NativeMethod::unsafe_method(
                "vote".to_string(),
                1 << SECONDS_PER_BLOCK,
                CallFlags::STATES.bits(),
            ),
            NativeMethod::unsafe_method(
                "setGasPerBlock".to_string(),
                1 << 4,
                CallFlags::STATES.bits(),
            ),
            NativeMethod::unsafe_method(
                "setRegisterPrice".to_string(),
                1 << 4,
                CallFlags::STATES.bits(),
            ),
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

    fn invoke_method(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        match method {
            // NEP-17 standard methods
            "symbol" => Ok(Self::SYMBOL.as_bytes().to_vec()),
            "decimals" => Ok(vec![Self::DECIMALS]),
            "totalSupply" => Ok(Self::total_supply_bytes()),
            "balanceOf" => self.balance_of(engine, args),
            "transfer" => self.transfer(engine, args),
            // Governance query methods
            "unclaimedGas" => self.unclaimed_gas_invoke(engine, args),
            "getAccountState" => self.get_account_state_invoke(engine, args),
            "getCandidates" => self.get_candidates(engine),
            "getAllCandidates" => self.get_all_candidates(engine),
            "getCandidateVote" => self.get_candidate_vote(engine, args),
            "getCommittee" => self.get_committee(engine),
            "getNextBlockValidators" => self.get_next_block_validators(engine),
            "getGasPerBlock" => self.get_gas_per_block(engine),
            "getRegisterPrice" => self.get_register_price(engine),
            // Governance write methods
            "registerCandidate" => self.register_candidate(engine, args),
            "unregisterCandidate" => self.unregister_candidate(engine, args),
            "vote" => self.vote(engine, args),
            "setGasPerBlock" => self.set_gas_per_block(engine, args),
            "setRegisterPrice" => self.set_register_price(engine, args),
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

    /// Determines whether the committee should be refreshed at the specified height.
    /// Committee is refreshed when height is a multiple of committee_members_count.
    /// Matches C# NeoToken.ShouldRefreshCommittee.
    pub fn should_refresh_committee(height: u32, committee_members_count: usize) -> bool {
        if committee_members_count == 0 {
            return false;
        }
        height % (committee_members_count as u32) == 0
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

/// NEP-17 and governance method implementations
impl NeoToken {
    /// Encodes a BigInt amount to bytes (C# compatible format)
    fn encode_amount(value: &BigInt) -> Vec<u8> {
        let mut bytes = value.to_signed_bytes_le();
        if bytes.is_empty() {
            bytes.push(0);
        }
        bytes
    }

    /// Decodes bytes to a BigInt amount
    fn decode_amount(data: &[u8]) -> BigInt {
        BigInt::from_signed_bytes_le(data)
    }

    /// Reads an account UInt160 from argument bytes
    fn read_account(&self, data: &[u8]) -> CoreResult<UInt160> {
        if data.len() != 20 {
            return Err(CoreError::native_contract(
                "Account argument must be 20 bytes".to_string(),
            ));
        }
        UInt160::from_bytes(data).map_err(|err| CoreError::native_contract(err.to_string()))
    }

    /// Reads an ECPoint public key from argument bytes
    fn read_public_key(&self, data: &[u8]) -> CoreResult<ECPoint> {
        if data.len() != 33 {
            return Err(CoreError::native_contract(
                "Public key argument must be 33 bytes".to_string(),
            ));
        }
        ECPoint::from_bytes(data).map_err(|err| CoreError::native_contract(err.to_string()))
    }

    /// balanceOf implementation - returns NEO balance of an account
    fn balance_of(&self, engine: &mut ApplicationEngine, args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
        if args.len() != 1 {
            return Err(CoreError::native_contract(
                "balanceOf expects exactly one argument".to_string(),
            ));
        }
        let account = self.read_account(&args[0])?;
        let snapshot = engine.snapshot_cache();
        let balance = self.balance_of_snapshot(snapshot.as_ref(), &account)?;
        Ok(Self::encode_amount(&balance))
    }

    /// transfer implementation - transfers NEO between accounts
    fn transfer(&self, engine: &mut ApplicationEngine, args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
        if args.len() < 3 {
            return Err(CoreError::native_contract(
                "transfer expects from, to, amount arguments".to_string(),
            ));
        }
        let from = self.read_account(&args[0])?;
        let to = self.read_account(&args[1])?;
        let amount = Self::decode_amount(&args[2]);

        // NEO has 0 decimals - amount must be non-negative integer
        if amount.is_negative() {
            return Err(CoreError::native_contract(
                "Amount cannot be negative".to_string(),
            ));
        }

        // Check witness for from address
        let caller = engine.calling_script_hash();
        if from != caller && !engine.check_witness_hash(&from) {
            return Ok(vec![0]); // false
        }

        if amount.is_zero() {
            self.emit_transfer_event(engine, Some(&from), Some(&to), &amount)?;
            return Ok(vec![1]); // true
        }

        let snapshot = engine.snapshot_cache();
        let mut from_balance = self.balance_of_snapshot(snapshot.as_ref(), &from)?;
        if from_balance < amount {
            return Ok(vec![0]); // Insufficient balance
        }
        from_balance -= &amount;
        let mut to_balance = self.balance_of_snapshot(snapshot.as_ref(), &to)?;
        to_balance += &amount;

        let context = engine.get_native_storage_context(&self.hash())?;
        self.write_account_balance(&context, engine, &from, &from_balance)?;
        self.write_account_balance(&context, engine, &to, &to_balance)?;
        self.emit_transfer_event(engine, Some(&from), Some(&to), &amount)?;
        Ok(vec![1]) // true
    }

    /// Writes account balance to storage
    fn write_account_balance(
        &self,
        context: &StorageContext,
        engine: &mut ApplicationEngine,
        account: &UInt160,
        balance: &BigInt,
    ) -> CoreResult<()> {
        let key = StorageKey::create_with_uint160(Self::ID, PREFIX_ACCOUNT, account)
            .suffix()
            .to_vec();
        if balance.is_zero() {
            engine.delete_storage_item(context, &key)?;
        } else {
            let bytes = Self::encode_amount(balance);
            engine.put_storage_item(context, &key, &bytes)?;
        }
        Ok(())
    }

    /// Emits Transfer event
    fn emit_transfer_event(
        &self,
        engine: &mut ApplicationEngine,
        from: Option<&UInt160>,
        to: Option<&UInt160>,
        amount: &BigInt,
    ) -> CoreResult<()> {
        let from_bytes = from.map(|addr| addr.to_bytes()).unwrap_or_default();
        let to_bytes = to.map(|addr| addr.to_bytes()).unwrap_or_default();
        let amount_bytes = Self::encode_amount(amount);
        engine.emit_event("Transfer", vec![from_bytes, to_bytes, amount_bytes])?;
        Ok(())
    }

    /// unclaimedGas invoke wrapper
    fn unclaimed_gas_invoke(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if args.len() < 2 {
            return Err(CoreError::native_contract(
                "unclaimedGas expects account and end arguments".to_string(),
            ));
        }
        let account = self.read_account(&args[0])?;
        let end = if args[1].len() >= 4 {
            u32::from_le_bytes([args[1][0], args[1][1], args[1][2], args[1][3]])
        } else {
            return Err(CoreError::native_contract(
                "Invalid end block argument".to_string(),
            ));
        };
        let snapshot = engine.snapshot_cache();
        let gas = self.unclaimed_gas(snapshot.as_ref(), &account, end)?;
        Ok(Self::encode_amount(&gas))
    }

    /// getAccountState invoke wrapper
    fn get_account_state_invoke(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if args.is_empty() {
            return Err(CoreError::native_contract(
                "getAccountState expects account argument".to_string(),
            ));
        }
        let account = self.read_account(&args[0])?;
        let snapshot = engine.snapshot_cache();
        match self.get_account_state(snapshot.as_ref(), &account)? {
            Some(state) => {
                // Return serialized account state
                let stack_item = state.to_stack_item();
                let bytes =
                    BinarySerializer::serialize(&stack_item, &ExecutionEngineLimits::default())
                        .map_err(CoreError::native_contract)?;
                Ok(bytes)
            }
            None => Ok(vec![]), // Null for non-existent account
        }
    }

    /// getCandidates - returns top candidates with votes
    fn get_candidates(&self, engine: &mut ApplicationEngine) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        let candidates = self.get_candidates_internal(snapshot.as_ref())?;
        // Return as serialized array (limited to 256 candidates per C# spec)
        let limited: Vec<_> = candidates.into_iter().take(256).collect();
        let items: Vec<StackItem> = limited
            .iter()
            .map(|(pk, votes)| {
                StackItem::from_struct(vec![
                    StackItem::from_byte_string(pk.as_bytes().to_vec()),
                    StackItem::from_int(votes.clone()),
                ])
            })
            .collect();
        let array = StackItem::from_array(items);
        let bytes = BinarySerializer::serialize(&array, &ExecutionEngineLimits::default())
            .map_err(CoreError::native_contract)?;
        Ok(bytes)
    }

    /// getAllCandidates - returns iterator over all candidates
    fn get_all_candidates(&self, engine: &mut ApplicationEngine) -> CoreResult<Vec<u8>> {
        // For now, return same as getCandidates (full implementation needs iterator support)
        self.get_candidates(engine)
    }

    /// getCandidateVote - returns vote count for specific candidate
    fn get_candidate_vote(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if args.is_empty() {
            return Err(CoreError::native_contract(
                "getCandidateVote expects public key argument".to_string(),
            ));
        }
        let pubkey = self.read_public_key(&args[0])?;
        let snapshot = engine.snapshot_cache();
        let key =
            StorageKey::create_with_bytes(Self::ID, Self::PREFIX_CANDIDATE, pubkey.as_bytes());
        match snapshot.as_ref().try_get(&key) {
            Some(item) => {
                let votes = item.to_bigint();
                Ok(Self::encode_amount(&votes))
            }
            None => {
                // Return -1 for non-registered candidate (matches C# behavior)
                Ok(Self::encode_amount(&BigInt::from(-1)))
            }
        }
    }

    /// Internal helper to get candidates from storage
    fn get_candidates_internal<S>(&self, snapshot: &S) -> CoreResult<Vec<(ECPoint, BigInt)>>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let prefix = StorageKey::create(Self::ID, Self::PREFIX_CANDIDATE);
        let mut candidates = Vec::new();
        for (key, item) in snapshot.find(Some(&prefix), SeekDirection::Forward) {
            if key.id != Self::ID {
                continue;
            }
            let suffix = key.suffix();
            if suffix.first().copied() != Some(Self::PREFIX_CANDIDATE) {
                continue;
            }
            // Extract public key from suffix (after prefix byte)
            let pk_bytes = &suffix[1..];
            if let Ok(pk) = ECPoint::from_bytes(pk_bytes) {
                let votes = item.to_bigint();
                // Only include candidates with non-negative votes (registered)
                if !votes.is_negative() {
                    candidates.push((pk, votes));
                }
            }
        }
        // Sort by votes descending
        candidates.sort_by(|a, b| b.1.cmp(&a.1));
        Ok(candidates)
    }

    /// getCommittee - returns current committee members
    fn get_committee(&self, engine: &mut ApplicationEngine) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        let committee = self
            .committee_from_snapshot(snapshot.as_ref())
            .unwrap_or_else(|| engine.protocol_settings().standby_committee.clone());

        let items: Vec<StackItem> = committee
            .iter()
            .map(|pk| StackItem::from_byte_string(pk.as_bytes().to_vec()))
            .collect();
        let array = StackItem::from_array(items);
        let bytes = BinarySerializer::serialize(&array, &ExecutionEngineLimits::default())
            .map_err(CoreError::native_contract)?;
        Ok(bytes)
    }

    /// getNextBlockValidators - returns validators for next block
    fn get_next_block_validators(&self, engine: &mut ApplicationEngine) -> CoreResult<Vec<u8>> {
        let validators = NativeHelpers::get_next_block_validators(engine.protocol_settings());
        let items: Vec<StackItem> = validators
            .iter()
            .map(|pk| StackItem::from_byte_string(pk.as_bytes().to_vec()))
            .collect();
        let array = StackItem::from_array(items);
        let bytes = BinarySerializer::serialize(&array, &ExecutionEngineLimits::default())
            .map_err(CoreError::native_contract)?;
        Ok(bytes)
    }

    /// getGasPerBlock - returns current GAS generation rate per block
    fn get_gas_per_block(&self, engine: &mut ApplicationEngine) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        let ledger = LedgerContract::new();
        let current_index = ledger.current_index(snapshot.as_ref()).unwrap_or(0);
        let gas_per_block = self.get_gas_per_block_internal(snapshot.as_ref(), current_index);
        Ok(Self::encode_amount(&gas_per_block))
    }

    /// Internal helper to get GAS per block at specific height
    fn get_gas_per_block_internal<S>(&self, snapshot: &S, index: u32) -> BigInt
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let records = self.get_sorted_gas_records(snapshot, index);
        records
            .first()
            .map(|(_, gas)| gas.clone())
            .unwrap_or_else(|| BigInt::from(5_00000000i64)) // Default 5 GAS per block
    }

    /// getRegisterPrice - returns current candidate registration price
    fn get_register_price(&self, engine: &mut ApplicationEngine) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        let key = StorageKey::create(Self::ID, Self::PREFIX_REGISTER_PRICE);
        let price = snapshot
            .as_ref()
            .try_get(&key)
            .map(|item| item.to_bigint())
            .unwrap_or_else(|| BigInt::from(Self::DEFAULT_REGISTER_PRICE));
        Ok(Self::encode_amount(&price))
    }

    /// registerCandidate - registers a public key as validator candidate
    fn register_candidate(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if args.is_empty() {
            return Err(CoreError::native_contract(
                "registerCandidate expects public key argument".to_string(),
            ));
        }
        let pubkey = self.read_public_key(&args[0])?;

        // Verify witness for the public key's address
        let account = Contract::create_signature_contract(pubkey.clone()).script_hash();
        if !engine.check_witness_hash(&account) {
            return Err(CoreError::native_contract(
                "No witness for candidate public key".to_string(),
            ));
        }

        // Check and deduct registration fee
        let snapshot = engine.snapshot_cache();
        let key = StorageKey::create(Self::ID, Self::PREFIX_REGISTER_PRICE);
        // TODO: Implement GAS burn for registration fee
        let _price = snapshot
            .as_ref()
            .try_get(&key)
            .map(|item| item.to_bigint())
            .unwrap_or_else(|| BigInt::from(Self::DEFAULT_REGISTER_PRICE));

        // Burn GAS for registration (full implementation pending)

        let context = engine.get_native_storage_context(&self.hash())?;
        let candidate_key =
            StorageKey::create_with_bytes(Self::ID, Self::PREFIX_CANDIDATE, pubkey.as_bytes())
                .suffix()
                .to_vec();

        // Initialize with 0 votes
        let zero_votes = Self::encode_amount(&BigInt::zero());
        engine.put_storage_item(&context, &candidate_key, &zero_votes)?;

        Ok(vec![1]) // true - success
    }

    /// unregisterCandidate - removes a public key from candidates
    fn unregister_candidate(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if args.is_empty() {
            return Err(CoreError::native_contract(
                "unregisterCandidate expects public key argument".to_string(),
            ));
        }
        let pubkey = self.read_public_key(&args[0])?;

        // Verify witness for the public key's address
        let account = Contract::create_signature_contract(pubkey.clone()).script_hash();
        if !engine.check_witness_hash(&account) {
            return Err(CoreError::native_contract(
                "No witness for candidate public key".to_string(),
            ));
        }

        let context = engine.get_native_storage_context(&self.hash())?;
        let candidate_key =
            StorageKey::create_with_bytes(Self::ID, Self::PREFIX_CANDIDATE, pubkey.as_bytes())
                .suffix()
                .to_vec();

        // Mark as unregistered by setting negative votes
        let neg_one = Self::encode_amount(&BigInt::from(-1));
        engine.put_storage_item(&context, &candidate_key, &neg_one)?;

        Ok(vec![1]) // true - success
    }

    /// vote - allows NEO holders to vote for a candidate
    fn vote(&self, engine: &mut ApplicationEngine, args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
        if args.len() < 2 {
            return Err(CoreError::native_contract(
                "vote expects account and voteTo arguments".to_string(),
            ));
        }
        let account = self.read_account(&args[0])?;
        let vote_to = if args[1].is_empty() {
            None // Cancel vote
        } else {
            Some(self.read_public_key(&args[1])?)
        };

        // Verify witness for the account
        if !engine.check_witness_hash(&account) {
            return Err(CoreError::native_contract(
                "No witness for voting account".to_string(),
            ));
        }

        let snapshot = engine.snapshot_cache();

        // Get current account state
        let state = self.get_account_state(snapshot.as_ref(), &account)?;
        if state.is_none() && vote_to.is_some() {
            return Err(CoreError::native_contract(
                "Account has no NEO balance to vote with".to_string(),
            ));
        }

        // If voting for a candidate, verify they are registered
        if let Some(ref pk) = vote_to {
            let candidate_key =
                StorageKey::create_with_bytes(Self::ID, Self::PREFIX_CANDIDATE, pk.as_bytes());
            match snapshot.as_ref().try_get(&candidate_key) {
                Some(item) => {
                    let votes = item.to_bigint();
                    if votes.is_negative() {
                        return Err(CoreError::native_contract(
                            "Cannot vote for unregistered candidate".to_string(),
                        ));
                    }
                }
                None => {
                    return Err(CoreError::native_contract(
                        "Candidate not found".to_string(),
                    ));
                }
            }
        }

        // Update account state with new vote target
        // In full implementation, this would also update vote counts
        // For now, just acknowledge the vote

        Ok(vec![1]) // true - success
    }

    /// setGasPerBlock - sets GAS generation rate (committee only)
    fn set_gas_per_block(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        // Verify committee witness
        let committee_address = NativeHelpers::committee_address(engine.protocol_settings(), None);
        if !engine.check_witness_hash(&committee_address) {
            return Err(CoreError::native_contract(
                "setGasPerBlock requires committee witness".to_string(),
            ));
        }

        if args.is_empty() {
            return Err(CoreError::native_contract(
                "setGasPerBlock expects gasPerBlock argument".to_string(),
            ));
        }
        let gas_per_block = Self::decode_amount(&args[0]);
        if gas_per_block.is_negative() || gas_per_block > BigInt::from(10_00000000i64) {
            return Err(CoreError::native_contract(
                "Invalid gasPerBlock value (must be 0-10 GAS)".to_string(),
            ));
        }

        let snapshot = engine.snapshot_cache();
        let ledger = LedgerContract::new();
        let current_index = ledger.current_index(snapshot.as_ref()).unwrap_or(0);

        let context = engine.get_native_storage_context(&self.hash())?;
        // Create key with block index suffix
        let mut key_data = vec![Self::PREFIX_GAS_PER_BLOCK];
        key_data.extend_from_slice(&current_index.to_be_bytes());

        engine.put_storage_item(&context, &key_data, &Self::encode_amount(&gas_per_block))?;
        Ok(vec![])
    }

    /// setRegisterPrice - sets candidate registration price (committee only)
    fn set_register_price(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        // Verify committee witness
        let committee_address = NativeHelpers::committee_address(engine.protocol_settings(), None);
        if !engine.check_witness_hash(&committee_address) {
            return Err(CoreError::native_contract(
                "setRegisterPrice requires committee witness".to_string(),
            ));
        }

        if args.is_empty() {
            return Err(CoreError::native_contract(
                "setRegisterPrice expects price argument".to_string(),
            ));
        }
        let price = Self::decode_amount(&args[0]);
        if price.is_negative() {
            return Err(CoreError::native_contract(
                "Register price cannot be negative".to_string(),
            ));
        }

        let context = engine.get_native_storage_context(&self.hash())?;
        let key = StorageKey::create(Self::ID, Self::PREFIX_REGISTER_PRICE)
            .suffix()
            .to_vec();
        engine.put_storage_item(&context, &key, &Self::encode_amount(&price))?;
        Ok(vec![])
    }
}

/// NeoAccountState helper methods
impl NeoAccountState {
    /// Converts account state to a StackItem for serialization
    fn to_stack_item(&self) -> StackItem {
        StackItem::from_struct(vec![
            StackItem::from_int(self.balance.clone()),
            StackItem::from_int(self.balance_height),
            match &self.vote_to {
                Some(pk) => StackItem::from_byte_string(pk.as_bytes().to_vec()),
                None => StackItem::Null,
            },
            StackItem::from_int(self.last_gas_per_vote.clone()),
        ])
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
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        self.invoke_method(engine, method, args)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    /// OnPersist: Refresh committee if required.
    /// Matches C# NeoToken.OnPersistAsync.
    fn on_persist(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        let block = engine.persisting_block().cloned().ok_or_else(|| {
            CoreError::native_contract("No persisting block available".to_string())
        })?;

        let committee_count = engine.protocol_settings().committee_members_count();
        // Refresh committee when block index is a multiple of committee count
        if Self::should_refresh_committee(block.index(), committee_count) {
            // In a full implementation, this would:
            // 1. Get current cached committee
            // 2. Recompute committee from votes
            // 3. Update storage
            // 4. Emit CommitteeChanged notification if changed (Cockatrice hardfork)
            //
            // For now, committee is derived from standby_committee in protocol settings,
            // so no storage update is needed during on_persist.
        }

        Ok(())
    }

    /// PostPersist: Distribute GAS rewards to committee members.
    /// Matches C# NeoToken.PostPersistAsync.
    fn post_persist(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        use super::gas_token::GasToken;

        let block = engine.persisting_block().cloned().ok_or_else(|| {
            CoreError::native_contract("No persisting block available".to_string())
        })?;

        let committee_count = engine.protocol_settings().committee_members_count();
        let _validators_count = engine.protocol_settings().validators_count;
        let snapshot = engine.snapshot_cache();

        // Get current committee
        let committee = self
            .committee_from_snapshot(snapshot.as_ref())
            .unwrap_or_else(|| engine.protocol_settings().standby_committee.clone());

        if committee.is_empty() {
            return Ok(());
        }

        // Calculate which committee member gets rewarded this block
        let index = (block.index() % committee_count as u32) as usize;
        if index >= committee.len() {
            return Ok(());
        }

        // Get GAS per block
        let gas_per_block = self.get_gas_per_block_internal(snapshot.as_ref(), block.index());

        // Committee reward: 10% of gas_per_block goes to the committee member
        let committee_reward_ratio: i64 = 10; // 10%
        let committee_reward: BigInt = &gas_per_block * committee_reward_ratio / 100;

        if !committee_reward.is_zero() {
            let pubkey = &committee[index];
            let account = Contract::create_signature_contract(pubkey.clone()).script_hash();
            let gas_token = GasToken::new();
            gas_token.mint(engine, &account, &committee_reward, false)?;
        }

        // Voter reward distribution (when committee refreshes)
        // Full implementation would update Prefix_VoterRewardPerCommittee for each committee member
        // based on their votes. This is complex and requires full voting state tracking.
        if Self::should_refresh_committee(block.index(), committee_count) {
            // Voter reward ratio: 80% distributed among voters
            // This would iterate through all committee members and update their voter reward accumulator
            // For now, this is a placeholder as full voting state is not yet implemented
        }

        Ok(())
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
