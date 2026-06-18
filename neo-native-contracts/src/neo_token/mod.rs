//! NeoToken (NEO) native contract (id -5).
//!
//! Implements the C# `Neo.SmartContract.Native.NeoToken`: NEP-17 metadata
//! (`symbol` "NEO", `decimals` 0) and balances, the committee/candidate reads
//! (getCommittee, getCandidates, getNextBlockValidators, …), the committee
//! setters (setGasPerBlock, setRegisterPrice), candidate registration
//! (`registerCandidate` / `unregisterCandidate`), the GAS reward read
//! `unclaimedGas` (C# `CalculateBonus`), `vote`/`VoteInternal`, and NEP-17
//! `transfer` (with NEO's governance `OnBalanceChanging`: GAS reward
//! distribution + vote-weight tracking on both accounts). The full ABI surface
//! is implemented and byte-for-byte C# parity. What remains is the block-boundary
//! committee recompute (`OnPersist`, not an ABI method).

use std::any::Any;
use std::sync::LazyLock;

use neo_config::{Hardfork, ProtocolSettings};
use neo_crypto::ECPoint;
use neo_error::{CoreError, CoreResult};
use neo_execution::application_engine_contract::NativeArgNullMask;
use neo_execution::{ApplicationEngine, Contract, NativeContract, NativeEvent, NativeMethod};
use neo_primitives::{CallFlags, ContractParameterType, FindOptions, UInt160};
use neo_serialization::BinarySerializer;
use neo_storage::persistence::{DataCache, SeekDirection};
use neo_storage::{StorageItem, StorageKey};
use neo_vm::{Interoperable, StackItem};
use neo_vm_rs::{ExecutionEngineLimits, StackValue};
use num_bigint::BigInt;
use num_traits::ToPrimitive;

use crate::LedgerContract;
use crate::hashes::NEO_TOKEN_HASH;

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

/// The NeoToken native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct NeoToken;

impl NeoToken {
    /// Stable native contract id (matches C# `NeoToken`).
    pub const ID: i32 = -5;
    /// Stable native contract name (matches C# `NeoToken.Name`).
    pub const NAME: &'static str = "NeoToken";
    /// NEP-17 symbol (C# `NeoToken.Symbol => "NEO"`).
    pub const SYMBOL: &'static str = "NEO";
    /// NEP-17 decimals (C# `NeoToken.Decimals => 0`).
    pub const DECIMALS: u8 = 0;

    /// Construct a new `NeoToken` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the NEO script hash.
    pub fn hash(&self) -> UInt160 {
        Self::script_hash()
    }

    /// Returns the NEO script hash.
    pub fn script_hash() -> UInt160 {
        *NEO_TOKEN_HASH
    }

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

    /// C# `GetRegisterPrice` = `(long)(BigInteger)snapshot[_registerPrice]`.
    fn register_price(&self, snapshot: &DataCache) -> CoreResult<i64> {
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
    fn put_register_price(&self, snapshot: &DataCache, price: i64) -> CoreResult<()> {
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

    fn register_price_key() -> StorageKey {
        StorageKey::create(NeoToken::ID, PREFIX_REGISTER_PRICE)
    }

    /// C# `SetGasPerBlock` storage effect: write a `Prefix_GasPerBlock` record at
    /// `index` (a big-endian `uint` key suffix), overwriting any record already at
    /// that index (`GetAndChange(key, factory).Set(gasPerBlock)`). `update` upserts
    /// (a brand-new index key is tracked as Changed), which commits to the same
    /// stored key/value as the C# Added path — only the resulting store contents
    /// feed the state root.
    fn put_gas_per_block(&self, snapshot: &DataCache, index: u32, gas_per_block: &BigInt) {
        let key = StorageKey::create_with_uint32(NeoToken::ID, PREFIX_GAS_PER_BLOCK, index);
        snapshot.update(
            key,
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(gas_per_block)),
        );
    }

    /// Returns the GAS-per-block effective at `index`: the most recent
    /// `Prefix_GasPerBlock` record whose record index is ≤ `index` (C#
    /// `GetSortedGasRecords(...).First().GasPerBlock`), defaulting to 5 GAS.
    fn gas_per_block_at(&self, snapshot: &DataCache, index: u32) -> BigInt {
        let prefix = StorageKey::create(NeoToken::ID, PREFIX_GAS_PER_BLOCK);
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

    /// Decodes a stored `NeoAccountState` struct into its fields.
    fn decode_neo_account_state(value: &[u8]) -> CoreResult<NeoAccountStateView> {
        let limits = ExecutionEngineLimits::default();
        let decoded = BinarySerializer::deserialize_stack_value_with_limits(
            value,
            limits.max_item_size as usize,
            limits.max_stack_size as usize,
        )
        .map_err(|e| CoreError::deserialization(format!("neo account state: {e}")))?;
        NeoAccountStateView::from_stack_value(decoded)
    }

    /// Encodes a `NeoAccountState` (`Struct[Balance, BalanceHeight, VoteTo,
    /// LastGasPerVote]`) — the write counterpart of [`decode_neo_account_state`].
    fn encode_neo_account_state(state: &NeoAccountStateView) -> CoreResult<Vec<u8>> {
        let item = state.to_stack_value();
        BinarySerializer::serialize_stack_value_default(&item)
            .map_err(|e| CoreError::invalid_operation(format!("encode neo account state: {e}")))
    }

    /// The `Prefix_VotersCount` storage key (a single key, no suffix).
    fn voters_count_key() -> StorageKey {
        StorageKey::create(NeoToken::ID, PREFIX_VOTERS_COUNT)
    }

    /// Reads the total voted NEO (`Prefix_VotersCount`), defaulting to zero.
    fn read_voters_count(&self, snapshot: &DataCache) -> BigInt {
        snapshot
            .get(&Self::voters_count_key())
            .map(|item| BigInt::from_signed_bytes_le(&item.value_bytes()))
            .unwrap_or_else(|| BigInt::from(0))
    }

    /// Writes the total voted NEO (`Prefix_VotersCount`).
    fn write_voters_count(&self, snapshot: &DataCache, value: &BigInt) {
        snapshot.update(
            Self::voters_count_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(value)),
        );
    }

    /// C# `NeoToken.CheckCandidate`: when a candidate is unregistered and has no
    /// remaining votes, delete its candidate + voter-reward entries.
    fn check_candidate(
        &self,
        snapshot: &DataCache,
        pubkey: &ECPoint,
        registered: bool,
        votes: &BigInt,
    ) -> CoreResult<()> {
        if !registered && *votes == BigInt::from(0) {
            let reward_key = StorageKey::create_with_bytes(
                NeoToken::ID,
                PREFIX_VOTER_REWARD_PER_COMMITTEE,
                &pubkey.to_bytes(),
            );
            snapshot.delete(&reward_key);
            snapshot.delete(&Self::candidate_key(pubkey));
        } else {
            snapshot.update(
                Self::candidate_key(pubkey),
                StorageItem::from_bytes(Self::encode_candidate_state(registered, votes)?),
            );
        }
        Ok(())
    }

    /// C# `NeoToken.OnBalanceChanging`: invoked whenever an account's NEO balance is
    /// about to change by `amount` (a signed delta). It (a) computes the account's
    /// accrued GAS via `DistributeGas` — mutating `state.balance_height` /
    /// `state.last_gas_per_vote` and returning the datoshi to mint (or `None`), and
    /// (b) when the account votes, shifts that candidate's vote weight and the global
    /// voters-count by `amount`. The caller writes `state` back and mints the return.
    fn neo_on_balance_changing(
        &self,
        engine: &ApplicationEngine,
        snapshot: &DataCache,
        state: &mut NeoAccountStateView,
        amount: &BigInt,
    ) -> CoreResult<Option<BigInt>> {
        // DistributeGas: bonus on the OLD state, then advance the reward markers.
        let mut distribution = None;
        if let Some(block) = engine.persisting_block() {
            let end = block.index();
            let bonus = self.calculate_bonus(snapshot, state, end)?;
            state.balance_height = end;
            if let Some(vote_to) = &state.vote_to {
                state.last_gas_per_vote = self.voter_reward_per_committee(snapshot, vote_to);
            }
            if bonus != BigInt::from(0) {
                distribution = Some(bonus);
            }
        }
        // Vote-weight: a balance delta moves the voted candidate's weight + voters count.
        if *amount != BigInt::from(0) {
            if let Some(vote_to) = state.vote_to.clone() {
                let mut count = self.read_voters_count(snapshot);
                count += amount;
                self.write_voters_count(snapshot, &count);
                if let Some(item) = snapshot.get(&Self::candidate_key(&vote_to)) {
                    let (registered, mut votes) =
                        Self::decode_candidate_state(&item.value_bytes())?;
                    votes += amount;
                    self.check_candidate(snapshot, &vote_to, registered, &votes)?;
                }
            }
        }
        Ok(distribution)
    }

    /// C# `FungibleToken.PostTransferAsync` for NEO: emit `Transfer(from, to, amount)`
    /// and, when `to` is a deployed contract, queue its `onNEP17Payment` callback.
    fn neo_post_transfer(
        &self,
        engine: &mut ApplicationEngine,
        from: &UInt160,
        to: &UInt160,
        amount: &BigInt,
        data: &[u8],
    ) -> CoreResult<()> {
        engine
            .send_notification(
                NeoToken::script_hash(),
                "Transfer".to_string(),
                vec![
                    StackItem::from_byte_string(from.to_bytes()),
                    StackItem::from_byte_string(to.to_bytes()),
                    StackItem::from_int(amount.clone()),
                ],
            )
            .map_err(|e| CoreError::invalid_operation(format!("NeoToken::transfer notify: {e}")))?;
        if !crate::ContractManagement::is_contract(&engine.snapshot_cache(), to) {
            return Ok(());
        }
        let data_item = if data.is_empty() {
            StackItem::null()
        } else {
            BinarySerializer::deserialize(data, &ExecutionEngineLimits::default(), None)
                .map_err(|e| CoreError::deserialization(format!("NeoToken::transfer data: {e}")))?
        };
        engine.queue_contract_call_from_native(
            NeoToken::script_hash(),
            *to,
            "onNEP17Payment",
            vec![
                StackItem::from_byte_string(from.to_bytes()),
                StackItem::from_int(amount.clone()),
                data_item,
            ],
        );
        Ok(())
    }

    /// C# `FungibleToken.Transfer` specialised to NEO (`NeoAccountState`): witness the
    /// `from` account (with the calling-contract bypass), move the balance applying
    /// `OnBalanceChanging` on each side, then `PostTransfer` and mint the collected
    /// GAS distributions. Returns `false` (no fault) on a failed witness / missing
    /// source / insufficient balance, matching C#.
    fn neo_transfer_core(
        &self,
        engine: &mut ApplicationEngine,
        caller: UInt160,
        from: &UInt160,
        to: &UInt160,
        amount: &BigInt,
        data: &[u8],
    ) -> CoreResult<bool> {
        if *amount < BigInt::from(0) {
            return Err(CoreError::invalid_operation(
                "NeoToken::transfer: amount cannot be negative",
            ));
        }
        if caller != *from
            && !engine.check_witness(from).map_err(|e| {
                CoreError::invalid_operation(format!("NeoToken::transfer: witness: {e}"))
            })?
        {
            return Ok(false);
        }
        let snapshot = engine.snapshot_cache();
        let zero = BigInt::from(0);
        let mut distributions: Vec<(UInt160, BigInt)> = Vec::new();
        let from_state = self.read_account_state(&snapshot, from);

        if *amount == zero {
            if let Some(bytes) = from_state {
                let mut state = Self::decode_neo_account_state(&bytes)?;
                if let Some(d) =
                    self.neo_on_balance_changing(engine, &snapshot, &mut state, &zero)?
                {
                    distributions.push((*from, d));
                }
                snapshot.update(
                    Self::neo_account_key(from),
                    StorageItem::from_bytes(Self::encode_neo_account_state(&state)?),
                );
            }
        } else {
            let Some(bytes) = from_state else {
                return Ok(false);
            };
            let mut from_state = Self::decode_neo_account_state(&bytes)?;
            if from_state.balance < *amount {
                return Ok(false);
            }
            if from == to {
                if let Some(d) =
                    self.neo_on_balance_changing(engine, &snapshot, &mut from_state, &zero)?
                {
                    distributions.push((*from, d));
                }
                snapshot.update(
                    Self::neo_account_key(from),
                    StorageItem::from_bytes(Self::encode_neo_account_state(&from_state)?),
                );
            } else {
                let neg_amount = -amount;
                if let Some(d) =
                    self.neo_on_balance_changing(engine, &snapshot, &mut from_state, &neg_amount)?
                {
                    distributions.push((*from, d));
                }
                if from_state.balance == *amount {
                    snapshot.delete(&Self::neo_account_key(from));
                } else {
                    from_state.balance -= amount;
                    snapshot.update(
                        Self::neo_account_key(from),
                        StorageItem::from_bytes(Self::encode_neo_account_state(&from_state)?),
                    );
                }
                let mut to_state = match self.read_account_state(&snapshot, to) {
                    Some(bytes) => Self::decode_neo_account_state(&bytes)?,
                    None => NeoAccountStateView {
                        balance: BigInt::from(0),
                        balance_height: 0,
                        vote_to: None,
                        last_gas_per_vote: BigInt::from(0),
                    },
                };
                if let Some(d) =
                    self.neo_on_balance_changing(engine, &snapshot, &mut to_state, amount)?
                {
                    distributions.push((*to, d));
                }
                to_state.balance += amount;
                snapshot.update(
                    Self::neo_account_key(to),
                    StorageItem::from_bytes(Self::encode_neo_account_state(&to_state)?),
                );
            }
        }

        self.neo_post_transfer(engine, from, to, amount, data)?;
        for (account, datoshi) in distributions {
            crate::GasToken::new().gas_mint(engine, &account, &datoshi, true)?;
        }
        Ok(true)
    }

    /// C# `NeoToken.VoteInternal(engine, account, voteTo)`: the vote transition
    /// applied after the caller has authorized the voter — `_votersCount`
    /// bookkeeping, the GAS reward (`DistributeGas` + `GAS.Mint`), candidate
    /// vote-weight deltas, the `NeoAccountState.VoteTo` update, and the `Vote`
    /// notification. Returns `false` (no fault) when the account has no state, a
    /// zero balance, or the new candidate is missing/unregistered, matching C#.
    ///
    /// Exposed `pub(crate)` because C# `PolicyContract.BlockAccountInternal`
    /// (HF_Faun) clears a blocked account's vote by calling
    /// `NEO.VoteInternal(engine, account, null)` directly, bypassing the witness
    /// check performed by the public `vote` method.
    pub(crate) fn vote_internal(
        &self,
        engine: &mut ApplicationEngine,
        account: &UInt160,
        vote_to: Option<&ECPoint>,
    ) -> CoreResult<bool> {
        let vote_to: Option<ECPoint> = vote_to.cloned();
        let snapshot = engine.snapshot_cache();
        let Some(acct_bytes) = self.read_account_state(&snapshot, account) else {
            return Ok(false); // no account state
        };
        let mut acct = Self::decode_neo_account_state(&acct_bytes)?;
        if acct.balance == BigInt::from(0) {
            return Ok(false);
        }
        // The new candidate must exist and be registered.
        if let Some(new_pk) = &vote_to {
            match snapshot.get(&Self::candidate_key(new_pk)) {
                Some(item) => {
                    let (registered, _) = Self::decode_candidate_state(&item.value_bytes())?;
                    if !registered {
                        return Ok(false);
                    }
                }
                None => return Ok(false),
            }
        }
        let old_vote = acct.vote_to.clone();
        // _votersCount changes only when the account starts or stops voting.
        if old_vote.is_none() != vote_to.is_none() {
            let mut count = self.read_voters_count(&snapshot);
            if old_vote.is_none() {
                count += &acct.balance;
            } else {
                count -= &acct.balance;
            }
            self.write_voters_count(&snapshot, &count);
        }
        // DistributeGas: compute the bonus with the OLD state, then advance
        // BalanceHeight + LastGasPerVote (only when a persisting block exists).
        let mut gas_to_mint = BigInt::from(0);
        if let Some(block) = engine.persisting_block() {
            let end = block.index();
            let bonus = self.calculate_bonus(&snapshot, &acct, end)?;
            acct.balance_height = end;
            if let Some(old_pk) = &old_vote {
                acct.last_gas_per_vote = self.voter_reward_per_committee(&snapshot, old_pk);
            }
            if bonus != BigInt::from(0) {
                gas_to_mint = bonus;
            }
        }
        // Remove the account's weight from the previously-voted candidate.
        if let Some(old_pk) = &old_vote {
            if let Some(item) = snapshot.get(&Self::candidate_key(old_pk)) {
                let (registered, mut votes) = Self::decode_candidate_state(&item.value_bytes())?;
                votes -= &acct.balance;
                self.check_candidate(&snapshot, old_pk, registered, &votes)?;
            }
        }
        // Switching to a new (different) candidate resets the reward marker.
        if let Some(new_pk) = &vote_to {
            if Some(new_pk) != old_vote.as_ref() {
                acct.last_gas_per_vote = self.voter_reward_per_committee(&snapshot, new_pk);
            }
        }
        let from = old_vote.clone();
        acct.vote_to = vote_to.clone();
        // Add the account's weight to the new candidate (re-read so a vote
        // for the same candidate nets to zero), else clear the reward marker.
        if let Some(new_pk) = &vote_to {
            let item = snapshot.get(&Self::candidate_key(new_pk)).ok_or_else(|| {
                CoreError::invalid_operation("NeoToken::vote: candidate disappeared")
            })?;
            let (registered, mut votes) = Self::decode_candidate_state(&item.value_bytes())?;
            votes += &acct.balance;
            snapshot.update(
                Self::candidate_key(new_pk),
                StorageItem::from_bytes(Self::encode_candidate_state(registered, &votes)?),
            );
        } else {
            acct.last_gas_per_vote = BigInt::from(0);
        }
        snapshot.update(
            Self::neo_account_key(account),
            StorageItem::from_bytes(Self::encode_neo_account_state(&acct)?),
        );

        let to_item = |pk: &Option<ECPoint>| match pk {
            Some(p) => StackItem::from_byte_string(p.to_bytes()),
            None => StackItem::null(),
        };
        engine
            .send_notification(
                NeoToken::script_hash(),
                "Vote".to_string(),
                vec![
                    StackItem::from_byte_string(account.to_bytes()),
                    to_item(&from),
                    to_item(&vote_to),
                    StackItem::from_int(acct.balance.clone()),
                ],
            )
            .map_err(|e| CoreError::invalid_operation(format!("NeoToken::vote: notify: {e}")))?;
        if gas_to_mint > BigInt::from(0) {
            crate::GasToken::new().gas_mint(engine, account, &gas_to_mint, true)?;
        }
        Ok(true)
    }

    /// C# `GetSortedGasRecords(snapshot, end)`: the `Prefix_GasPerBlock` records with
    /// index ≤ `end`, descending by index.
    fn sorted_gas_records(&self, snapshot: &DataCache, end: u32) -> Vec<(u32, BigInt)> {
        let prefix = StorageKey::create(NeoToken::ID, PREFIX_GAS_PER_BLOCK);
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
    fn voter_reward_per_committee(&self, snapshot: &DataCache, pubkey: &ECPoint) -> BigInt {
        let key = StorageKey::create_with_bytes(
            NeoToken::ID,
            PREFIX_VOTER_REWARD_PER_COMMITTEE,
            &pubkey.to_bytes(),
        );
        snapshot
            .get(&key)
            .map(|item| BigInt::from_signed_bytes_le(&item.value_bytes()))
            .unwrap_or_else(|| BigInt::from(0))
    }

    /// C# `NeoToken.CalculateBonus`: the unclaimed GAS for an account between
    /// `BalanceHeight` and `end` — the NEO-holder reward (`balance * Σ gasPerBlock *
    /// 10 / 100 / TotalAmount`) plus the vote reward (`balance * (latestGasPerVote -
    /// lastGasPerVote) / VoteFactor`).
    fn calculate_bonus(
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

    /// Reads the cached committee from `Prefix_Committee` (C#
    /// `GetCommitteeFromCache`) as `(pubkey, votes)` pairs in stored order. The
    /// value is a `BinarySerializer` array whose elements are `Struct[pubkey(33-byte
    /// compressed), votes]` (C# `CachedCommittee.ElementToStackItem`). Errors when
    /// the cache has never been initialized, matching the C# indexer/`GetAndChange`
    /// null deref.
    fn read_committee_with_votes(
        &self,
        snapshot: &DataCache,
    ) -> CoreResult<Vec<(ECPoint, BigInt)>> {
        let key = StorageKey::create(NeoToken::ID, PREFIX_COMMITTEE);
        let item = snapshot.get(&key).ok_or_else(|| {
            CoreError::invalid_operation("NeoToken committee cache is not initialized")
        })?;
        let limits = ExecutionEngineLimits::default();
        let decoded = BinarySerializer::deserialize_stack_value_with_limits(
            &item.value_bytes(),
            limits.max_item_size as usize,
            limits.max_stack_size as usize,
        )
        .map_err(|e| CoreError::deserialization(format!("committee cache: {e}")))?;
        Ok(CachedCommittee::from_stack_value(decoded)?.into_members())
    }

    /// Reads only the cached committee public keys, in stored order.
    fn read_committee_points(&self, snapshot: &DataCache) -> CoreResult<Vec<ECPoint>> {
        Ok(self
            .read_committee_with_votes(snapshot)?
            .into_iter()
            .map(|(point, _)| point)
            .collect())
    }

    /// Serializes `(pubkey, votes)` committee members as the `Prefix_Committee`
    /// storage value — an Array of `Struct[pubkey, votes]` (C#
    /// `CachedCommittee.ToStackItem`), the byte-exact write counterpart of
    /// [`read_committee_with_votes`].
    fn encode_committee(members: &[(ECPoint, BigInt)]) -> CoreResult<Vec<u8>> {
        let array = CachedCommittee::new(members.to_vec()).to_stack_value();
        BinarySerializer::serialize_stack_value_default(&array)
            .map_err(|e| CoreError::invalid_operation(format!("encode committee cache: {e}")))
    }

    /// C# `NeoToken.ShouldRefreshCommittee(height, committeeMembersCount)`:
    /// the committee is recounted on every block whose index is a multiple of the
    /// committee size. `committee_count` must be non-zero (validated by callers,
    /// like the C# division-by-zero).
    fn should_refresh_committee(height: u32, committee_count: usize) -> bool {
        height % (committee_count as u32) == 0
    }

    /// C# `NeoToken.ComputeCommitteeMembers(snapshot, settings)`: the next committee
    /// as `(pubkey, votes)` pairs. When the voter turnout reaches
    /// `EffectiveVoterTurnout` (0.2) AND at least `CommitteeMembersCount` registered
    /// candidates exist, the committee is the top `CommitteeMembersCount` candidates
    /// ordered by (votes descending, pubkey ascending); otherwise it falls back to
    /// the standby committee, each zipped with its registered-candidate votes (zero
    /// when not a candidate).
    ///
    /// The C# turnout test is `votersCount / (decimal)TotalAmount < 0.2M`; both
    /// operands are integers and `TotalAmount = 1e8`, so the decimal quotient is
    /// exact and the comparison is equivalent to the integer-safe
    /// `votersCount * 5 < TotalAmount`.
    fn compute_committee_members(
        &self,
        snapshot: &DataCache,
        settings: &neo_config::ProtocolSettings,
    ) -> CoreResult<Vec<(ECPoint, BigInt)>> {
        let voters_count = self.read_voters_count(snapshot);
        let candidates = self.read_registered_candidates(snapshot)?;
        let committee_count = settings.committee_members_count();
        if committee_count == 0 {
            return Err(CoreError::invalid_operation(
                "ComputeCommitteeMembers requires a non-empty standby committee",
            ));
        }
        let turnout_reached = voters_count * 5 >= BigInt::from(NEO_TOTAL_AMOUNT);
        if !turnout_reached || candidates.len() < committee_count {
            return Ok(settings
                .standby_committee
                .iter()
                .map(|point| {
                    let votes = candidates
                        .iter()
                        .find(|(candidate, _)| candidate == point)
                        .map(|(_, votes)| votes.clone())
                        .unwrap_or_else(|| BigInt::from(0));
                    (point.clone(), votes)
                })
                .collect());
        }
        let mut sorted = candidates;
        // OrderByDescending(votes).ThenBy(pubkey): votes descending, pubkey ascending.
        sorted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        sorted.truncate(committee_count);
        Ok(sorted)
    }

    /// C# `Contract.GetBFTAddress(pubkeys)`: the script hash of the
    /// `m`-of-`n` multisig over `pubkeys` with the BFT threshold
    /// `m = n - (n - 1) / 3`. (Distinct from the committee address, whose
    /// threshold is the simple majority `n - (n - 1) / 2`.) `pub(crate)` so
    /// `GasToken::initialize` can mint the initial GAS distribution to the
    /// standby-validator BFT address (C# GasToken.cs:33).
    pub(crate) fn bft_address(pubkeys: &[ECPoint]) -> CoreResult<UInt160> {
        if pubkeys.is_empty() {
            return Err(CoreError::invalid_operation(
                "BFT address requires at least one key",
            ));
        }
        let m = pubkeys.len() - (pubkeys.len() - 1) / 3;
        let script =
            neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(
                m, pubkeys,
            )
                .map_err(|e| CoreError::invalid_operation(format!("BFT multisig script: {e}")))?;
        Ok(UInt160::from_script(&script))
    }

    /// C# `FungibleToken.Mint` specialised to NEO (`NeoAccountState` +
    /// `OnBalanceChanging` + the GAS-distribution drain of NEO's
    /// `PostTransferAsync`): credit `amount` NEO to `account`, raise the stored
    /// total supply, emit `Transfer(null, account, amount)`, queue the recipient's
    /// `onNEP17Payment` when `call_on_payment` and the recipient is a deployed
    /// contract, then mint any GAS distribution collected by `OnBalanceChanging`.
    /// A zero amount is a no-op; a negative amount faults.
    fn neo_mint(
        &self,
        engine: &mut ApplicationEngine,
        account: &UInt160,
        amount: &BigInt,
        call_on_payment: bool,
    ) -> CoreResult<()> {
        let zero = BigInt::from(0);
        if *amount < zero {
            return Err(CoreError::invalid_operation(
                "NeoToken::mint: amount cannot be negative",
            ));
        }
        if *amount == zero {
            return Ok(());
        }
        let snapshot = engine.snapshot_cache();
        let mut state = match self.read_account_state(&snapshot, account) {
            Some(bytes) => Self::decode_neo_account_state(&bytes)?,
            None => NeoAccountStateView {
                balance: BigInt::from(0),
                balance_height: 0,
                vote_to: None,
                last_gas_per_vote: BigInt::from(0),
            },
        };
        let mut distributions: Vec<(UInt160, BigInt)> = Vec::new();
        if let Some(datoshi) =
            self.neo_on_balance_changing(engine, &snapshot, &mut state, amount)?
        {
            distributions.push((*account, datoshi));
        }
        state.balance += amount;
        snapshot.update(
            Self::neo_account_key(account),
            StorageItem::from_bytes(Self::encode_neo_account_state(&state)?),
        );
        let supply_key = StorageKey::create(NeoToken::ID, crate::NEP17_PREFIX_TOTAL_SUPPLY);
        let supply = snapshot
            .get(&supply_key)
            .map(|item| BigInt::from_signed_bytes_le(&item.value_bytes()))
            .unwrap_or_else(|| BigInt::from(0))
            + amount;
        snapshot.update(
            supply_key,
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&supply)),
        );
        // PostTransfer with from = null (C# PostTransferAsync(null, account, …)).
        engine
            .send_notification(
                NeoToken::script_hash(),
                "Transfer".to_string(),
                vec![
                    StackItem::null(),
                    StackItem::from_byte_string(account.to_bytes()),
                    StackItem::from_int(amount.clone()),
                ],
            )
            .map_err(|e| CoreError::invalid_operation(format!("NeoToken::mint notify: {e}")))?;
        if call_on_payment
            && crate::ContractManagement::is_contract(&engine.snapshot_cache(), account)
        {
            engine.queue_contract_call_from_native(
                NeoToken::script_hash(),
                *account,
                "onNEP17Payment",
                vec![
                    StackItem::null(),
                    StackItem::from_int(amount.clone()),
                    StackItem::null(),
                ],
            );
        }
        for (target, datoshi) in distributions {
            crate::GasToken::new().gas_mint(engine, &target, &datoshi, call_on_payment)?;
        }
        Ok(())
    }

    /// C# `GetCommittee` = committee public keys sorted ascending (`OrderBy(p => p)`).
    fn committee_sorted(&self, snapshot: &DataCache) -> CoreResult<Vec<ECPoint>> {
        let mut points = self.read_committee_points(snapshot)?;
        points.sort();
        Ok(points)
    }

    /// Decodes a `CandidateState` storage value — a `Struct[Registered(bool), Votes]`
    /// — into `(registered, votes)`.
    fn decode_candidate_state(value: &[u8]) -> CoreResult<(bool, BigInt)> {
        let limits = ExecutionEngineLimits::default();
        let decoded = BinarySerializer::deserialize_stack_value_with_limits(
            value,
            limits.max_item_size as usize,
            limits.max_stack_size as usize,
        )
        .map_err(|e| CoreError::deserialization(format!("candidate state: {e}")))?;
        let state = CandidateState::from_stack_value(decoded)?;
        Ok((state.registered, state.votes))
    }

    /// Encodes a `CandidateState` storage value — a `Struct[Registered(bool),
    /// Votes]` — the write counterpart of [`decode_candidate_state`].
    fn encode_candidate_state(registered: bool, votes: &BigInt) -> CoreResult<Vec<u8>> {
        let item = CandidateState::new(registered, votes.clone()).to_stack_value();
        BinarySerializer::serialize_stack_value_default(&item)
            .map_err(|e| CoreError::invalid_operation(format!("encode candidate state: {e}")))
    }

    /// The `Prefix_Candidate` storage key for `pubkey` (`prefix ++ 33-byte pubkey`).
    fn candidate_key(pubkey: &ECPoint) -> StorageKey {
        StorageKey::create_with_bytes(NeoToken::ID, PREFIX_CANDIDATE, &pubkey.to_bytes())
    }

    /// The `Prefix_Account` storage key for `account` (NEP-17 account prefix).
    fn neo_account_key(account: &UInt160) -> StorageKey {
        StorageKey::create_with_uint160(NeoToken::ID, crate::NEP17_PREFIX_ACCOUNT, account)
    }

    /// C# `GetCandidatesInternal`: scan `Prefix_Candidate` (key = prefix ++ 33-byte
    /// pubkey; value = CandidateState `Struct[Registered(bool), Votes]`), returning
    /// the raw `(key, value)` storage entries of the registered candidates in
    /// storage-scan order, excluding candidates whose signature-contract address is
    /// blocked by `PolicyContract` (`!Policy.IsBlocked(snapshot, sigScriptHash)`).
    fn registered_candidate_entries(
        &self,
        snapshot: &DataCache,
    ) -> CoreResult<Vec<(StorageKey, StorageItem)>> {
        let prefix = StorageKey::create(NeoToken::ID, PREFIX_CANDIDATE);
        let mut out = Vec::new();
        for (key, item) in snapshot.find(Some(&prefix), SeekDirection::Forward) {
            let key_bytes = key.key();
            if key_bytes.len() < 34 {
                continue;
            }
            let Ok(pubkey) = ECPoint::from_bytes(&key_bytes[1..34]) else {
                continue;
            };
            let (registered, _votes) = Self::decode_candidate_state(&item.value_bytes())?;
            if registered {
                let account =
                    UInt160::from_script(&Contract::create_signature_redeem_script(pubkey));
                if snapshot
                    .get(&crate::PolicyContract::blocked_account_key(&account))
                    .is_none()
                {
                    out.push((key, item));
                }
            }
        }
        Ok(out)
    }

    /// [`registered_candidate_entries`] projected to `(pubkey, votes)` pairs — the
    /// shape consumed by `getCandidates` and the committee recompute.
    fn read_registered_candidates(
        &self,
        snapshot: &DataCache,
    ) -> CoreResult<Vec<(ECPoint, BigInt)>> {
        self.registered_candidate_entries(snapshot)?
            .into_iter()
            .map(|(key, item)| {
                let pubkey = ECPoint::from_bytes(&key.key()[1..34])
                    .map_err(|e| CoreError::invalid_data(format!("candidate key: {e}")))?;
                let (_registered, votes) = Self::decode_candidate_state(&item.value_bytes())?;
                Ok((pubkey, votes))
            })
            .collect()
    }

    /// C# `RegisterInternal` (NeoToken.cs:411-423), shared by `registerCandidate`
    /// and the Echidna `onNEP17Payment` GAS-payment path: requires a witness from
    /// the candidate's signature-contract account (returning `false` without one),
    /// creates/flips the CandidateState to Registered, and emits
    /// `CandidateStateChanged` for a fresh registration (post-Echidna, matching the
    /// V1 `registerCandidate` registration's AllowNotify). `method` labels errors
    /// with the invoking ABI method.
    fn register_internal(
        &self,
        engine: &mut ApplicationEngine,
        pubkey: &ECPoint,
        method: &str,
    ) -> CoreResult<bool> {
        let account =
            UInt160::from_script(&Contract::create_signature_redeem_script(pubkey.clone()));
        let authorized = engine.check_witness_hash(&account).map_err(|e| {
            CoreError::invalid_operation(format!("NeoToken::{method}: witness: {e}"))
        })?;
        if !authorized {
            return Ok(false);
        }
        let snapshot = engine.snapshot_cache();
        let key = Self::candidate_key(pubkey);
        let (registered, votes) = match snapshot.get(&key) {
            Some(item) => Self::decode_candidate_state(&item.value_bytes())?,
            None => (false, BigInt::from(0)),
        };
        if registered {
            return Ok(true);
        }
        snapshot.update(
            key,
            StorageItem::from_bytes(Self::encode_candidate_state(true, &votes)?),
        );
        if engine.is_hardfork_enabled(Hardfork::HfEchidna) {
            engine
                .send_notification(
                    NeoToken::script_hash(),
                    "CandidateStateChanged".to_string(),
                    vec![
                        StackItem::from_byte_string(pubkey.to_bytes()),
                        StackItem::from_bool(true),
                        StackItem::from_int(votes),
                    ],
                )
                .map_err(|e| {
                    CoreError::invalid_operation(format!("NeoToken::{method}: notify: {e}"))
                })?;
        }
        Ok(true)
    }

    /// C# `GetCandidateVote`: the votes for `pubkey` if it is a registered candidate,
    /// else -1 (also -1 when there is no candidate entry at all).
    fn candidate_vote(&self, snapshot: &DataCache, pubkey: &ECPoint) -> CoreResult<BigInt> {
        match snapshot.get(&Self::candidate_key(pubkey)) {
            Some(item) => {
                let (registered, votes) = Self::decode_candidate_state(&item.value_bytes())?;
                Ok(if registered { votes } else { BigInt::from(-1) })
            }
            None => Ok(BigInt::from(-1)),
        }
    }

    /// Marshals `(pubkey, votes)` candidate pairs as an Array of `Struct[pubkey,
    /// votes]` (C# `(ECPoint, BigInteger)[]` return shape).
    fn candidates_to_array_bytes(candidates: &[(ECPoint, BigInt)]) -> CoreResult<Vec<u8>> {
        let array = StackValue::Array(
            0,
            candidates
                .iter()
                .map(|(pk, votes)| {
                    StackValue::Struct(
                        0,
                        vec![
                            StackValue::ByteString(pk.to_bytes()),
                            StackValue::BigInteger(votes.to_signed_bytes_le()),
                        ],
                    )
                })
                .collect::<Vec<_>>(),
        );
        BinarySerializer::serialize_stack_value_default(&array)
            .map_err(|e| CoreError::invalid_operation(format!("getCandidates: {e}")))
    }

    /// Serializes EC points as an Array of compressed (33-byte) byte strings — the
    /// return shape shared by `getCommittee` / `getNextBlockValidators`.
    fn points_to_stack_value<'a, I>(points: I) -> StackValue
    where
        I: IntoIterator<Item = &'a ECPoint>,
    {
        StackValue::Array(
            0,
            points
                .into_iter()
                .map(|p| StackValue::ByteString(p.to_bytes()))
                .collect::<Vec<_>>(),
        )
    }

    fn points_to_array_bytes(points: &[ECPoint]) -> CoreResult<Vec<u8>> {
        let array = Self::points_to_stack_value(points.iter());
        BinarySerializer::serialize_stack_value_default(&array)
            .map_err(|e| CoreError::invalid_operation(format!("NeoToken point array: {e}")))
    }

    fn points_to_stack_item<'a, I>(points: I) -> CoreResult<StackItem>
    where
        I: IntoIterator<Item = &'a ECPoint>,
    {
        StackItem::try_from(Self::points_to_stack_value(points))
            .map_err(|e| CoreError::invalid_operation(format!("NeoToken point array: {e}")))
    }

    /// The committee multisig threshold `m = n - (n - 1) / 2` (committee majority,
    /// matching C# `GetCommitteeAddress`). `n` must be non-zero.
    fn committee_threshold(n: usize) -> usize {
        n - (n - 1) / 2
    }

    /// C# `GetCommitteeAddress` = script hash of the `m`-of-`n` multisig over the
    /// committee public keys, where `m = n - (n - 1) / 2`. The multisig builder sorts
    /// the keys ascending exactly as C# `Contract.CreateMultiSigRedeemScript` does.
    fn compute_committee_address(&self, snapshot: &DataCache) -> CoreResult<UInt160> {
        let points = self.read_committee_points(snapshot)?;
        if points.is_empty() {
            return Err(CoreError::invalid_operation("committee is empty"));
        }
        let m = Self::committee_threshold(points.len());
        let script =
            neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(
                m, &points,
            )
                .map_err(|e| CoreError::invalid_operation(format!("committee multisig script: {e}")))?;
        Ok(UInt160::from_script(&script))
    }

    /// C# `GetAccountState`: the stored `NeoAccountState` struct bytes under
    /// `Prefix_Account ++ account`, or `None` when the account has no entry. The
    /// stored value is already the BinarySerializer-encoded struct (balance,
    /// balanceHeight, voteTo, lastGasPerVote), which is exactly the Array/Struct
    /// return shape — so it is returned as-is (the same pattern as
    /// `getDesignatedByRole` / `getContract`).
    fn read_account_state(&self, snapshot: &DataCache, account: &UInt160) -> Option<Vec<u8>> {
        let key = Self::neo_account_key(account);
        snapshot
            .get(&key)
            .map(|item| item.value_bytes().into_owned())
    }
}

/// Decoded view of a `NeoAccountState` (`Struct[Balance, BalanceHeight, VoteTo,
/// LastGasPerVote]`, C# `NeoAccountState.FromStackItem`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct NeoAccountStateView {
    balance: BigInt,
    balance_height: u32,
    vote_to: Option<ECPoint>,
    last_gas_per_vote: BigInt,
}

impl NeoAccountStateView {
    fn to_stack_value(&self) -> StackValue {
        let mut items = match crate::AccountState::new(self.balance.clone()).to_stack_value() {
            StackValue::Struct(0, items) => items,
            _ => unreachable!("AccountState always projects to Struct"),
        };
        items.push(StackValue::Integer(i64::from(self.balance_height)));
        items.push(match &self.vote_to {
            Some(pubkey) => StackValue::ByteString(pubkey.to_bytes()),
            None => StackValue::Null,
        });
        items.push(StackValue::BigInteger(
            self.last_gas_per_vote.to_signed_bytes_le(),
        ));
        StackValue::Struct(0, items)
    }

    fn from_stack_value(stack_value: StackValue) -> CoreResult<Self> {
        let StackValue::Struct(0, items) = stack_value else {
            return Err(CoreError::invalid_data("neo account state is not a struct"));
        };
        if items.len() < 4 {
            return Err(CoreError::invalid_data(
                "neo account state must have at least 4 fields",
            ));
        }

        let balance =
            crate::AccountState::from_stack_value(StackValue::Struct(0, vec![items[0].clone()]))?
                .balance;
        let balance_height = neo_vm_rs::stack_value_as_u32(&items[1]).ok_or_else(|| {
            CoreError::invalid_data("account balanceHeight: expected uint32 integer")
        })?;
        let vote_to = if matches!(items[2], StackValue::Null) {
            None
        } else {
            let bytes = items[2].to_byte_string_bytes().ok_or_else(|| {
                CoreError::invalid_data("account voteTo: expected byte-compatible value")
            })?;
            Some(
                ECPoint::from_bytes(&bytes)
                    .map_err(|e| CoreError::invalid_data(format!("account voteTo point: {e}")))?,
            )
        };
        let last_gas_per_vote = neo_vm_rs::stack_value_as_bigint(&items[3])
            .map_err(|e| CoreError::invalid_data(format!("account lastGasPerVote: {e}")))?;
        Ok(Self {
            balance,
            balance_height,
            vote_to,
            last_gas_per_vote,
        })
    }
}

neo_vm::impl_interoperable_via_stack_value!(NeoAccountStateView);

#[derive(Debug, Clone, PartialEq, Eq)]
struct CandidateState {
    registered: bool,
    votes: BigInt,
}

impl CandidateState {
    fn new(registered: bool, votes: BigInt) -> Self {
        Self { registered, votes }
    }

    fn to_stack_value(&self) -> StackValue {
        StackValue::Struct(
            0,
            vec![
                StackValue::Boolean(self.registered),
                StackValue::BigInteger(self.votes.to_signed_bytes_le()),
            ],
        )
    }

    fn from_stack_value(stack_value: StackValue) -> CoreResult<Self> {
        let StackValue::Struct(0, items) = stack_value else {
            return Err(CoreError::invalid_data("candidate state is not a struct"));
        };
        if items.len() < 2 {
            return Err(CoreError::invalid_data(
                "candidate state must have at least 2 fields",
            ));
        }

        let registered = neo_vm_rs::stack_value_as_bool(&items[0]).ok_or_else(|| {
            CoreError::invalid_data("candidate registered: expected boolean-compatible value")
        })?;
        let votes = neo_vm_rs::stack_value_as_bigint(&items[1])
            .map_err(|e| CoreError::invalid_data(format!("candidate votes: {e}")))?;
        Ok(Self { registered, votes })
    }
}

neo_vm::impl_interoperable_via_stack_value!(CandidateState);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CachedCommittee {
    members: Vec<(ECPoint, BigInt)>,
}

impl CachedCommittee {
    pub(crate) fn new(members: Vec<(ECPoint, BigInt)>) -> Self {
        Self { members }
    }

    pub(crate) fn into_members(self) -> Vec<(ECPoint, BigInt)> {
        self.members
    }

    pub(crate) fn to_stack_value(&self) -> StackValue {
        StackValue::Array(
            0,
            self.members
                .iter()
                .map(|(point, votes)| {
                    StackValue::Struct(
                        0,
                        vec![
                            StackValue::ByteString(point.to_bytes()),
                            StackValue::BigInteger(votes.to_signed_bytes_le()),
                        ],
                    )
                })
                .collect(),
        )
    }

    pub(crate) fn from_stack_value(stack_value: StackValue) -> CoreResult<Self> {
        let StackValue::Array(0, array) = stack_value else {
            return Err(CoreError::invalid_data("committee cache is not an array"));
        };
        let mut members = Vec::with_capacity(array.len());
        for element in array {
            members.push(Self::member_from_stack_value(element)?);
        }
        Ok(Self { members })
    }

    fn member_from_stack_value(stack_value: StackValue) -> CoreResult<(ECPoint, BigInt)> {
        let StackValue::Struct(0, items) = stack_value else {
            return Err(CoreError::invalid_data("committee element is not a struct"));
        };
        if items.len() < 2 {
            return Err(CoreError::invalid_data(
                "committee element must have at least 2 fields",
            ));
        }
        let bytes = items[0]
            .to_byte_string_bytes()
            .ok_or_else(|| CoreError::invalid_data("committee pubkey: not bytes"))?;
        let point = ECPoint::from_bytes(&bytes)
            .map_err(|e| CoreError::invalid_data(format!("committee EC point: {e}")))?;
        let votes = neo_vm_rs::stack_value_as_bigint(&items[1])
            .map_err(|e| CoreError::invalid_data(format!("committee votes: {e}")))?;
        Ok((point, votes))
    }
}

neo_vm::impl_interoperable_via_stack_value!(CachedCommittee);

static NEO_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    let read_states = CallFlags::READ_STATES.bits();
    let int = ContractParameterType::Integer;
    vec![
        // NEP-17 metadata: `[ContractMethod]` with no CpuFee -> fee 0, no flags.
        NativeMethod::new(
            "symbol".into(),
            0,
            true,
            0,
            vec![],
            ContractParameterType::String,
        ),
        NativeMethod::new("decimals".into(), 0, true, 0, vec![], int),
        // NEP-17 state reads: CpuFee 1<<15, RequiredCallFlags ReadStates.
        NativeMethod::new(
            "totalSupply".into(),
            1 << 15,
            true,
            read_states,
            vec![],
            int,
        ),
        NativeMethod::new(
            "balanceOf".into(),
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::Hash160],
            int,
        )
        .with_parameter_names(["account"]),
        // NEP-17 transfer(from, to, amount, data) -> Boolean (CpuFee 1<<17,
        // States|AllowCall|AllowNotify; NEO governance runs in OnBalanceChanging).
        NativeMethod::new(
            "transfer".into(),
            1 << 17,
            false,
            (CallFlags::STATES | CallFlags::ALLOW_CALL | CallFlags::ALLOW_NOTIFY).bits(),
            vec![
                ContractParameterType::Hash160,
                ContractParameterType::Hash160,
                ContractParameterType::Integer,
                ContractParameterType::Any,
            ],
            ContractParameterType::Boolean,
        )
        .with_storage_fee(50)
        .with_parameter_names(["from", "to", "amount", "data"]),
        // Governance reads.
        NativeMethod::new(
            "getGasPerBlock".into(),
            1 << 15,
            true,
            read_states,
            vec![],
            int,
        ),
        NativeMethod::new(
            "getRegisterPrice".into(),
            1 << 15,
            true,
            read_states,
            vec![],
            int,
        ),
        // Committee reads (CpuFee 1<<16 in C#).
        NativeMethod::new(
            "getCommittee".into(),
            1 << 16,
            true,
            read_states,
            vec![],
            ContractParameterType::Array,
        ),
        NativeMethod::new(
            "getCommitteeAddress".into(),
            1 << 16,
            true,
            read_states,
            vec![],
            ContractParameterType::Hash160,
        )
        .with_active_in(Hardfork::HfCockatrice),
        // getAccountState(account) -> NeoAccountState struct (Array) or null.
        NativeMethod::new(
            "getAccountState".into(),
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::Hash160],
            ContractParameterType::Array,
        )
        .with_parameter_names(["account"]),
        // unclaimedGas(account, end) -> Integer (CpuFee 1<<17, ReadStates).
        NativeMethod::new(
            "unclaimedGas".into(),
            1 << 17,
            true,
            read_states,
            vec![ContractParameterType::Hash160, int],
            int,
        )
        .with_parameter_names(["account", "end"]),
        // getNextBlockValidators -> ECPoint[] (Array), CpuFee 1<<16 in C#.
        NativeMethod::new(
            "getNextBlockValidators".into(),
            1 << 16,
            true,
            read_states,
            vec![],
            ContractParameterType::Array,
        ),
        // getCandidates -> (ECPoint, BigInteger)[] (Array of Structs), CpuFee 1<<22.
        NativeMethod::new(
            "getCandidates".into(),
            1 << 22,
            true,
            read_states,
            vec![],
            ContractParameterType::Array,
        ),
        // getAllCandidates -> iterator over the registered candidates
        // (InteropInterface), CpuFee 1<<22, ReadStates (NeoToken.cs:537).
        NativeMethod::new(
            "getAllCandidates".into(),
            1 << 22,
            true,
            read_states,
            vec![],
            ContractParameterType::InteropInterface,
        ),
        // getCandidateVote(pubKey) -> votes, or -1 if not a registered
        // candidate. (C# parameter is `ECPoint pubKey` — capital K, unlike
        // registerCandidate's `pubkey`.)
        NativeMethod::new(
            "getCandidateVote".into(),
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::PublicKey],
            int,
        )
        .with_parameter_names(["pubKey"]),
        // Governance writers (committee-gated, States, Void; C# CpuFee 1<<15).
        NativeMethod::new(
            "setRegisterPrice".into(),
            1 << 15,
            false,
            CallFlags::STATES.bits(),
            vec![ContractParameterType::Integer],
            ContractParameterType::Void,
        )
        .with_parameter_names(["registerPrice"]),
        NativeMethod::new(
            "setGasPerBlock".into(),
            1 << 15,
            false,
            CallFlags::STATES.bits(),
            vec![ContractParameterType::Integer],
            ContractParameterType::Void,
        )
        .with_parameter_names(["gasPerBlock"]),
        // Candidate registration (Echidna V1: States|AllowNotify). registerCandidate
        // has no manifest CpuFee (it charges GetRegisterPrice dynamically);
        // unregisterCandidate is CpuFee 1<<16. Both return Boolean.
        // registerCandidate / unregisterCandidate / vote are each a dual
        // registration (C# NeoToken.cs:397/431/456): V0 is genesis-active with
        // RequiredCallFlags=States and DeprecatedIn=HF_Echidna; V1 is
        // ActiveIn=HF_Echidna and adds AllowNotify (the candidate-state-change
        // notification). Exactly one is active at any height.
        NativeMethod::new(
            "registerCandidate".into(),
            0,
            false,
            CallFlags::STATES.bits(),
            vec![ContractParameterType::PublicKey],
            ContractParameterType::Boolean,
        )
        .with_parameter_names(["pubkey"])
        .with_deprecated_in(Hardfork::HfEchidna),
        NativeMethod::new(
            "registerCandidate".into(),
            0,
            false,
            CallFlags::STATES.bits() | CallFlags::ALLOW_NOTIFY.bits(),
            vec![ContractParameterType::PublicKey],
            ContractParameterType::Boolean,
        )
        .with_parameter_names(["pubkey"])
        .with_active_in(Hardfork::HfEchidna),
        NativeMethod::new(
            "unregisterCandidate".into(),
            1 << 16,
            false,
            CallFlags::STATES.bits(),
            vec![ContractParameterType::PublicKey],
            ContractParameterType::Boolean,
        )
        .with_parameter_names(["pubkey"])
        .with_deprecated_in(Hardfork::HfEchidna),
        NativeMethod::new(
            "unregisterCandidate".into(),
            1 << 16,
            false,
            CallFlags::STATES.bits() | CallFlags::ALLOW_NOTIFY.bits(),
            vec![ContractParameterType::PublicKey],
            ContractParameterType::Boolean,
        )
        .with_parameter_names(["pubkey"])
        .with_active_in(Hardfork::HfEchidna),
        // vote(account, voteTo?) -> Boolean. voteTo is a nullable PublicKey
        // (null = clear the vote). V0 States / V1 States|AllowNotify at Echidna.
        NativeMethod::new(
            "vote".into(),
            1 << 16,
            false,
            CallFlags::STATES.bits(),
            vec![
                ContractParameterType::Hash160,
                ContractParameterType::PublicKey,
            ],
            ContractParameterType::Boolean,
        )
        .with_parameter_names(["account", "voteTo"])
        .with_deprecated_in(Hardfork::HfEchidna),
        NativeMethod::new(
            "vote".into(),
            1 << 16,
            false,
            CallFlags::STATES.bits() | CallFlags::ALLOW_NOTIFY.bits(),
            vec![
                ContractParameterType::Hash160,
                ContractParameterType::PublicKey,
            ],
            ContractParameterType::Boolean,
        )
        .with_parameter_names(["account", "voteTo"])
        .with_active_in(Hardfork::HfEchidna),
        // onNEP17Payment(from, amount, data) -> Void: candidate registration
        // by paying the register price in GAS to the NEO contract. C#
        // `[ContractMethod(Hardfork.HF_Echidna, RequiredCallFlags =
        // CallFlags.States | CallFlags.AllowNotify)]` with no CpuFee
        // (NeoToken.cs:374).
        NativeMethod::new(
            "onNEP17Payment".into(),
            0,
            false,
            CallFlags::STATES.bits() | CallFlags::ALLOW_NOTIFY.bits(),
            vec![
                ContractParameterType::Hash160,
                int,
                ContractParameterType::Any,
            ],
            ContractParameterType::Void,
        )
        .with_parameter_names(["from", "amount", "data"])
        .with_active_in(Hardfork::HfEchidna),
    ]
});

/// NEO's `[ContractEvent]` declarations (NeoToken.cs:63-74) plus the inherited
/// `FungibleToken.Transfer` at order 0. C# concatenates the contract
/// constructor's attributes with the base type's and sorts by order, so the
/// manifest lists Transfer, CandidateStateChanged, Vote, CommitteeChanged.
static NEO_EVENTS: LazyLock<Vec<NativeEvent>> = LazyLock::new(|| {
    vec![
        crate::fungible_token_transfer_event(),
        NativeEvent::new(
            1,
            "CandidateStateChanged",
            &[
                ("pubkey", ContractParameterType::PublicKey),
                ("registered", ContractParameterType::Boolean),
                ("votes", ContractParameterType::Integer),
            ],
        ),
        NativeEvent::new(
            2,
            "Vote",
            &[
                ("account", ContractParameterType::Hash160),
                ("from", ContractParameterType::PublicKey),
                ("to", ContractParameterType::PublicKey),
                ("amount", ContractParameterType::Integer),
            ],
        ),
        NativeEvent::new(
            3,
            "CommitteeChanged",
            &[
                ("old", ContractParameterType::Array),
                ("new", ContractParameterType::Array),
            ],
        )
        .with_active_in(Hardfork::HfCockatrice),
    ]
});

impl NativeContract for NeoToken {
    fn id(&self) -> i32 {
        Self::ID
    }

    fn hash(&self) -> UInt160 {
        Self::script_hash()
    }

    fn name(&self) -> &str {
        Self::NAME
    }

    fn methods(&self) -> &[NativeMethod] {
        &NEO_METHODS
    }

    fn event_descriptors(&self) -> &[NativeEvent] {
        &NEO_EVENTS
    }

    /// C# `NeoToken._usedHardforks` contains `HF_Echidna` (via the
    /// Echidna-gated `[ContractMethod]` registrations, NeoToken.cs:374-457),
    /// so `IsInitializeBlock` refreshes NEO's stored manifest at the Echidna
    /// boundary — where `OnManifestCompose` adds NEP-27. The Rust table's
    /// `onNEP17Payment` now carries that gate, but the single-entry
    /// registerCandidate/unregisterCandidate/vote registrations (C# dual
    /// V0/V1 attributes) do not, so the explicit activation stays declared
    /// here too (`used_hardforks` dedupes).
    fn activations(&self) -> Vec<Hardfork> {
        vec![Hardfork::HfEchidna]
    }

    /// C# `NeoToken.OnManifestCompose` (NeoToken.cs:112-122): NEO declares
    /// NEP-27 in addition to NEP-17 once HF_Echidna is enabled at the height.
    fn supported_standards(&self, settings: &ProtocolSettings, block_height: u32) -> Vec<String> {
        if settings.is_hardfork_enabled(Hardfork::HfEchidna, block_height) {
            vec!["NEP-17".to_string(), "NEP-27".to_string()]
        } else {
            vec!["NEP-17".to_string()]
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    /// C# `NEO.GetCommitteeAddress`, exposed through the native-contract seam so
    /// the engine's `check_committee_witness` can verify committee-gated writers
    /// without depending on `neo-native-contracts`.
    fn committee_address(&self, snapshot: &DataCache) -> CoreResult<Option<UInt160>> {
        Ok(Some(self.compute_committee_address(snapshot)?))
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
            StorageKey::create(Self::ID, PREFIX_COMMITTEE),
            StorageItem::from_bytes(Self::encode_committee(&members)?),
        );
        // C# `new StorageItem(Array.Empty<byte>())` — BigInteger zero is stored
        // as empty bytes.
        snapshot.add(
            Self::voters_count_key(),
            StorageItem::from_bytes(Vec::new()),
        );
        let gas_record_key = StorageKey::create_with_uint32(Self::ID, PREFIX_GAS_PER_BLOCK, 0);
        snapshot.add(
            gas_record_key,
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_GAS_PER_BLOCK,
            ))),
        );
        snapshot.add(
            StorageKey::create(Self::ID, PREFIX_REGISTER_PRICE),
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
        let block_index = engine
            .persisting_block()
            .map(|block| block.index())
            .ok_or_else(|| {
                CoreError::invalid_operation("NeoToken::on_persist requires a persisting block")
            })?;
        let committee_count = engine.protocol_settings().committee_members_count();
        if committee_count == 0 {
            return Err(CoreError::invalid_operation(
                "NeoToken::on_persist requires a non-empty standby committee",
            ));
        }
        if !Self::should_refresh_committee(block_index, committee_count) {
            return Ok(());
        }
        let settings = engine.protocol_settings().clone();
        let snapshot = engine.snapshot_cache();
        // C# `GetAndChange(Prefix_Committee)!` — a missing cache faults.
        let prev_committee = self.read_committee_with_votes(&snapshot)?;
        let new_committee = self.compute_committee_members(&snapshot, &settings)?;
        snapshot.update(
            StorageKey::create(Self::ID, PREFIX_COMMITTEE),
            StorageItem::from_bytes(Self::encode_committee(&new_committee)?),
        );
        // Hardfork check for https://github.com/neo-project/neo/pull/3158.
        if engine.is_hardfork_enabled(Hardfork::HfCockatrice) {
            let prev_keys: Vec<&ECPoint> = prev_committee.iter().map(|(point, _)| point).collect();
            let new_keys: Vec<&ECPoint> = new_committee.iter().map(|(point, _)| point).collect();
            if prev_keys != new_keys {
                let prev_key_item = Self::points_to_stack_item(prev_keys.iter().copied())?;
                let new_key_item = Self::points_to_stack_item(new_keys.iter().copied())?;
                engine
                    .send_notification(
                        Self::script_hash(),
                        "CommitteeChanged".to_string(),
                        vec![prev_key_item, new_key_item],
                    )
                    .map_err(|e| {
                        CoreError::invalid_operation(format!(
                            "NeoToken::on_persist: CommitteeChanged notify: {e}"
                        ))
                    })?;
            }
        }
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
        let block_index = engine
            .persisting_block()
            .map(|block| block.index())
            .ok_or_else(|| {
                CoreError::invalid_operation("NeoToken::post_persist requires a persisting block")
            })?;
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
        let committee = self.read_committee_with_votes(&snapshot)?;
        let member_index = (block_index % (committee_count as u32)) as usize;
        let (member, _) = committee.get(member_index).ok_or_else(|| {
            CoreError::invalid_operation("NeoToken::post_persist: committee cache too small")
        })?;
        let account =
            UInt160::from_script(&Contract::create_signature_redeem_script(member.clone()));
        let committee_reward = &gas_per_block * COMMITTEE_REWARD_RATIO / 100;
        crate::GasToken::new().gas_mint(engine, &account, &committee_reward, false)?;

        // Record the cumulative reward of the voters of the committee.
        if Self::should_refresh_committee(block_index, committee_count) {
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
                    let key = StorageKey::create_with_bytes(
                        Self::ID,
                        PREFIX_VOTER_REWARD_PER_COMMITTEE,
                        &member.to_bytes(),
                    );
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
        match method {
            "symbol" => Ok(Self::SYMBOL.as_bytes().to_vec()),
            "decimals" => Ok(BigInt::from(Self::DECIMALS).to_signed_bytes_le()),
            "totalSupply" => {
                // C# `NeoToken.TotalSupply` overrides the fungible-token storage
                // reader and returns the immutable protocol amount.
                Ok(BigInt::from(NEO_TOTAL_AMOUNT).to_signed_bytes_le())
            }
            "balanceOf" => {
                let account = crate::args::raw_account(args, "NeoToken::balanceOf")?;
                let snapshot = engine.snapshot_cache();
                Ok(crate::read_nep17_balance(&snapshot, Self::ID, &account)?.to_signed_bytes_le())
            }
            "transfer" => {
                // C# FungibleToken.Transfer(from, to, amount, data) with NEO's
                // governance OnBalanceChanging side-effects.
                let from = crate::args::raw_hash160(args, 0, "NeoToken::transfer")?;
                let to = crate::args::raw_hash160(args, 1, "NeoToken::transfer")?;
                let amount = BigInt::from_signed_bytes_le(args.get(2).ok_or_else(|| {
                    CoreError::invalid_operation("NeoToken::transfer requires an amount")
                })?);
                let data = args.get(3).map(Vec::as_slice).unwrap_or(&[]);
                let caller = engine
                    .get_calling_script_hash()
                    .unwrap_or_else(UInt160::zero);
                Ok(vec![u8::from(self.neo_transfer_core(
                    engine, caller, &from, &to, &amount, data,
                )?)])
            }
            "getGasPerBlock" => {
                let snapshot = engine.snapshot_cache();
                let index = LedgerContract::new()
                    .current_index(&snapshot)?
                    .saturating_add(1);
                Ok(self.gas_per_block_at(&snapshot, index).to_signed_bytes_le())
            }
            "getRegisterPrice" => {
                let snapshot = engine.snapshot_cache();
                Ok(BigInt::from(self.register_price(&snapshot)?).to_signed_bytes_le())
            }
            "setRegisterPrice" => {
                // C#: validate registerPrice > 0 -> AssertCommittee -> overwrite
                // Prefix_RegisterPrice.
                let price = args
                    .first()
                    .map(|b| BigInt::from_signed_bytes_le(b))
                    .and_then(|b| b.to_i64())
                    .ok_or_else(|| {
                        CoreError::invalid_operation("NeoToken::setRegisterPrice requires a price")
                    })?;
                if price <= 0 {
                    return Err(CoreError::invalid_operation(format!(
                        "RegisterPrice must be positive, got {price}"
                    )));
                }
                crate::committee::assert_committee(engine, "setRegisterPrice")?;
                self.put_register_price(&engine.snapshot_cache(), price)?;
                Ok(Vec::new())
            }
            "setGasPerBlock" => {
                // C#: validate 0 <= gasPerBlock <= 10*GAS.Factor -> AssertCommittee
                // -> write a Prefix_GasPerBlock record at (persisting index + 1).
                let gas_per_block = args
                    .first()
                    .map(|b| BigInt::from_signed_bytes_le(b))
                    .ok_or_else(|| {
                        CoreError::invalid_operation("NeoToken::setGasPerBlock requires a value")
                    })?;
                // GAS.Factor = 10^8; the inclusive upper bound is 10 GAS.
                let max = BigInt::from(10) * BigInt::from(100_000_000i64);
                if gas_per_block < BigInt::from(0) || gas_per_block > max {
                    return Err(CoreError::invalid_operation(format!(
                        "GasPerBlock must be between [0, {max}]"
                    )));
                }
                crate::committee::assert_committee(engine, "setGasPerBlock")?;
                // C# `engine.PersistingBlock!.Index + 1`: the method runs during
                // block persistence, so a missing persisting block is a fault
                // (matching the C# null-forgiving deref throwing on null).
                let index = engine
                    .persisting_block()
                    .map(|b| b.index())
                    .ok_or_else(|| {
                        CoreError::invalid_operation(
                            "NeoToken::setGasPerBlock requires a persisting block",
                        )
                    })?
                    .checked_add(1)
                    .ok_or_else(|| {
                        CoreError::invalid_operation(
                            "NeoToken::setGasPerBlock: block index overflow",
                        )
                    })?;
                self.put_gas_per_block(&engine.snapshot_cache(), index, &gas_per_block);
                Ok(Vec::new())
            }
            "getCommittee" => {
                // C# returns ECPoint[] sorted ascending; marshaled as an Array of
                // compressed (33-byte) public-key byte strings.
                let snapshot = engine.snapshot_cache();
                Self::points_to_array_bytes(&self.committee_sorted(&snapshot)?)
            }
            "getNextBlockValidators" => {
                // First ValidatorsCount committee members (stored order), sorted.
                let count =
                    usize::try_from(engine.protocol_settings().validators_count).unwrap_or(0);
                let snapshot = engine.snapshot_cache();
                Self::points_to_array_bytes(&self.next_block_validators(&snapshot, count)?)
            }
            "getCandidates" => {
                let snapshot = engine.snapshot_cache();
                // C# `GetCandidatesInternal().Select(...).Take(256).ToArray()`
                // (NeoToken.cs:528): at most the first 256 registered candidates.
                let mut candidates = self.read_registered_candidates(&snapshot)?;
                candidates.truncate(256);
                Self::candidates_to_array_bytes(&candidates)
            }
            "getCandidateVote" => {
                let pubkey_bytes = args.first().ok_or_else(|| {
                    CoreError::invalid_operation("NeoToken::getCandidateVote requires a public key")
                })?;
                // C# takes an ECPoint; an invalid key faults at marshaling.
                let pubkey = ECPoint::from_bytes(pubkey_bytes).map_err(|e| {
                    CoreError::invalid_operation(format!(
                        "NeoToken::getCandidateVote: bad public key: {e}"
                    ))
                })?;
                let snapshot = engine.snapshot_cache();
                Ok(self
                    .candidate_vote(&snapshot, &pubkey)?
                    .to_signed_bytes_le())
            }
            "registerCandidate" => {
                // C# RegisterCandidate (Echidna V1) + RegisterInternal: charge the
                // register price, then require a witness from the candidate's
                // signature-contract account; create/flip the CandidateState to
                // Registered and (post-Echidna) emit CandidateStateChanged.
                let pubkey_bytes = args.first().ok_or_else(|| {
                    CoreError::invalid_operation(
                        "NeoToken::registerCandidate requires a public key",
                    )
                })?;
                let pubkey = ECPoint::from_bytes(pubkey_bytes).map_err(|e| {
                    CoreError::invalid_operation(format!(
                        "NeoToken::registerCandidate: bad public key: {e}"
                    ))
                })?;
                // engine.AddFee(GetRegisterPrice * FeeFactor) — charged before the
                // witness check, matching the V1 ordering.
                let price = self.register_price(&engine.snapshot_cache())?;
                engine
                    .charge_execution_fee(u64::try_from(price).unwrap_or(0))
                    .map_err(|e| {
                        CoreError::invalid_operation(format!(
                            "NeoToken::registerCandidate: fee: {e}"
                        ))
                    })?;
                Ok(vec![u8::from(self.register_internal(
                    engine,
                    &pubkey,
                    "registerCandidate",
                )?)])
            }
            "getAllCandidates" => {
                // C# GetAllCandidates (NeoToken.cs:537-545): a StorageIterator
                // over the registered, non-blocked candidate entries with
                // RemovePrefix | DeserializeValues | PickField1 and prefix
                // length 1 — each element is Struct[33-byte pubkey, Votes]. The
                // 4-byte iterator id is decoded back into an InteropInterface
                // by the dispatcher.
                let results = self.registered_candidate_entries(&engine.snapshot_cache())?;
                let iterator_id = engine
                    .create_storage_iterator_with_options(
                        results,
                        1,
                        FindOptions::RemovePrefix
                            | FindOptions::DeserializeValues
                            | FindOptions::PickField1,
                    )
                    .map_err(|e| {
                        CoreError::invalid_operation(format!("NeoToken::getAllCandidates: {e}"))
                    })?;
                Ok(iterator_id.to_le_bytes().to_vec())
            }
            "onNEP17Payment" => {
                // C# NeoToken.OnNEP17Payment (NeoToken.cs:374-389, HF_Echidna):
                // candidate registration by paying the register price in GAS to
                // the NEO contract. The `from` argument is unused — the witness
                // requirement is RegisterInternal's, on the candidate account
                // derived from `data`'s public key.
                if engine.get_calling_script_hash() != Some(crate::GasToken::script_hash()) {
                    return Err(CoreError::invalid_operation(
                        "NeoToken::onNEP17Payment: only the GAS contract can call this method",
                    ));
                }
                let amount = BigInt::from_signed_bytes_le(args.get(1).ok_or_else(|| {
                    CoreError::invalid_operation("NeoToken::onNEP17Payment requires an amount")
                })?);
                let price = self.register_price(&engine.snapshot_cache())?;
                if amount != BigInt::from(price) {
                    return Err(CoreError::invalid_operation(format!(
                        "NeoToken::onNEP17Payment: incorrect GAS amount; expected {price}, received {amount}"
                    )));
                }
                // `data` is an Any param (it arrives BinarySerialized); C#
                // decodes its span as a secp256r1 point, faulting on anything
                // that is not a valid public key (including Null).
                let data = args.get(2).map(Vec::as_slice).unwrap_or(&[]);
                let limits = ExecutionEngineLimits::default();
                let item = BinarySerializer::deserialize_stack_value_with_limits(
                    data,
                    limits.max_item_size as usize,
                    limits.max_stack_size as usize,
                )
                .map_err(|e| {
                    CoreError::invalid_operation(format!("NeoToken::onNEP17Payment data: {e}"))
                })?;
                let pubkey_bytes = item.to_byte_string_bytes().ok_or_else(|| {
                    CoreError::invalid_operation(
                        "NeoToken::onNEP17Payment data: cannot convert to bytes",
                    )
                })?;
                let pubkey = ECPoint::from_bytes(&pubkey_bytes).map_err(|e| {
                    CoreError::invalid_operation(format!(
                        "NeoToken::onNEP17Payment: bad public key: {e}"
                    ))
                })?;
                if !self.register_internal(engine, &pubkey, "onNEP17Payment")? {
                    return Err(CoreError::invalid_operation(
                        "NeoToken::onNEP17Payment: failed to register candidate",
                    ));
                }
                // C# `await GAS.Burn(engine, Hash, amount)`: burn the GAS this
                // transfer just credited to the NEO contract's own account.
                crate::GasToken::new().gas_burn(engine, &Self::script_hash(), &amount)?;
                Ok(Vec::new())
            }
            "unregisterCandidate" => {
                // C# UnregisterCandidate: witness on the candidate account, flip the
                // CandidateState to unregistered; CheckCandidate deletes the entry
                // once it has no remaining votes.
                let pubkey_bytes = args.first().ok_or_else(|| {
                    CoreError::invalid_operation(
                        "NeoToken::unregisterCandidate requires a public key",
                    )
                })?;
                let pubkey = ECPoint::from_bytes(pubkey_bytes).map_err(|e| {
                    CoreError::invalid_operation(format!(
                        "NeoToken::unregisterCandidate: bad public key: {e}"
                    ))
                })?;
                let account =
                    UInt160::from_script(&Contract::create_signature_redeem_script(pubkey.clone()));
                let authorized = engine.check_witness_hash(&account).map_err(|e| {
                    CoreError::invalid_operation(format!(
                        "NeoToken::unregisterCandidate: witness: {e}"
                    ))
                })?;
                if !authorized {
                    return Ok(vec![0u8]);
                }
                let snapshot = engine.snapshot_cache();
                let key = Self::candidate_key(&pubkey);
                let Some(item) = snapshot.get(&key) else {
                    return Ok(vec![1u8]); // not a candidate -> true
                };
                let (registered, votes) = Self::decode_candidate_state(&item.value_bytes())?;
                if !registered {
                    return Ok(vec![1u8]);
                }
                // C# `state.Registered = false; CheckCandidate(snapshot, pubkey,
                // state)` (NeoToken.cs:443,191): flip to unregistered, then when no
                // votes remain delete BOTH the candidate entry and the
                // `Prefix_VoterRewardPerCommittee` entry (otherwise a candidate that
                // accrued committee voter rewards and then lost all votes would leave
                // a stale reward record — a state-root divergence). Retain as
                // unregistered when votes remain.
                self.check_candidate(&snapshot, &pubkey, false, &votes)?;
                if engine.is_hardfork_enabled(Hardfork::HfEchidna) {
                    engine
                        .send_notification(
                            Self::script_hash(),
                            "CandidateStateChanged".to_string(),
                            vec![
                                StackItem::from_byte_string(pubkey.to_bytes()),
                                StackItem::from_bool(false),
                                StackItem::from_int(votes),
                            ],
                        )
                        .map_err(|e| {
                            CoreError::invalid_operation(format!(
                                "NeoToken::unregisterCandidate: notify: {e}"
                            ))
                        })?;
                }
                Ok(vec![1u8])
            }
            "vote" => {
                // C# Vote -> VoteInternal: witness on the voter, then the vote
                // transition (extracted into `vote_internal` so PolicyContract's
                // blockAccount can clear a blocked account's vote the way C#
                // calls `NEO.VoteInternal` directly).
                let account = crate::args::raw_account(args, "NeoToken::vote")?;
                // voteTo is a nullable PublicKey (bit 1 of the arg null-mask).
                let vote_to_is_null = engine
                    .get_state::<NativeArgNullMask>()
                    .is_some_and(|mask| mask.0 & (1 << 1) != 0);
                let vote_to: Option<ECPoint> = if vote_to_is_null {
                    None
                } else {
                    let bytes = args.get(1).ok_or_else(|| {
                        CoreError::invalid_operation(
                            "NeoToken::vote requires a candidate (or null)",
                        )
                    })?;
                    Some(ECPoint::from_bytes(bytes).map_err(|e| {
                        CoreError::invalid_operation(format!("NeoToken::vote: bad candidate: {e}"))
                    })?)
                };
                if !engine.check_witness_hash(&account).map_err(|e| {
                    CoreError::invalid_operation(format!("NeoToken::vote: witness: {e}"))
                })? {
                    return Ok(vec![0u8]);
                }
                Ok(vec![u8::from(self.vote_internal(
                    engine,
                    &account,
                    vote_to.as_ref(),
                )?)])
            }
            "getCommitteeAddress" => {
                let snapshot = engine.snapshot_cache();
                Ok(self.compute_committee_address(&snapshot)?.to_bytes())
            }
            "getAccountState" => {
                let account = crate::args::raw_account(args, "NeoToken::getAccountState")?;
                let snapshot = engine.snapshot_cache();
                // C# returns the NeoAccountState struct, or null (empty payload)
                // when the account has no entry.
                Ok(self
                    .read_account_state(&snapshot, &account)
                    .unwrap_or_default())
            }
            "unclaimedGas" => {
                // C# UnclaimedGas(account, end): `end` must equal the persisting
                // block index (or Ledger.CurrentIndex + 1); compute CalculateBonus
                // for the account's NeoAccountState (zero when it has no entry).
                let account = crate::args::raw_account(args, "NeoToken::unclaimedGas")?;
                let end = args
                    .get(1)
                    .map(|b| BigInt::from_signed_bytes_le(b))
                    .and_then(|b| b.to_u32())
                    .ok_or_else(|| {
                        CoreError::invalid_operation("NeoToken::unclaimedGas requires an end index")
                    })?;
                let snapshot = engine.snapshot_cache();
                let expect_end = match engine.persisting_block() {
                    Some(block) => block.index(),
                    None => LedgerContract::new()
                        .current_index(&snapshot)?
                        .saturating_add(1),
                };
                if end != expect_end {
                    return Err(CoreError::invalid_operation(format!(
                        "NeoToken::unclaimedGas: end {end} must equal {expect_end}"
                    )));
                }
                let bonus = match self.read_account_state(&snapshot, &account) {
                    Some(bytes) => {
                        let state = Self::decode_neo_account_state(&bytes)?;
                        self.calculate_bonus(&snapshot, &state, end)?
                    }
                    None => BigInt::from(0),
                };
                Ok(bonus.to_signed_bytes_le())
            }
            other => Err(CoreError::invalid_operation(format!(
                "NeoToken method '{other}' is not implemented"
            ))),
        }
    }
}

#[cfg(test)]
mod tests;
