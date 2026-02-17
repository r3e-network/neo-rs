//
// governance.rs - Voting, candidate registration, and committee management
//

use super::*;
use crate::smart_contract::find_options::FindOptions;
use crate::smart_contract::iterators::StorageIterator;
use crate::smart_contract::native::security_fixes::{
    ReentrancyGuardType, SafeArithmetic, SecurityContext, StateValidator,
};

impl NeoToken {
    /// unclaimedGas invoke wrapper
    pub(super) fn unclaimed_gas_invoke(
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
    pub(super) fn get_account_state_invoke(
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
    pub(super) fn get_candidates(&self, engine: &mut ApplicationEngine) -> CoreResult<Vec<u8>> {
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
    pub(super) fn get_all_candidates(&self, engine: &mut ApplicationEngine) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        let prefix = StorageKey::create(Self::ID, Self::PREFIX_CANDIDATE);
        let policy = PolicyContract::new();
        let mut entries = Vec::new();

        for (key, item) in snapshot.find(Some(&prefix), SeekDirection::Forward) {
            if key.id != Self::ID {
                continue;
            }
            let suffix = key.suffix();
            if suffix.first().copied() != Some(Self::PREFIX_CANDIDATE) {
                continue;
            }

            let pk_bytes = &suffix[1..];
            let Ok(pk) = ECPoint::from_bytes(pk_bytes) else {
                continue;
            };

            let state =
                CandidateState::from_storage_item(&item).map_err(CoreError::native_contract)?;
            if !state.registered {
                continue;
            }

            let candidate_account = Contract::create_signature_contract(pk.clone()).script_hash();
            if policy
                .is_blocked_snapshot(snapshot.as_ref(), &candidate_account)
                .unwrap_or(false)
            {
                continue;
            }

            entries.push((key, item));
        }

        let options =
            FindOptions::RemovePrefix | FindOptions::DeserializeValues | FindOptions::PickField1;
        let iterator = StorageIterator::new(entries, 1, options);
        let iterator_id = engine
            .store_storage_iterator(iterator)
            .map_err(CoreError::native_contract)?;

        Ok(iterator_id.to_le_bytes().to_vec())
    }

    /// getCandidateVote - returns vote count for specific candidate
    pub(super) fn get_candidate_vote(
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
        if let Some(state) = self.get_candidate_state(snapshot.as_ref(), &pubkey)? {
            if state.registered {
                Ok(Self::encode_amount(&state.votes))
            } else {
                Ok(Self::encode_amount(&BigInt::from(-1)))
            }
        } else {
            Ok(Self::encode_amount(&BigInt::from(-1)))
        }
    }

    /// Snapshot helper for retrieving a candidate's vote count.
    pub fn get_candidate_vote_snapshot<S>(
        &self,
        snapshot: &S,
        pubkey: &ECPoint,
    ) -> CoreResult<BigInt>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        if let Some(state) = self.get_candidate_state(snapshot, pubkey)? {
            if state.registered {
                Ok(state.votes)
            } else {
                Ok(BigInt::from(-1))
            }
        } else {
            Ok(BigInt::from(-1))
        }
    }

    pub(super) fn get_candidate_state<S>(
        &self,
        snapshot: &S,
        pubkey: &ECPoint,
    ) -> CoreResult<Option<CandidateState>>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let key =
            StorageKey::create_with_bytes(Self::ID, Self::PREFIX_CANDIDATE, pubkey.as_bytes());
        let Some(item) = snapshot.try_get(&key) else {
            return Ok(None);
        };
        CandidateState::from_storage_item(&item)
            .map(Some)
            .map_err(CoreError::native_contract)
    }

    pub(super) fn write_candidate_state(
        &self,
        context: &StorageContext,
        engine: &mut ApplicationEngine,
        pubkey: &ECPoint,
        state: &CandidateState,
    ) -> CoreResult<()> {
        let candidate_suffix =
            StorageKey::create_with_bytes(Self::ID, Self::PREFIX_CANDIDATE, pubkey.as_bytes())
                .suffix()
                .to_vec();
        if !state.registered && state.votes.is_zero() {
            engine.delete_storage_item(context, &candidate_suffix)?;
            let voter_reward_suffix = StorageKey::create_with_bytes(
                Self::ID,
                Self::PREFIX_VOTER_REWARD_PER_COMMITTEE,
                pubkey.as_bytes(),
            )
            .suffix()
            .to_vec();
            engine.delete_storage_item(context, &voter_reward_suffix)?;
            return Ok(());
        }

        let bytes =
            BinarySerializer::serialize(&state.to_stack_item(), &ExecutionEngineLimits::default())
                .map_err(CoreError::native_contract)?;
        engine.put_storage_item(context, &candidate_suffix, &bytes)?;
        Ok(())
    }

    /// Internal helper to get candidates from storage
    pub(super) fn get_candidates_internal<S>(
        &self,
        snapshot: &S,
    ) -> CoreResult<Vec<(ECPoint, BigInt)>>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let prefix = StorageKey::create(Self::ID, Self::PREFIX_CANDIDATE);
        let mut candidates = Vec::new();
        let policy = PolicyContract::new();
        let blocked_accounts = policy.blocked_accounts_snapshot(snapshot);
        let has_blocked_accounts = !blocked_accounts.is_empty();
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
            let Ok(pk) = ECPoint::from_bytes(pk_bytes) else {
                continue;
            };

            let state =
                CandidateState::from_storage_item(&item).map_err(CoreError::native_contract)?;
            if !state.registered {
                continue;
            }

            let candidate_account = Contract::create_signature_contract(pk.clone()).script_hash();
            if has_blocked_accounts && blocked_accounts.contains(&candidate_account) {
                continue;
            }

            candidates.push((pk, state.votes));
        }
        Ok(candidates)
    }

    /// Snapshot helper for retrieving the registered candidates (limited to 256).
    pub fn get_candidates_snapshot<S>(&self, snapshot: &S) -> CoreResult<Vec<(ECPoint, BigInt)>>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let candidates = self.get_candidates_internal(snapshot)?;
        Ok(candidates.into_iter().take(256).collect())
    }

    /// getCommittee - returns current committee members
    pub(super) fn get_committee(&self, engine: &mut ApplicationEngine) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        let committee =
            self.committee_from_cache_with_votes(snapshot.as_ref(), engine.protocol_settings())?;
        let mut keys: Vec<ECPoint> = committee.into_iter().map(|(pk, _)| pk).collect();
        keys.sort();
        let items: Vec<StackItem> = keys
            .iter()
            .map(|pk| StackItem::from_byte_string(pk.as_bytes().to_vec()))
            .collect();
        let array = StackItem::from_array(items);
        let bytes = BinarySerializer::serialize(&array, &ExecutionEngineLimits::default())
            .map_err(CoreError::native_contract)?;
        Ok(bytes)
    }

    pub(super) fn get_committee_address(
        &self,
        engine: &mut ApplicationEngine,
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        let committee =
            self.committee_from_cache_with_votes(snapshot.as_ref(), engine.protocol_settings())?;
        let mut keys: Vec<ECPoint> = committee.into_iter().map(|(pk, _)| pk).collect();
        keys.sort();
        if keys.is_empty() {
            return Ok(vec![]);
        }
        let m = keys.len() - (keys.len().saturating_sub(1)) / 2;
        let redeem = Contract::create_multi_sig_redeem_script(m, &keys);
        Ok(UInt160::from_script(&redeem).to_bytes())
    }

    /// getNextBlockValidators - returns validators for next block
    pub(super) fn get_next_block_validators(
        &self,
        engine: &mut ApplicationEngine,
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        let validators = self.get_next_block_validators_snapshot(
            snapshot.as_ref(),
            usize::try_from(engine.protocol_settings().validators_count.max(0)).unwrap_or(0),
            engine.protocol_settings(),
        )?;
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
    pub(super) fn get_gas_per_block(&self, engine: &mut ApplicationEngine) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        let ledger = LedgerContract::new();
        let current_index = ledger.current_index(snapshot.as_ref()).unwrap_or(0);
        let gas_per_block =
            self.get_gas_per_block_internal(snapshot.as_ref(), current_index.saturating_add(1));
        Ok(Self::encode_amount(&gas_per_block))
    }

    /// Internal helper to get GAS per block at specific height
    pub(super) fn get_gas_per_block_internal<S>(&self, snapshot: &S, index: u32) -> BigInt
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
    pub(super) fn get_register_price(&self, engine: &mut ApplicationEngine) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        let key = StorageKey::create(Self::ID, Self::PREFIX_REGISTER_PRICE);
        let price = snapshot
            .as_ref()
            .try_get(&key)
            .map(|item| item.to_bigint())
            .unwrap_or_else(|| BigInt::from(Self::DEFAULT_REGISTER_PRICE));
        Ok(Self::encode_amount(&price))
    }

    /// onNEP17Payment - candidate registration via GAS transfer (Echidna+).
    pub(super) fn on_nep17_payment(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if args.len() < 3 {
            return Err(CoreError::native_contract(
                "onNEP17Payment expects from, amount, and data arguments".to_string(),
            ));
        }

        let gas_hash = GasToken::new().hash();
        if engine.calling_script_hash() != gas_hash {
            return Err(CoreError::native_contract(
                "Only GAS contract can call this method".to_string(),
            ));
        }

        let from = self.read_account(&args[0])?;
        let amount = Self::decode_amount(&args[1]);
        let pubkey = self.read_public_key(&args[2])?;

        let snapshot = engine.snapshot_cache();
        let register_price_key = StorageKey::create(Self::ID, Self::PREFIX_REGISTER_PRICE);
        let expected_price = snapshot
            .as_ref()
            .try_get(&register_price_key)
            .map(|item| item.to_bigint())
            .unwrap_or_else(|| BigInt::from(Self::DEFAULT_REGISTER_PRICE));

        if amount != expected_price {
            return Err(CoreError::native_contract(format!(
                "Incorrect GAS amount. Expected {} but received {}",
                expected_price, amount
            )));
        }

        if !self.register_internal(engine, &pubkey)? {
            return Err(CoreError::native_contract(
                "Failed to register candidate".to_string(),
            ));
        }

        // Burn the GAS payment from this contract (matches C# GAS.Burn(engine, Hash, amount)).
        GasToken::new().burn(engine, &self.hash(), &amount)?;

        // `from` is unused for state changes, but must be read to match the ABI.
        let _ = from;
        Ok(vec![])
    }

    /// registerCandidate - registers a public key as validator candidate
    pub(super) fn register_candidate(
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
        let account = Contract::create_signature_contract(pubkey.clone()).script_hash();
        let echidna_enabled = engine.is_hardfork_enabled(Hardfork::HfEchidna);
        if !echidna_enabled && !engine.check_witness_hash(&account)? {
            return Ok(vec![0]);
        }

        // Charge registration fee as execution fee (matches C# AddFee).
        let snapshot = engine.snapshot_cache();
        let key = StorageKey::create(Self::ID, Self::PREFIX_REGISTER_PRICE);
        let price = snapshot
            .as_ref()
            .try_get(&key)
            .map(|item| item.to_bigint())
            .unwrap_or_else(|| BigInt::from(Self::DEFAULT_REGISTER_PRICE));
        if price.is_negative() {
            return Err(CoreError::native_contract(
                "Registration price cannot be negative".to_string(),
            ));
        }
        if !price.is_zero() {
            let fee = price
                .to_u64()
                .ok_or_else(|| CoreError::native_contract("Register price overflow"))?;
            engine.add_runtime_fee(fee)?;
        }

        Ok(vec![if self.register_internal(engine, &pubkey)? {
            1
        } else {
            0
        }])
    }

    pub(super) fn register_internal(
        &self,
        engine: &mut ApplicationEngine,
        pubkey: &ECPoint,
    ) -> CoreResult<bool> {
        let snapshot = engine.snapshot_cache();
        let mut state = self
            .get_candidate_state(snapshot.as_ref(), pubkey)?
            .unwrap_or_default();
        if state.registered {
            return Ok(true);
        }

        state.registered = true;
        let context = engine.get_native_storage_context(&self.hash())?;
        self.write_candidate_state(&context, engine, pubkey, &state)?;

        engine
            .send_notification(
                self.hash(),
                "CandidateStateChanged".to_string(),
                vec![
                    StackItem::from_byte_string(pubkey.as_bytes().to_vec()),
                    StackItem::from_bool(true),
                    StackItem::from_int(state.votes.clone()),
                ],
            )
            .map_err(CoreError::native_contract)?;

        Ok(true)
    }

    /// unregisterCandidate - removes a public key from candidates
    pub(super) fn unregister_candidate(
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

        let account = Contract::create_signature_contract(pubkey.clone()).script_hash();
        if !engine.check_witness_hash(&account)? {
            return Ok(vec![0]);
        }

        let snapshot = engine.snapshot_cache();
        let Some(mut state) = self.get_candidate_state(snapshot.as_ref(), &pubkey)? else {
            return Ok(vec![1]);
        };
        if !state.registered {
            return Ok(vec![1]);
        }

        state.registered = false;
        let context = engine.get_native_storage_context(&self.hash())?;
        self.write_candidate_state(&context, engine, &pubkey, &state)?;

        engine
            .send_notification(
                self.hash(),
                "CandidateStateChanged".to_string(),
                vec![
                    StackItem::from_byte_string(pubkey.as_bytes().to_vec()),
                    StackItem::from_bool(false),
                    StackItem::from_int(state.votes.clone()),
                ],
            )
            .map_err(CoreError::native_contract)?;

        Ok(vec![1])
    }

    /// vote - allows NEO holders to vote for a candidate
    pub(super) fn vote(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if args.len() < 2 {
            return Err(CoreError::native_contract(
                "vote expects account and voteTo arguments".to_string(),
            ));
        }

        let account = self.read_account(&args[0])?;
        let vote_to = if args[1].is_empty() {
            None
        } else {
            Some(self.read_public_key(&args[1])?)
        };

        if !engine.check_witness_hash(&account)? {
            return Ok(vec![0]);
        }

        Ok(vec![if self.vote_internal(engine, &account, vote_to)? {
            1
        } else {
            0
        }])
    }

    /// vote_internal - actual voting logic (bypasses witness check)
    pub(crate) fn vote_internal(
        &self,
        engine: &mut ApplicationEngine,
        account: &UInt160,
        vote_to: Option<ECPoint>,
    ) -> CoreResult<bool> {
        // Enter reentrancy guard
        let _guard = SecurityContext::enter_guard(ReentrancyGuardType::NeoVote)?;

        let snapshot = engine.snapshot_cache();
        let snapshot_ref = snapshot.as_ref();
        let context = engine.get_native_storage_context(&self.hash())?;

        let mut state_account = match self.get_account_state(snapshot_ref, account)? {
            Some(state) => state,
            None => return Ok(false),
        };

        // Validate account state
        if state_account.balance.is_zero() {
            return Ok(false);
        }
        StateValidator::validate_account_state(
            &state_account.balance,
            state_account.balance_height,
            engine.current_block_index(),
        )?;

        let mut validator_new: Option<CandidateState> = None;
        if let Some(ref pk) = vote_to {
            let Some(state) = self.get_candidate_state(snapshot_ref, pk)? else {
                return Ok(false);
            };
            if !state.registered {
                return Ok(false);
            }
            StateValidator::validate_candidate_state(state.registered, &state.votes)?;
            validator_new = Some(state);
        }

        // Update voters count when switching between voting/non-voting.
        if state_account.vote_to.is_none() ^ vote_to.is_none() {
            let voters_key = StorageKey::create(Self::ID, Self::PREFIX_VOTERS_COUNT);
            let current_voters = snapshot_ref
                .try_get(&voters_key)
                .map(|item| item.to_bigint())
                .unwrap_or_else(BigInt::zero);

            // Validate voters count
            StateValidator::validate_voters_count(&current_voters, &current_voters)?;

            let delta = if state_account.vote_to.is_none() {
                state_account.balance.clone()
            } else {
                -state_account.balance.clone()
            };

            // Use safe arithmetic
            let updated_voters = SafeArithmetic::safe_add(&current_voters, &delta)?;

            // Validate updated voters count
            StateValidator::validate_voters_count(&updated_voters, &updated_voters)?;

            engine.put_storage_item(
                &context,
                voters_key.suffix(),
                &Self::encode_amount(&updated_voters),
            )?;
        }

        let gas_distribution = self.distribute_gas(engine, account, &mut state_account)?;

        // Remove votes from previous candidate.
        if let Some(old_vote) = state_account.vote_to.clone() {
            let mut old_state = self
                .get_candidate_state(snapshot_ref, &old_vote)?
                .unwrap_or_default();

            // Validate state before modification
            StateValidator::validate_candidate_state(old_state.registered, &old_state.votes)?;

            // Use safe arithmetic
            old_state.votes = SafeArithmetic::safe_sub(&old_state.votes, &state_account.balance)?;
            self.write_candidate_state(&context, engine, &old_vote, &old_state)?;
        }

        // Update LastGasPerVote for new vote target.
        if let Some(ref pk) = vote_to {
            if state_account.vote_to.as_ref() != Some(pk) {
                let latest = self.latest_gas_per_vote(snapshot_ref, pk);
                state_account.last_gas_per_vote = latest;
            }
        }

        let from = state_account.vote_to.clone();
        state_account.vote_to = vote_to.clone();

        if let Some(mut new_state) = validator_new {
            // Use safe arithmetic
            new_state.votes = SafeArithmetic::safe_add(&new_state.votes, &state_account.balance)?;

            // Validate final state
            StateValidator::validate_candidate_state(new_state.registered, &new_state.votes)?;

            self.write_candidate_state(&context, engine, vote_to.as_ref().unwrap(), &new_state)?;
        } else {
            state_account.last_gas_per_vote = BigInt::zero();
        }

        self.write_account_state(&context, engine, account, &state_account)?;

        engine
            .send_notification(
                self.hash(),
                "Vote".to_string(),
                vec![
                    StackItem::from_byte_string(account.to_bytes()),
                    from.as_ref()
                        .map(|pk| StackItem::from_byte_string(pk.as_bytes().to_vec()))
                        .unwrap_or_else(StackItem::null),
                    vote_to
                        .as_ref()
                        .map(|pk| StackItem::from_byte_string(pk.as_bytes().to_vec()))
                        .unwrap_or_else(StackItem::null),
                    StackItem::from_int(state_account.balance.clone()),
                ],
            )
            .map_err(CoreError::native_contract)?;

        if let Some(reward) = gas_distribution {
            GasToken::new().mint(engine, account, &reward, true)?;
        }

        Ok(true)
    }

    /// setGasPerBlock - sets GAS generation rate (committee only)
    pub(super) fn set_gas_per_block(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        // Verify committee witness against current committee address.
        let committee_address =
            NativeHelpers::committee_address(engine.protocol_settings(), Some(snapshot.as_ref()));
        if !engine.check_witness_hash(&committee_address)? {
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

        let ledger = LedgerContract::new();
        let fallback_index = ledger
            .current_index(snapshot.as_ref())
            .unwrap_or(0)
            .saturating_add(1);
        let index = engine
            .persisting_block()
            .map(|b| b.index().saturating_add(1))
            .unwrap_or(fallback_index);

        let context = engine.get_native_storage_context(&self.hash())?;
        // Create key with block index suffix
        let mut key_data = vec![Self::PREFIX_GAS_PER_BLOCK];
        key_data.extend_from_slice(&index.to_be_bytes());

        engine.put_storage_item(&context, &key_data, &Self::encode_amount(&gas_per_block))?;
        Ok(vec![])
    }

    /// setRegisterPrice - sets candidate registration price (committee only)
    pub(super) fn set_register_price(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        let committee_address =
            NativeHelpers::committee_address(engine.protocol_settings(), Some(snapshot.as_ref()));
        if !engine.check_witness_hash(&committee_address)? {
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
        if price.is_negative() || price.is_zero() {
            return Err(CoreError::native_contract(
                "Register price must be positive".to_string(),
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
