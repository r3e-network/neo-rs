
use std::collections::HashMap;
use std::sync::Arc;
use neo_proc_macros::contract_method;
use neo_vm::reference_counter::ReferenceCounter;
use neo_vm::stack_item::StackItem;
use crate::cryptography::ECCurve;
use crate::neo_contract::iinteroperable::IInteroperable;
use crate::neo_contract::storage_context::StorageContext;
use crate::persistence::{DataCache, SeekDirection};
use crate::uint160::UInt160;

pub struct NeoToken {
    total_amount: Integer,
}

impl NeoToken {
    const SYMBOL: &'static str = "NEO";
    const DECIMALS: u8 = 0;
    const EFFECTIVE_VOTER_TURNOUT: f64 = 0.2;

    const PREFIX_VOTERS_COUNT: u8 = 1;
    const PREFIX_CANDIDATE: u8 = 33;
    const PREFIX_COMMITTEE: u8 = 14;
    const PREFIX_GAS_PER_BLOCK: u8 = 29;
    const PREFIX_REGISTER_PRICE: u8 = 13;
    const PREFIX_VOTER_REWARD_PER_COMMITTEE: u8 = 23;

    const NEO_HOLDER_REWARD_RATIO: u8 = 10;
    const COMMITTEE_REWARD_RATIO: u8 = 10;
    const VOTER_REWARD_RATIO: u8 = 80;

    pub fn new() -> Self {
        Self {
            total_amount: Integer::from(100_000_000) * Self::factor(),
        }
    }

    fn factor() -> Integer {
        Integer::from(10).pow(Self::DECIMALS as u32)
    }

    pub fn total_supply(&self, snapshot: &dyn DataCache) -> Integer {
        self.total_amount.clone()
    }

    pub fn unclaimed_gas(&self, snapshot: &dyn DataCache, account: &UInt160, end: u32) -> Integer {
        let storage_key = self.create_storage_key(Self::PREFIX_ACCOUNT).add(account);
        if let Some(storage) = snapshot.try_get(&storage_key) {
            let state: NeoAccountState = storage.get_interoperable();
            self.calculate_bonus(snapshot, &state, end)
        } else {
            Integer::zero()
        }
    }

    fn calculate_bonus(&self, snapshot: &dyn DataCache, state: &NeoAccountState, end: u32) -> Integer {
        if state.balance.is_zero() {
            return Integer::zero();
        }
        if state.balance.sign() < 0 {
            panic!("Balance out of range");
        }

        let expect_end = Ledger::current_index(snapshot) + 1;
        if expect_end != end {
            panic!("End out of range");
        }
        if state.balance_height >= end {
            return Integer::zero();
        }

        let neo_holder_reward = self.calculate_neo_holder_reward(snapshot, &state.balance, state.balance_height, end);
        if state.vote_to.is_none() {
            return neo_holder_reward;
        }

        let key_latest = self.create_storage_key(Self::PREFIX_VOTER_REWARD_PER_COMMITTEE).add(&state.vote_to);
        let latest_gas_per_vote = snapshot.try_get(&key_latest).unwrap_or(Integer::zero());
        let vote_reward = &state.balance * (&latest_gas_per_vote - &state.last_gas_per_vote) / 100_000_000;

        neo_holder_reward + vote_reward
    }

    fn calculate_neo_holder_reward(&self, snapshot: &dyn DataCache, value: &Integer, start: u32, end: u32) -> Integer {
        let mut sum = Integer::zero();
        for (index, gas_per_block) in self.get_sorted_gas_records(snapshot, end - 1) {
            if index > start {
                sum += &gas_per_block * (end - index);
                end = index;
            } else {
                sum += &gas_per_block * (end - start);
                break;
            }
        }
        value * sum * Self::NEO_HOLDER_REWARD_RATIO / 100 / &self.total_amount
    }

    fn get_sorted_gas_records(&self, snapshot: &dyn DataCache, end: u32) -> Vec<(u32, Integer)> {
        let key = self.create_storage_key(Self::PREFIX_GAS_PER_BLOCK).add_big_endian(end);
        let boundary = self.create_storage_key(Self::PREFIX_GAS_PER_BLOCK);
        snapshot
            .find_range(&key, &boundary, SeekDirection::Backward)
            .map(|u| {
                let index = u32::from_be_bytes(u.key.key[u.key.key.len() - 4..].try_into().unwrap());
                (index, u.value.into())
            })
            .collect()
    }

    #[contract_method(cpu_fee = 1 << 15, storage_fee = 50, name = "unclaimedGas")]
    pub fn unclaimed_gas(&self, snapshot: &dyn DataCache, account: &UInt160, end: u32) -> Integer {
        let state = self.get_account_state(snapshot, account);
        if state.balance.is_zero() {
            return Integer::zero();
        }
        self.calculate_bonus(snapshot, &state, end)
    }

    #[contract_method(cpu_fee = 1 << 17, storage_fee = 50)]
    pub fn register_candidate(&self, engine: &mut ApplicationEngine, pubkey: ECPoint) -> bool {
        if !pubkey.is_valid() {
            return false;
        }
        let account = Contract::create_standard_account(&pubkey);
        let key = self.create_storage_key(Self::PREFIX_CANDIDATE_STATE).add(&pubkey);
        let snapshot = engine.snapshot_cache();
        let mut state: CandidateState = snapshot.get(&key).unwrap_or_default();
        if state.registered {
            return false;
        }
        if self.get_account_state(snapshot, &account).balance < Self::CANDIDATE_REGISTRATION_AMOUNT {
            return false;
        }
        state.registered = true;
        snapshot.put(&key, &state);
        true
    }

    #[contract_method(cpu_fee = 1 << 16, storage_fee = 50)]
    pub fn unregister_candidate(&self, engine: &mut ApplicationEngine, pubkey: ECPoint) -> bool {
        if !pubkey.is_valid() {
            return false;
        }
        let account = Contract::create_standard_account(&pubkey);
        let key = self.create_storage_key(Self::PREFIX_CANDIDATE_STATE).add(&pubkey);
        let snapshot = engine.snapshot_cache();
        let mut state: CandidateState = snapshot.get(&key).unwrap_or_default();
        if !state.registered {
            return false;
        }
        if !engine.check_witness(&account) {
            return false;
        }
        state.registered = false;
        snapshot.put(&key, &state);
        true
    }

    #[contract_method(cpu_fee = 1 << 16, storage_fee = 50)]
    pub fn vote(&self, engine: &mut ApplicationEngine, account: UInt160, vote_to: Option<ECPoint>) -> bool {
        if !engine.check_witness(&account) {
            return false;
        }
        let snapshot = engine.snapshot_cache();
        let mut state = self.get_account_state(snapshot, &account);
        if state.balance.is_zero() {
            return false;
        }
        if let Some(pubkey) = &vote_to {
            if !pubkey.is_valid() {
                return false;
            }
            let candidate_key = self.create_storage_key(Self::PREFIX_CANDIDATE_STATE).add(pubkey);
            let candidate_state: CandidateState = snapshot.get(&candidate_key).unwrap_or_default();
            if !candidate_state.registered {
                return false;
            }
        }
        let gas_to_claim = self.calculate_bonus(snapshot, &state, Ledger::current_index(snapshot) + 1);
        state.last_gas_per_vote = self.get_gas_per_vote(snapshot);
        state.vote_to = vote_to;
        let key = self.create_storage_key(Self::PREFIX_ACCOUNT).add(&account);
        snapshot.put(&key, &state);
        if !gas_to_claim.is_zero() {
            engine.transfer_gas(&self.hash, &account, gas_to_claim);
        }
        true
    }

    pub fn get_candidates(&self, snapshot: &StorageContext) -> Vec<(ECPoint, Integer)> {
        let prefix = self.create_storage_key(Self::PREFIX_CANDIDATE_STATE);
        snapshot.find(prefix.as_slice(), None)
            .filter_map(|(_, value)| {
                let state: CandidateState = value.get_interoperable();
                if state.registered {
                    Some((ECPoint::decode_point(&value.key[1..], ECCurve::Secp256r1).unwrap(), state.votes))
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn get_all_candidates(&self, snapshot: &StorageContext) -> Vec<(ECPoint, bool, Integer)> {
        let prefix = self.create_storage_key(Self::PREFIX_CANDIDATE_STATE);
        snapshot.find(prefix.as_slice(), None)
            .map(|(_, value)| {
                let state: CandidateState = value.get_interoperable();
                (ECPoint::decode_point(&value.key[1..], ECCurve::Secp256r1).unwrap(), state.registered, state.votes)
            })
            .collect()
    }

    pub fn get_committee(&self, snapshot: &StorageContext) -> Vec<ECPoint> {
        let key = self.create_storage_key(Self::PREFIX_COMMITTEE);
        snapshot.get(&key)
            .map(|value| value.get_interoperable())
            .unwrap_or_default()
    }

    pub fn get_gas_per_block(&self, snapshot: &StorageContext) -> Integer {
        let key = self.create_storage_key(Self::PREFIX_GAS_PER_BLOCK);
        snapshot.get(&key)
            .map(|value| value.get_interoperable())
            .unwrap_or_else(|| Integer::from(5 * Self::GENERATION_AMOUNT))
    }

    pub fn get_register_price(&self, snapshot: &StorageContext) -> Integer {
        let key = self.create_storage_key(Self::PREFIX_REGISTER_PRICE);
        snapshot.get(&key)
            .map(|value| value.get_interoperable())
            .unwrap_or_else(|| Integer::from(1000 * Self::GENERATION_AMOUNT))
    }

    #[contract_method(cpu_fee = 1 << 15, storage_fee = 50)]
    pub fn get_account_state(&self, snapshot: &StorageContext, account: &UInt160) -> NeoAccountState {
        let key = self.create_storage_key(Self::PREFIX_ACCOUNT).add(account);
        snapshot.get(&key)
            .map(|value| value.get_interoperable())
            .unwrap_or_default()
    }

    #[contract_method(cpu_fee = 1 << 15)]
    pub fn get_candidate_vote(&self, snapshot: &StorageContext, pubkey: ECPoint) -> Integer {
        if !pubkey.is_valid() {
            return Integer::zero();
        }
        let key = self.create_storage_key(Self::PREFIX_CANDIDATE_STATE).add(&pubkey);
        snapshot.get(&key)
            .map(|value| {
                let state: CandidateState = value.get_interoperable();
                state.votes
            })
            .unwrap_or_else(Integer::zero)
    }
}

pub struct NeoAccountState {
    pub balance: Integer,
    pub balance_height: u32,
    pub vote_to: Option<ECPoint>,
    pub last_gas_per_vote: Integer,
}

impl IInteroperable for NeoAccountState {
    fn from_stack_item(&mut self, item: StackItem) {
        if let StackItem::Struct(s) = item {
            self.balance = s[0].clone().into();
            self.balance_height = s[1].clone().into();
            self.vote_to = if s[2].is_null() {
                None
            } else {
                Some(ECPoint::decode_point(&s[2].get_span(), ECCurve::Secp256r1).unwrap())
            };
            self.last_gas_per_vote = s[3].clone().into();
        }
    }

    fn to_stack_item(&self, reference_counter: &mut ReferenceCounter) -> StackItem {
        let mut s = Struct::new(reference_counter);
        s.push(self.balance.clone().into());
        s.push(self.balance_height.into());
        s.push(match &self.vote_to {
            Some(v) => v.to_array().into(),
            None => StackItem::Null,
        });
        s.push(self.last_gas_per_vote.clone().into());
        StackItem::Struct(s)
    }
}

pub struct CandidateState {
    pub registered: bool,
    pub votes: Integer,
}

impl IInteroperable for CandidateState {
    fn from_stack_item(&mut self, item: StackItem) {
        if let StackItem::Struct(s) = item {
            self.registered = s[0].clone().into();
            self.votes = s[1].clone().into();
        }
    }

    fn to_stack_item(&self, reference_counter: &mut ReferenceCounter) -> StackItem {
        let mut s = Struct::new(reference_counter);
        s.push(self.registered.into());
        s.push(self.votes.clone().into());
        StackItem::Struct(s)
    }
}

pub struct CommitteeState {
    pub members: Vec<ECPoint>,
    pub standby_members: Vec<ECPoint>,
}

impl IInteroperable for CommitteeState {
    fn from_stack_item(&mut self, item: StackItem) {
        if let StackItem::Struct(s) = item {
            self.members = s[0].clone().into_iter().map(|i| ECPoint::decode_point(&i.get_span(), ECCurve::Secp256r1).unwrap()).collect();
            self.standby_members = s[1].clone().into_iter().map(|i| ECPoint::decode_point(&i.get_span(), ECCurve::Secp256r1).unwrap()).collect();
        }
    }

    fn to_stack_item(&self, reference_counter: &mut ReferenceCounter) -> StackItem {
        let mut s = Struct::new(reference_counter);
        s.push(self.members.iter().map(|m| m.to_array().into()).collect::<Array>().into());
        s.push(self.standby_members.iter().map(|m| m.to_array().into()).collect::<Array>().into());
        StackItem::Struct(s)
    }
}

impl NeoToken {
    pub fn balance_of(&self, snapshot: &dyn DataCache, account: &UInt160) -> Integer {
        let storage_key = self.create_storage_key(Self::PREFIX_ACCOUNT).add(account);
        if let Some(storage) = snapshot.try_get(&storage_key) {
            let state: NeoAccountState = storage.get_interoperable();
            state.balance
        } else {
            Integer::zero()
        }
    }

    pub fn transfer(&mut self, snapshot: &mut dyn DataCache, from: &UInt160, to: &UInt160, amount: &Integer, data: Option<&[u8]>) -> bool {
        if amount.sign() < 0 {
            return false;
        }
        if from == to {
            return true;
        }
        if self.balance_of(snapshot, from) < *amount {
            return false;
        }

        self.update_account(snapshot, from, amount, true);
        self.update_account(snapshot, to, amount, false);

        // Emit transfer event
        self.emit_transfer(snapshot, from, to, amount);

        // Handle data parameter if needed
        if let Some(data) = data {
            // Process data, e.g., call onNEP17Payment if 'to' is a contract
            if let Some(contract) = snapshot.get_contract_state(to) {
                let method = "onNEP17Payment";
                let params = vec![
                    StackItem::from(from.clone()),
                    StackItem::from(amount.clone()),
                    StackItem::from(ByteString::from(data)),
                ];
                
                if let Err(e) = Runtime::call_contract(snapshot, to, method, params) {
                    // Log the error, but don't revert the transfer
                    Runtime::log(snapshot, &format!("Error calling onNEP17Payment: {:?}", e));
                }
            }
        }

        true
    }

    fn update_account(&self, snapshot: &mut dyn DataCache, account: &UInt160, amount: &Integer, is_reduction: bool) {
        let storage_key = self.create_storage_key(Self::PREFIX_ACCOUNT).add(account);
        let mut state = if let Some(storage) = snapshot.try_get(&storage_key) {
            storage.get_interoperable::<NeoAccountState>()
        } else {
            NeoAccountState {
                balance: Integer::zero(),
                balance_height: Ledger::current_index(snapshot),
                vote_to: None,
                last_gas_per_vote: Integer::zero(),
            }
        };

        if is_reduction {
            state.balance -= amount;
        } else {
            state.balance += amount;
        }

        if state.balance.is_zero() {
            snapshot.delete(&storage_key);
        } else {
            snapshot.put(&storage_key, &state);
        }
    }

    fn emit_transfer(&self, snapshot: &dyn DataCache, from: &UInt160, to: &UInt160, amount: &Integer) {
        let runtime = Runtime::current(snapshot);
        runtime.notify(
            "Transfer",
            &[
                StackItem::from(from.clone()),
                StackItem::from(to.clone()),
                StackItem::from(amount.clone()),
            ],
        );
    }

    pub fn register_candidate(&mut self, snapshot: &mut dyn DataCache, pubkey: &ECPoint) -> bool {
        let key = self.create_storage_key(Self::PREFIX_CANDIDATE).add(pubkey);
        if snapshot.contains_key(&key) {
            return false;
        }

        let register_price = self.get_register_price(snapshot);
        if self.balance_of(snapshot, &Runtime::calling_script_hash(snapshot)) < register_price {
            return false;
        }

        let state = CandidateState {
            registered: true,
            votes: Integer::zero(),
        };
        snapshot.put(&key, &state);

        self.burn(snapshot, &Runtime::calling_script_hash(snapshot), &register_price);
        true
    }

    pub fn unregister_candidate(&mut self, snapshot: &mut dyn DataCache, pubkey: &ECPoint) -> bool {
        let key = self.create_storage_key(Self::PREFIX_CANDIDATE).add(pubkey);
        if let Some(storage) = snapshot.try_get(&key) {
            let state: CandidateState = storage.get_interoperable();
            if !state.registered {
                return false;
            }
            snapshot.delete(&key);
            true
        } else {
            false
        }
    }

    pub fn vote(&mut self, snapshot: &mut dyn DataCache, account: &UInt160, vote_to: Option<&ECPoint>) -> bool {
        let account_key = self.create_storage_key(Self::PREFIX_ACCOUNT).add(account);
        let mut state = if let Some(storage) = snapshot.try_get(&account_key) {
            storage.get_interoperable::<NeoAccountState>()
        } else {
            return false;
        };

        if state.balance.is_zero() {
            return false;
        }

        if let Some(old_vote) = &state.vote_to {
            self.update_candidate_votes(snapshot, old_vote, &state.balance, false);
        }

        if let Some(new_vote) = vote_to {
            if !self.is_candidate(snapshot, new_vote) {
                return false;
            }
            self.update_candidate_votes(snapshot, new_vote, &state.balance, true);
        }

        state.vote_to = vote_to.cloned();
        state.last_gas_per_vote = self.get_gas_per_vote(snapshot);
        snapshot.put(&account_key, &state);

        true
    }

    fn update_candidate_votes(&mut self, snapshot: &mut dyn DataCache, pubkey: &ECPoint, amount: &Integer, increase: bool) {
        let key = self.create_storage_key(Self::PREFIX_CANDIDATE).add(pubkey);
        if let Some(storage) = snapshot.try_get(&key) {
            let mut state: CandidateState = storage.get_interoperable();
            if increase {
                state.votes += amount;
            } else {
                state.votes -= amount;
            }
            snapshot.put(&key, &state);
        }
    }

    fn is_candidate(&self, snapshot: &dyn DataCache, pubkey: &ECPoint) -> bool {
        let key = self.create_storage_key(Self::PREFIX_CANDIDATE).add(pubkey);
        if let Some(storage) = snapshot.try_get(&key) {
            let state: CandidateState = storage.get_interoperable();
            state.registered
        } else {
            false
        }
    }

    fn get_register_price(&self, snapshot: &dyn DataCache) -> Integer {
        let key = self.create_storage_key(Self::PREFIX_REGISTER_PRICE);
        snapshot.get(&key).unwrap_or_else(|| Integer::from(1000 * 100000000))
    }

    fn get_gas_per_vote(&self, snapshot: &dyn DataCache) -> Integer {
        let key = self.create_storage_key(Self::PREFIX_GAS_PER_BLOCK);
        snapshot.get(&key).unwrap_or_else(Integer::zero)
    }

    fn burn(&mut self, snapshot: &mut dyn DataCache, account: &UInt160, amount: &Integer) {
        self.update_account(snapshot, account, amount, true);
        self.total_amount -= amount;
    }
}
