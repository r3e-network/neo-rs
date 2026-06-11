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
use neo_vm::StackItem;
use neo_vm_rs::ExecutionEngineLimits;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

use crate::hashes::NEO_TOKEN_HASH;
use crate::LedgerContract;

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

/// Lazily-initialised script-hash handle for the NEO native contract.
pub static NEO_HASH: LazyLock<UInt160> = LazyLock::new(|| *NEO_TOKEN_HASH);

/// The NeoToken native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct NeoToken;

impl NeoToken {
    /// Stable native contract id (matches C# `NeoToken`).
    pub const ID: i32 = -5;
    /// NEP-17 symbol (C# `NeoToken.Symbol => "NEO"`).
    pub const SYMBOL: &'static str = "NEO";
    /// NEP-17 decimals (C# `NeoToken.Decimals => 0`).
    pub const DECIMALS: u8 = 0;

    /// Construct a new `NeoToken` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the NEO script hash.
    pub fn script_hash() -> UInt160 {
        *NEO_HASH
    }
}

/// C# `GetRegisterPrice` = `(long)(BigInteger)snapshot[_registerPrice]`.
fn register_price(snapshot: &DataCache) -> CoreResult<i64> {
    crate::read_storage_int(
        snapshot,
        NeoToken::ID,
        PREFIX_REGISTER_PRICE,
        DEFAULT_REGISTER_PRICE,
    )
}

/// C# `SetRegisterPrice` storage effect: overwrite `Prefix_RegisterPrice` as a
/// `BigInteger` (`GetAndChange(_registerPrice).Set(registerPrice)`).
fn put_register_price(snapshot: &DataCache, price: i64) {
    snapshot.update(
        StorageKey::new(NeoToken::ID, vec![PREFIX_REGISTER_PRICE]),
        StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(price))),
    );
}

/// C# `SetGasPerBlock` storage effect: write a `Prefix_GasPerBlock` record at
/// `index` (a big-endian `uint` key suffix), overwriting any record already at
/// that index (`GetAndChange(key, factory).Set(gasPerBlock)`). `update` upserts
/// (a brand-new index key is tracked as Changed), which commits to the same
/// stored key/value as the C# Added path — only the resulting store contents
/// feed the state root.
fn put_gas_per_block(snapshot: &DataCache, index: u32, gas_per_block: &BigInt) {
    let mut key = vec![PREFIX_GAS_PER_BLOCK];
    key.extend_from_slice(&index.to_be_bytes());
    snapshot.update(
        StorageKey::new(NeoToken::ID, key),
        StorageItem::from_bytes(crate::bigint_to_storage_bytes(gas_per_block)),
    );
}

/// Returns the GAS-per-block effective at `index`: the most recent
/// `Prefix_GasPerBlock` record whose record index is ≤ `index` (C#
/// `GetSortedGasRecords(...).First().GasPerBlock`), defaulting to 5 GAS.
fn gas_per_block_at(snapshot: &DataCache, index: u32) -> BigInt {
    let prefix = StorageKey::new(NeoToken::ID, vec![PREFIX_GAS_PER_BLOCK]);
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

/// Decoded view of a `NeoAccountState` (`Struct[Balance, BalanceHeight, VoteTo,
/// LastGasPerVote]`, C# `NeoAccountState.FromStackItem`).
struct NeoAccountStateView {
    balance: BigInt,
    balance_height: u32,
    vote_to: Option<ECPoint>,
    last_gas_per_vote: BigInt,
}

/// Decodes a stored `NeoAccountState` struct into its fields.
fn decode_neo_account_state(value: &[u8]) -> CoreResult<NeoAccountStateView> {
    let decoded = BinarySerializer::deserialize(value, &ExecutionEngineLimits::default(), None)
        .map_err(|e| CoreError::deserialization(format!("neo account state: {e}")))?;
    let StackItem::Struct(fields) = decoded else {
        return Err(CoreError::invalid_data("neo account state is not a struct"));
    };
    let items = fields.items();
    let balance = items
        .first()
        .ok_or_else(|| CoreError::invalid_data("neo account state missing balance"))?
        .as_int()
        .map_err(|e| CoreError::invalid_data(format!("account balance: {e}")))?;
    let balance_height = match items.get(1) {
        Some(f) => f
            .as_int()
            .map_err(|e| CoreError::invalid_data(format!("account balanceHeight: {e}")))?
            .to_u32()
            .unwrap_or(0),
        None => 0,
    };
    let vote_to = match items.get(2) {
        Some(f) if !matches!(f, StackItem::Null) => {
            let bytes = f
                .as_bytes()
                .map_err(|e| CoreError::invalid_data(format!("account voteTo: {e}")))?;
            Some(
                ECPoint::from_bytes(&bytes)
                    .map_err(|e| CoreError::invalid_data(format!("account voteTo point: {e}")))?,
            )
        }
        _ => None,
    };
    let last_gas_per_vote = match items.get(3) {
        Some(f) => f
            .as_int()
            .map_err(|e| CoreError::invalid_data(format!("account lastGasPerVote: {e}")))?,
        None => BigInt::from(0),
    };
    Ok(NeoAccountStateView { balance, balance_height, vote_to, last_gas_per_vote })
}

/// Encodes a `NeoAccountState` (`Struct[Balance, BalanceHeight, VoteTo,
/// LastGasPerVote]`) — the write counterpart of [`decode_neo_account_state`].
fn encode_neo_account_state(state: &NeoAccountStateView) -> CoreResult<Vec<u8>> {
    let vote_to = match &state.vote_to {
        Some(pubkey) => StackItem::from_byte_string(pubkey.to_bytes()),
        None => StackItem::null(),
    };
    let item = StackItem::from_struct(vec![
        StackItem::from_int(state.balance.clone()),
        StackItem::from_int(BigInt::from(state.balance_height)),
        vote_to,
        StackItem::from_int(state.last_gas_per_vote.clone()),
    ]);
    BinarySerializer::serialize(&item, &ExecutionEngineLimits::default())
        .map_err(|e| CoreError::invalid_operation(format!("encode neo account state: {e}")))
}

/// The `Prefix_VotersCount` storage key (a single key, no suffix).
fn voters_count_key() -> StorageKey {
    StorageKey::new(NeoToken::ID, vec![PREFIX_VOTERS_COUNT])
}

/// Reads the total voted NEO (`Prefix_VotersCount`), defaulting to zero.
fn read_voters_count(snapshot: &DataCache) -> BigInt {
    snapshot
        .get(&voters_count_key())
        .map(|item| BigInt::from_signed_bytes_le(&item.value_bytes()))
        .unwrap_or_else(|| BigInt::from(0))
}

/// Writes the total voted NEO (`Prefix_VotersCount`).
fn write_voters_count(snapshot: &DataCache, value: &BigInt) {
    snapshot.update(voters_count_key(), StorageItem::from_bytes(crate::bigint_to_storage_bytes(value)));
}

/// C# `NeoToken.CheckCandidate`: when a candidate is unregistered and has no
/// remaining votes, delete its candidate + voter-reward entries.
fn check_candidate(snapshot: &DataCache, pubkey: &ECPoint, registered: bool, votes: &BigInt) -> CoreResult<()> {
    if !registered && *votes == BigInt::from(0) {
        let mut reward_key = vec![PREFIX_VOTER_REWARD_PER_COMMITTEE];
        reward_key.extend_from_slice(&pubkey.to_bytes());
        snapshot.delete(&StorageKey::new(NeoToken::ID, reward_key));
        snapshot.delete(&candidate_key(pubkey));
    } else {
        snapshot.update(candidate_key(pubkey), StorageItem::from_bytes(encode_candidate_state(registered, votes)?));
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
    engine: &ApplicationEngine,
    snapshot: &DataCache,
    state: &mut NeoAccountStateView,
    amount: &BigInt,
) -> CoreResult<Option<BigInt>> {
    // DistributeGas: bonus on the OLD state, then advance the reward markers.
    let mut distribution = None;
    if let Some(block) = engine.persisting_block() {
        let end = block.index();
        let bonus = calculate_bonus(snapshot, state, end)?;
        state.balance_height = end;
        if let Some(vote_to) = &state.vote_to {
            state.last_gas_per_vote = voter_reward_per_committee(snapshot, vote_to);
        }
        if bonus != BigInt::from(0) {
            distribution = Some(bonus);
        }
    }
    // Vote-weight: a balance delta moves the voted candidate's weight + voters count.
    if *amount != BigInt::from(0) {
        if let Some(vote_to) = state.vote_to.clone() {
            let mut count = read_voters_count(snapshot);
            count += amount;
            write_voters_count(snapshot, &count);
            if let Some(item) = snapshot.get(&candidate_key(&vote_to)) {
                let (registered, mut votes) = decode_candidate_state(&item.value_bytes())?;
                votes += amount;
                check_candidate(snapshot, &vote_to, registered, &votes)?;
            }
        }
    }
    Ok(distribution)
}

/// C# `FungibleToken.PostTransferAsync` for NEO: emit `Transfer(from, to, amount)`
/// and, when `to` is a deployed contract, queue its `onNEP17Payment` callback.
fn neo_post_transfer(
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
    engine: &mut ApplicationEngine,
    caller: UInt160,
    from: &UInt160,
    to: &UInt160,
    amount: &BigInt,
    data: &[u8],
) -> CoreResult<bool> {
    if *amount < BigInt::from(0) {
        return Err(CoreError::invalid_operation("NeoToken::transfer: amount cannot be negative"));
    }
    if caller != *from
        && !engine
            .check_witness(from)
            .map_err(|e| CoreError::invalid_operation(format!("NeoToken::transfer: witness: {e}")))?
    {
        return Ok(false);
    }
    let snapshot = engine.snapshot_cache();
    let zero = BigInt::from(0);
    let mut distributions: Vec<(UInt160, BigInt)> = Vec::new();
    let from_state = read_account_state(&snapshot, from);

    if *amount == zero {
        if let Some(bytes) = from_state {
            let mut state = decode_neo_account_state(&bytes)?;
            if let Some(d) = neo_on_balance_changing(engine, &snapshot, &mut state, &zero)? {
                distributions.push((*from, d));
            }
            snapshot.update(neo_account_key(from), StorageItem::from_bytes(encode_neo_account_state(&state)?));
        }
    } else {
        let Some(bytes) = from_state else {
            return Ok(false);
        };
        let mut from_state = decode_neo_account_state(&bytes)?;
        if from_state.balance < *amount {
            return Ok(false);
        }
        if from == to {
            if let Some(d) = neo_on_balance_changing(engine, &snapshot, &mut from_state, &zero)? {
                distributions.push((*from, d));
            }
            snapshot.update(neo_account_key(from), StorageItem::from_bytes(encode_neo_account_state(&from_state)?));
        } else {
            let neg_amount = -amount;
            if let Some(d) = neo_on_balance_changing(engine, &snapshot, &mut from_state, &neg_amount)? {
                distributions.push((*from, d));
            }
            if from_state.balance == *amount {
                snapshot.delete(&neo_account_key(from));
            } else {
                from_state.balance -= amount;
                snapshot.update(neo_account_key(from), StorageItem::from_bytes(encode_neo_account_state(&from_state)?));
            }
            let mut to_state = match read_account_state(&snapshot, to) {
                Some(bytes) => decode_neo_account_state(&bytes)?,
                None => NeoAccountStateView {
                    balance: BigInt::from(0),
                    balance_height: 0,
                    vote_to: None,
                    last_gas_per_vote: BigInt::from(0),
                },
            };
            if let Some(d) = neo_on_balance_changing(engine, &snapshot, &mut to_state, amount)? {
                distributions.push((*to, d));
            }
            to_state.balance += amount;
            snapshot.update(neo_account_key(to), StorageItem::from_bytes(encode_neo_account_state(&to_state)?));
        }
    }

    neo_post_transfer(engine, from, to, amount, data)?;
    for (account, datoshi) in distributions {
        crate::gas_token::gas_mint(engine, &account, &datoshi, true)?;
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
    engine: &mut ApplicationEngine,
    account: &UInt160,
    vote_to: Option<&ECPoint>,
) -> CoreResult<bool> {
    let vote_to: Option<ECPoint> = vote_to.cloned();
    let snapshot = engine.snapshot_cache();
    let Some(acct_bytes) = read_account_state(&snapshot, account) else {
        return Ok(false); // no account state
    };
    let mut acct = decode_neo_account_state(&acct_bytes)?;
    if acct.balance == BigInt::from(0) {
        return Ok(false);
    }
    // The new candidate must exist and be registered.
    if let Some(new_pk) = &vote_to {
        match snapshot.get(&candidate_key(new_pk)) {
            Some(item) => {
                let (registered, _) = decode_candidate_state(&item.value_bytes())?;
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
        let mut count = read_voters_count(&snapshot);
        if old_vote.is_none() {
            count += &acct.balance;
        } else {
            count -= &acct.balance;
        }
        write_voters_count(&snapshot, &count);
    }
    // DistributeGas: compute the bonus with the OLD state, then advance
    // BalanceHeight + LastGasPerVote (only when a persisting block exists).
    let mut gas_to_mint = BigInt::from(0);
    if let Some(block) = engine.persisting_block() {
        let end = block.index();
        let bonus = calculate_bonus(&snapshot, &acct, end)?;
        acct.balance_height = end;
        if let Some(old_pk) = &old_vote {
            acct.last_gas_per_vote = voter_reward_per_committee(&snapshot, old_pk);
        }
        if bonus != BigInt::from(0) {
            gas_to_mint = bonus;
        }
    }
    // Remove the account's weight from the previously-voted candidate.
    if let Some(old_pk) = &old_vote {
        if let Some(item) = snapshot.get(&candidate_key(old_pk)) {
            let (registered, mut votes) = decode_candidate_state(&item.value_bytes())?;
            votes -= &acct.balance;
            check_candidate(&snapshot, old_pk, registered, &votes)?;
        }
    }
    // Switching to a new (different) candidate resets the reward marker.
    if let Some(new_pk) = &vote_to {
        if Some(new_pk) != old_vote.as_ref() {
            acct.last_gas_per_vote = voter_reward_per_committee(&snapshot, new_pk);
        }
    }
    let from = old_vote.clone();
    acct.vote_to = vote_to.clone();
    // Add the account's weight to the new candidate (re-read so a vote
    // for the same candidate nets to zero), else clear the reward marker.
    if let Some(new_pk) = &vote_to {
        let item = snapshot.get(&candidate_key(new_pk)).ok_or_else(|| {
            CoreError::invalid_operation("NeoToken::vote: candidate disappeared")
        })?;
        let (registered, mut votes) = decode_candidate_state(&item.value_bytes())?;
        votes += &acct.balance;
        snapshot.update(
            candidate_key(new_pk),
            StorageItem::from_bytes(encode_candidate_state(registered, &votes)?),
        );
    } else {
        acct.last_gas_per_vote = BigInt::from(0);
    }
    snapshot.update(
        neo_account_key(account),
        StorageItem::from_bytes(encode_neo_account_state(&acct)?),
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
        crate::gas_token::gas_mint(engine, account, &gas_to_mint, true)?;
    }
    Ok(true)
}

/// C# `GetSortedGasRecords(snapshot, end)`: the `Prefix_GasPerBlock` records with
/// index ≤ `end`, descending by index.
fn sorted_gas_records(snapshot: &DataCache, end: u32) -> Vec<(u32, BigInt)> {
    let prefix = StorageKey::new(NeoToken::ID, vec![PREFIX_GAS_PER_BLOCK]);
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
fn voter_reward_per_committee(snapshot: &DataCache, pubkey: &ECPoint) -> BigInt {
    let mut key_bytes = vec![PREFIX_VOTER_REWARD_PER_COMMITTEE];
    key_bytes.extend_from_slice(&pubkey.to_bytes());
    snapshot
        .get(&StorageKey::new(NeoToken::ID, key_bytes))
        .map(|item| BigInt::from_signed_bytes_le(&item.value_bytes()))
        .unwrap_or_else(|| BigInt::from(0))
}

/// C# `NeoToken.CalculateBonus`: the unclaimed GAS for an account between
/// `BalanceHeight` and `end` — the NEO-holder reward (`balance * Σ gasPerBlock *
/// 10 / 100 / TotalAmount`) plus the vote reward (`balance * (latestGasPerVote -
/// lastGasPerVote) / VoteFactor`).
fn calculate_bonus(snapshot: &DataCache, state: &NeoAccountStateView, end: u32) -> CoreResult<BigInt> {
    if state.balance == BigInt::from(0) {
        return Ok(BigInt::from(0));
    }
    if state.balance < BigInt::from(0) {
        return Err(CoreError::invalid_operation("NeoToken account balance cannot be negative"));
    }
    if state.balance_height >= end {
        return Ok(BigInt::from(0));
    }

    // NEO-holder reward over [BalanceHeight, end), folding in each gas-per-block
    // change point (C# CalculateReward).
    let start = state.balance_height;
    let mut sum_gas_per_block = BigInt::from(0);
    let mut window_end = end;
    for (index, gas_per_block) in sorted_gas_records(snapshot, end.saturating_sub(1)) {
        if index > start {
            sum_gas_per_block += &gas_per_block * (window_end - index);
            window_end = index;
        } else {
            sum_gas_per_block += &gas_per_block * (window_end - start);
            break;
        }
    }
    let neo_holder_reward = &state.balance * &sum_gas_per_block * NEO_HOLDER_REWARD_RATIO
        / 100
        / NEO_TOTAL_AMOUNT;

    // Vote reward (only when the account currently votes).
    let vote_reward = match &state.vote_to {
        Some(vote) => {
            let latest = voter_reward_per_committee(snapshot, vote);
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
fn read_committee_with_votes(snapshot: &DataCache) -> CoreResult<Vec<(ECPoint, BigInt)>> {
    let key = StorageKey::new(NeoToken::ID, vec![PREFIX_COMMITTEE]);
    let item = snapshot.get(&key).ok_or_else(|| {
        CoreError::invalid_operation("NeoToken committee cache is not initialized")
    })?;
    let decoded = BinarySerializer::deserialize(&item.value_bytes(), &ExecutionEngineLimits::default(), None)
        .map_err(|e| CoreError::deserialization(format!("committee cache: {e}")))?;
    let StackItem::Array(array) = decoded else {
        return Err(CoreError::invalid_data("committee cache is not an array"));
    };
    let mut members = Vec::with_capacity(array.items().len());
    for element in array.items() {
        let StackItem::Struct(fields) = element else {
            return Err(CoreError::invalid_data("committee element is not a struct"));
        };
        let items = fields.items();
        let pubkey = items
            .first()
            .ok_or_else(|| CoreError::invalid_data("committee element is empty"))?;
        let bytes = pubkey
            .as_bytes()
            .map_err(|e| CoreError::invalid_data(format!("committee pubkey: {e}")))?;
        let point = ECPoint::from_bytes(&bytes)
            .map_err(|e| CoreError::invalid_data(format!("committee EC point: {e}")))?;
        let votes = match items.get(1) {
            Some(f) => f
                .as_int()
                .map_err(|e| CoreError::invalid_data(format!("committee votes: {e}")))?,
            None => BigInt::from(0),
        };
        members.push((point, votes));
    }
    Ok(members)
}

/// Reads only the cached committee public keys, in stored order.
fn read_committee_points(snapshot: &DataCache) -> CoreResult<Vec<ECPoint>> {
    Ok(read_committee_with_votes(snapshot)?
        .into_iter()
        .map(|(point, _)| point)
        .collect())
}

/// Serializes `(pubkey, votes)` committee members as the `Prefix_Committee`
/// storage value — an Array of `Struct[pubkey, votes]` (C#
/// `CachedCommittee.ToStackItem`), the byte-exact write counterpart of
/// [`read_committee_with_votes`].
fn encode_committee(members: &[(ECPoint, BigInt)]) -> CoreResult<Vec<u8>> {
    let array = StackItem::from_array(
        members
            .iter()
            .map(|(point, votes)| {
                StackItem::from_struct(vec![
                    StackItem::from_byte_string(point.to_bytes()),
                    StackItem::from_int(votes.clone()),
                ])
            })
            .collect::<Vec<_>>(),
    );
    BinarySerializer::serialize(&array, &ExecutionEngineLimits::default())
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
    snapshot: &DataCache,
    settings: &neo_config::ProtocolSettings,
) -> CoreResult<Vec<(ECPoint, BigInt)>> {
    let voters_count = read_voters_count(snapshot);
    let candidates = read_registered_candidates(snapshot)?;
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
        return Err(CoreError::invalid_operation("BFT address requires at least one key"));
    }
    let m = pubkeys.len() - (pubkeys.len() - 1) / 3;
    let script = neo_redeem_script::multi_sig_redeem_script_from_points(m, pubkeys)
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
    engine: &mut ApplicationEngine,
    account: &UInt160,
    amount: &BigInt,
    call_on_payment: bool,
) -> CoreResult<()> {
    let zero = BigInt::from(0);
    if *amount < zero {
        return Err(CoreError::invalid_operation("NeoToken::mint: amount cannot be negative"));
    }
    if *amount == zero {
        return Ok(());
    }
    let snapshot = engine.snapshot_cache();
    let mut state = match read_account_state(&snapshot, account) {
        Some(bytes) => decode_neo_account_state(&bytes)?,
        None => NeoAccountStateView {
            balance: BigInt::from(0),
            balance_height: 0,
            vote_to: None,
            last_gas_per_vote: BigInt::from(0),
        },
    };
    let mut distributions: Vec<(UInt160, BigInt)> = Vec::new();
    if let Some(datoshi) = neo_on_balance_changing(engine, &snapshot, &mut state, amount)? {
        distributions.push((*account, datoshi));
    }
    state.balance += amount;
    snapshot.update(
        neo_account_key(account),
        StorageItem::from_bytes(encode_neo_account_state(&state)?),
    );
    let supply_key = StorageKey::new(NeoToken::ID, vec![crate::NEP17_PREFIX_TOTAL_SUPPLY]);
    let supply = snapshot
        .get(&supply_key)
        .map(|item| BigInt::from_signed_bytes_le(&item.value_bytes()))
        .unwrap_or_else(|| BigInt::from(0))
        + amount;
    snapshot.update(supply_key, StorageItem::from_bytes(crate::bigint_to_storage_bytes(&supply)));
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
    if call_on_payment && crate::ContractManagement::is_contract(&engine.snapshot_cache(), account)
    {
        engine.queue_contract_call_from_native(
            NeoToken::script_hash(),
            *account,
            "onNEP17Payment",
            vec![StackItem::null(), StackItem::from_int(amount.clone()), StackItem::null()],
        );
    }
    for (target, datoshi) in distributions {
        crate::gas_token::gas_mint(engine, &target, &datoshi, call_on_payment)?;
    }
    Ok(())
}

/// C# `GetCommittee` = committee public keys sorted ascending (`OrderBy(p => p)`).
fn committee_sorted(snapshot: &DataCache) -> CoreResult<Vec<ECPoint>> {
    let mut points = read_committee_points(snapshot)?;
    points.sort();
    Ok(points)
}

/// C# `GetNextBlockValidators`: the first `validators_count` committee members
/// (in stored, vote-ranked order), then sorted ascending. Public so
/// `GasToken::on_persist` can resolve the primary validator the block's
/// network fees are minted to (C# GasToken.cs:55) and the blockchain service
/// can build the extensible-witness whitelist (C# `Blockchain.
/// UpdateExtensibleWitnessWhiteList`).
pub fn next_block_validators(snapshot: &DataCache, validators_count: usize) -> CoreResult<Vec<ECPoint>> {
    let mut points = read_committee_points(snapshot)?;
    points.truncate(validators_count);
    points.sort();
    Ok(points)
}

/// Decodes a `CandidateState` storage value — a `Struct[Registered(bool), Votes]`
/// — into `(registered, votes)`.
fn decode_candidate_state(value: &[u8]) -> CoreResult<(bool, BigInt)> {
    let decoded = BinarySerializer::deserialize(value, &ExecutionEngineLimits::default(), None)
        .map_err(|e| CoreError::deserialization(format!("candidate state: {e}")))?;
    let StackItem::Struct(fields) = decoded else {
        return Err(CoreError::invalid_data("candidate state is not a struct"));
    };
    let items = fields.items();
    let registered = items.first().is_some_and(|f| f.as_bool().unwrap_or(false));
    let votes = match items.get(1) {
        Some(f) => f
            .as_int()
            .map_err(|e| CoreError::invalid_data(format!("candidate votes: {e}")))?,
        None => BigInt::from(0),
    };
    Ok((registered, votes))
}

/// Encodes a `CandidateState` storage value — a `Struct[Registered(bool),
/// Votes]` — the write counterpart of [`decode_candidate_state`].
fn encode_candidate_state(registered: bool, votes: &BigInt) -> CoreResult<Vec<u8>> {
    let item = StackItem::from_struct(vec![
        StackItem::from_bool(registered),
        StackItem::from_int(votes.clone()),
    ]);
    BinarySerializer::serialize(&item, &ExecutionEngineLimits::default())
        .map_err(|e| CoreError::invalid_operation(format!("encode candidate state: {e}")))
}

/// The `Prefix_Candidate` storage key for `pubkey` (`prefix ++ 33-byte pubkey`).
fn candidate_key(pubkey: &ECPoint) -> StorageKey {
    let mut key = vec![PREFIX_CANDIDATE];
    key.extend_from_slice(&pubkey.to_bytes());
    StorageKey::new(NeoToken::ID, key)
}

/// The `Prefix_Account` storage key for `account` (NEP-17 account prefix).
fn neo_account_key(account: &UInt160) -> StorageKey {
    let mut key = vec![crate::NEP17_PREFIX_ACCOUNT];
    key.extend_from_slice(&account.to_bytes());
    StorageKey::new(NeoToken::ID, key)
}

/// C# `GetCandidatesInternal`: scan `Prefix_Candidate` (key = prefix ++ 33-byte
/// pubkey; value = CandidateState `Struct[Registered(bool), Votes]`), returning
/// the raw `(key, value)` storage entries of the registered candidates in
/// storage-scan order, excluding candidates whose signature-contract address is
/// blocked by `PolicyContract` (`!Policy.IsBlocked(snapshot, sigScriptHash)`).
fn registered_candidate_entries(
    snapshot: &DataCache,
) -> CoreResult<Vec<(StorageKey, StorageItem)>> {
    let prefix = StorageKey::new(NeoToken::ID, vec![PREFIX_CANDIDATE]);
    let mut out = Vec::new();
    for (key, item) in snapshot.find(Some(&prefix), SeekDirection::Forward) {
        let key_bytes = key.key();
        if key_bytes.len() < 34 {
            continue;
        }
        let Ok(pubkey) = ECPoint::from_bytes(&key_bytes[1..34]) else {
            continue;
        };
        let (registered, _votes) = decode_candidate_state(&item.value_bytes())?;
        if registered {
            let account =
                UInt160::from_script(&Contract::create_signature_redeem_script(pubkey));
            if snapshot
                .get(&crate::policy_contract::blocked_account_key(&account))
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
fn read_registered_candidates(snapshot: &DataCache) -> CoreResult<Vec<(ECPoint, BigInt)>> {
    registered_candidate_entries(snapshot)?
        .into_iter()
        .map(|(key, item)| {
            let pubkey = ECPoint::from_bytes(&key.key()[1..34])
                .map_err(|e| CoreError::invalid_data(format!("candidate key: {e}")))?;
            let (_registered, votes) = decode_candidate_state(&item.value_bytes())?;
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
    engine: &mut ApplicationEngine,
    pubkey: &ECPoint,
    method: &str,
) -> CoreResult<bool> {
    let account = UInt160::from_script(&Contract::create_signature_redeem_script(pubkey.clone()));
    let authorized = engine
        .check_witness_hash(&account)
        .map_err(|e| CoreError::invalid_operation(format!("NeoToken::{method}: witness: {e}")))?;
    if !authorized {
        return Ok(false);
    }
    let snapshot = engine.snapshot_cache();
    let key = candidate_key(pubkey);
    let (registered, votes) = match snapshot.get(&key) {
        Some(item) => decode_candidate_state(&item.value_bytes())?,
        None => (false, BigInt::from(0)),
    };
    if registered {
        return Ok(true);
    }
    snapshot.update(key, StorageItem::from_bytes(encode_candidate_state(true, &votes)?));
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
fn candidate_vote(snapshot: &DataCache, pubkey: &ECPoint) -> CoreResult<BigInt> {
    let mut key_bytes = vec![PREFIX_CANDIDATE];
    key_bytes.extend_from_slice(&pubkey.to_bytes());
    match snapshot.get(&StorageKey::new(NeoToken::ID, key_bytes)) {
        Some(item) => {
            let (registered, votes) = decode_candidate_state(&item.value_bytes())?;
            Ok(if registered { votes } else { BigInt::from(-1) })
        }
        None => Ok(BigInt::from(-1)),
    }
}

/// Marshals `(pubkey, votes)` candidate pairs as an Array of `Struct[pubkey,
/// votes]` (C# `(ECPoint, BigInteger)[]` return shape).
fn candidates_to_array_bytes(candidates: &[(ECPoint, BigInt)]) -> CoreResult<Vec<u8>> {
    let array = StackItem::from_array(
        candidates
            .iter()
            .map(|(pk, votes)| {
                StackItem::from_struct(vec![
                    StackItem::from_byte_string(pk.to_bytes()),
                    StackItem::from_int(votes.clone()),
                ])
            })
            .collect::<Vec<_>>(),
    );
    BinarySerializer::serialize(&array, &ExecutionEngineLimits::default())
        .map_err(|e| CoreError::invalid_operation(format!("getCandidates: {e}")))
}

/// Serializes EC points as an Array of compressed (33-byte) byte strings — the
/// return shape shared by `getCommittee` / `getNextBlockValidators`.
fn points_to_array_bytes(points: &[ECPoint]) -> CoreResult<Vec<u8>> {
    let array = StackItem::from_array(
        points
            .iter()
            .map(|p| StackItem::from_byte_string(p.to_bytes()))
            .collect::<Vec<_>>(),
    );
    BinarySerializer::serialize(&array, &ExecutionEngineLimits::default())
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
fn compute_committee_address(snapshot: &DataCache) -> CoreResult<UInt160> {
    let points = read_committee_points(snapshot)?;
    if points.is_empty() {
        return Err(CoreError::invalid_operation("committee is empty"));
    }
    let m = committee_threshold(points.len());
    let script = neo_redeem_script::multi_sig_redeem_script_from_points(m, &points)
        .map_err(|e| CoreError::invalid_operation(format!("committee multisig script: {e}")))?;
    Ok(UInt160::from_script(&script))
}

/// C# `GetAccountState`: the stored `NeoAccountState` struct bytes under
/// `Prefix_Account ++ account`, or `None` when the account has no entry. The
/// stored value is already the BinarySerializer-encoded struct (balance,
/// balanceHeight, voteTo, lastGasPerVote), which is exactly the Array/Struct
/// return shape — so it is returned as-is (the same pattern as
/// `getDesignatedByRole` / `getContract`).
fn read_account_state(snapshot: &DataCache, account: &UInt160) -> Option<Vec<u8>> {
    let mut key_bytes = vec![crate::NEP17_PREFIX_ACCOUNT];
    key_bytes.extend_from_slice(&account.to_bytes());
    let key = StorageKey::new(NeoToken::ID, key_bytes);
    snapshot.get(&key).map(|item| item.value_bytes().into_owned())
}

static NEO_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    let read_states = CallFlags::READ_STATES.bits();
    let int = ContractParameterType::Integer;
    vec![
        // NEP-17 metadata: `[ContractMethod]` with no CpuFee -> fee 0, no flags.
        NativeMethod::new("symbol".into(), 0, true, 0, vec![], ContractParameterType::String),
        NativeMethod::new("decimals".into(), 0, true, 0, vec![], int),
        // NEP-17 state reads: CpuFee 1<<15, RequiredCallFlags ReadStates.
        NativeMethod::new("totalSupply".into(), 1 << 15, true, read_states, vec![], int),
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
        .with_parameter_names(["from", "to", "amount", "data"]),
        // Governance reads.
        NativeMethod::new("getGasPerBlock".into(), 1 << 15, true, read_states, vec![], int),
        NativeMethod::new("getRegisterPrice".into(), 1 << 15, true, read_states, vec![], int),
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
            vec![ContractParameterType::Hash160, ContractParameterType::PublicKey],
            ContractParameterType::Boolean,
        )
        .with_parameter_names(["account", "voteTo"])
        .with_deprecated_in(Hardfork::HfEchidna),
        NativeMethod::new(
            "vote".into(),
            1 << 16,
            false,
            CallFlags::STATES.bits() | CallFlags::ALLOW_NOTIFY.bits(),
            vec![ContractParameterType::Hash160, ContractParameterType::PublicKey],
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
        *NEO_HASH
    }

    fn name(&self) -> &str {
        "NeoToken"
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
    fn supported_standards(
        &self,
        settings: &ProtocolSettings,
        block_height: u32,
    ) -> Vec<String> {
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
        Ok(Some(compute_committee_address(snapshot)?))
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
            StorageKey::new(Self::ID, vec![PREFIX_COMMITTEE]),
            StorageItem::from_bytes(encode_committee(&members)?),
        );
        // C# `new StorageItem(Array.Empty<byte>())` — BigInteger zero is stored
        // as empty bytes.
        snapshot.add(voters_count_key(), StorageItem::from_bytes(Vec::new()));
        let mut gas_record_key = vec![PREFIX_GAS_PER_BLOCK];
        gas_record_key.extend_from_slice(&0u32.to_be_bytes());
        snapshot.add(
            StorageKey::new(Self::ID, gas_record_key),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_GAS_PER_BLOCK,
            ))),
        );
        snapshot.add(
            StorageKey::new(Self::ID, vec![PREFIX_REGISTER_PRICE]),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_REGISTER_PRICE,
            ))),
        );
        let bft = bft_address(&standby_validators)?;
        neo_mint(engine, &bft, &BigInt::from(NEO_TOTAL_AMOUNT), false)
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
        if !should_refresh_committee(block_index, committee_count) {
            return Ok(());
        }
        let settings = engine.protocol_settings().clone();
        let snapshot = engine.snapshot_cache();
        // C# `GetAndChange(Prefix_Committee)!` — a missing cache faults.
        let prev_committee = read_committee_with_votes(&snapshot)?;
        let new_committee = compute_committee_members(&snapshot, &settings)?;
        snapshot.update(
            StorageKey::new(Self::ID, vec![PREFIX_COMMITTEE]),
            StorageItem::from_bytes(encode_committee(&new_committee)?),
        );
        // Hardfork check for https://github.com/neo-project/neo/pull/3158.
        if engine.is_hardfork_enabled(Hardfork::HfCockatrice) {
            let prev_keys: Vec<&ECPoint> = prev_committee.iter().map(|(point, _)| point).collect();
            let new_keys: Vec<&ECPoint> = new_committee.iter().map(|(point, _)| point).collect();
            if prev_keys != new_keys {
                let to_array = |keys: &[&ECPoint]| {
                    StackItem::from_array(
                        keys.iter()
                            .map(|point| StackItem::from_byte_string(point.to_bytes()))
                            .collect::<Vec<_>>(),
                    )
                };
                engine
                    .send_notification(
                        Self::script_hash(),
                        "CommitteeChanged".to_string(),
                        vec![to_array(&prev_keys), to_array(&new_keys)],
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
        let gas_per_block = gas_per_block_at(&snapshot, block_index.saturating_add(1));
        let committee = read_committee_with_votes(&snapshot)?;
        let member_index = (block_index % (committee_count as u32)) as usize;
        let (member, _) = committee.get(member_index).ok_or_else(|| {
            CoreError::invalid_operation("NeoToken::post_persist: committee cache too small")
        })?;
        let account =
            UInt160::from_script(&Contract::create_signature_redeem_script(member.clone()));
        let committee_reward = &gas_per_block * COMMITTEE_REWARD_RATIO / 100;
        crate::gas_token::gas_mint(engine, &account, &committee_reward, false)?;

        // Record the cumulative reward of the voters of the committee.
        if should_refresh_committee(block_index, committee_count) {
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
                    let mut key_bytes = vec![PREFIX_VOTER_REWARD_PER_COMMITTEE];
                    key_bytes.extend_from_slice(&member.to_bytes());
                    let key = StorageKey::new(Self::ID, key_bytes);
                    // C# `GetAndChange(key, () => new StorageItem(0)).Add(...)`.
                    let accumulated = voter_reward_per_committee(&snapshot, member) + reward_per_neo;
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
                let snapshot = engine.snapshot_cache();
                let total =
                    crate::read_storage_int(&snapshot, Self::ID, crate::NEP17_PREFIX_TOTAL_SUPPLY, 0)?;
                Ok(BigInt::from(total).to_signed_bytes_le())
            }
            "balanceOf" => {
                let account_bytes = args.first().ok_or_else(|| {
                    CoreError::invalid_operation("NeoToken::balanceOf requires an account")
                })?;
                let account = UInt160::from_bytes(account_bytes).map_err(|e| {
                    CoreError::invalid_operation(format!("NeoToken::balanceOf: bad account: {e}"))
                })?;
                let snapshot = engine.snapshot_cache();
                Ok(crate::read_nep17_balance(&snapshot, Self::ID, &account)?.to_signed_bytes_le())
            }
            "transfer" => {
                // C# FungibleToken.Transfer(from, to, amount, data) with NEO's
                // governance OnBalanceChanging side-effects.
                let from = UInt160::from_bytes(args.first().ok_or_else(|| {
                    CoreError::invalid_operation("NeoToken::transfer requires a from account")
                })?)
                .map_err(|e| CoreError::invalid_operation(format!("NeoToken::transfer: bad from: {e}")))?;
                let to = UInt160::from_bytes(args.get(1).ok_or_else(|| {
                    CoreError::invalid_operation("NeoToken::transfer requires a to account")
                })?)
                .map_err(|e| CoreError::invalid_operation(format!("NeoToken::transfer: bad to: {e}")))?;
                let amount = BigInt::from_signed_bytes_le(args.get(2).ok_or_else(|| {
                    CoreError::invalid_operation("NeoToken::transfer requires an amount")
                })?);
                let data = args.get(3).map(Vec::as_slice).unwrap_or(&[]);
                let caller = engine.get_calling_script_hash().unwrap_or_else(UInt160::zero);
                Ok(vec![u8::from(neo_transfer_core(engine, caller, &from, &to, &amount, data)?)])
            }
            "getGasPerBlock" => {
                let snapshot = engine.snapshot_cache();
                let index = LedgerContract::new().current_index(&snapshot)?.saturating_add(1);
                Ok(gas_per_block_at(&snapshot, index).to_signed_bytes_le())
            }
            "getRegisterPrice" => {
                let snapshot = engine.snapshot_cache();
                Ok(BigInt::from(register_price(&snapshot)?).to_signed_bytes_le())
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
                let authorized = engine.check_committee_witness().map_err(|e| {
                    CoreError::invalid_operation(format!("setRegisterPrice committee check: {e}"))
                })?;
                if !authorized {
                    return Err(CoreError::invalid_operation(
                        "setRegisterPrice requires committee authorization",
                    ));
                }
                put_register_price(&engine.snapshot_cache(), price);
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
                let authorized = engine.check_committee_witness().map_err(|e| {
                    CoreError::invalid_operation(format!("setGasPerBlock committee check: {e}"))
                })?;
                if !authorized {
                    return Err(CoreError::invalid_operation(
                        "setGasPerBlock requires committee authorization",
                    ));
                }
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
                put_gas_per_block(&engine.snapshot_cache(), index, &gas_per_block);
                Ok(Vec::new())
            }
            "getCommittee" => {
                // C# returns ECPoint[] sorted ascending; marshaled as an Array of
                // compressed (33-byte) public-key byte strings.
                let snapshot = engine.snapshot_cache();
                points_to_array_bytes(&committee_sorted(&snapshot)?)
            }
            "getNextBlockValidators" => {
                // First ValidatorsCount committee members (stored order), sorted.
                let count =
                    usize::try_from(engine.protocol_settings().validators_count).unwrap_or(0);
                let snapshot = engine.snapshot_cache();
                points_to_array_bytes(&next_block_validators(&snapshot, count)?)
            }
            "getCandidates" => {
                let snapshot = engine.snapshot_cache();
                // C# `GetCandidatesInternal().Select(...).Take(256).ToArray()`
                // (NeoToken.cs:528): at most the first 256 registered candidates.
                let mut candidates = read_registered_candidates(&snapshot)?;
                candidates.truncate(256);
                candidates_to_array_bytes(&candidates)
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
                Ok(candidate_vote(&snapshot, &pubkey)?.to_signed_bytes_le())
            }
            "registerCandidate" => {
                // C# RegisterCandidate (Echidna V1) + RegisterInternal: charge the
                // register price, then require a witness from the candidate's
                // signature-contract account; create/flip the CandidateState to
                // Registered and (post-Echidna) emit CandidateStateChanged.
                let pubkey_bytes = args.first().ok_or_else(|| {
                    CoreError::invalid_operation("NeoToken::registerCandidate requires a public key")
                })?;
                let pubkey = ECPoint::from_bytes(pubkey_bytes).map_err(|e| {
                    CoreError::invalid_operation(format!(
                        "NeoToken::registerCandidate: bad public key: {e}"
                    ))
                })?;
                // engine.AddFee(GetRegisterPrice * FeeFactor) — charged before the
                // witness check, matching the V1 ordering.
                let price = register_price(&engine.snapshot_cache())?;
                engine
                    .charge_execution_fee(u64::try_from(price).unwrap_or(0))
                    .map_err(|e| {
                        CoreError::invalid_operation(format!("NeoToken::registerCandidate: fee: {e}"))
                    })?;
                Ok(vec![u8::from(register_internal(engine, &pubkey, "registerCandidate")?)])
            }
            "getAllCandidates" => {
                // C# GetAllCandidates (NeoToken.cs:537-545): a StorageIterator
                // over the registered, non-blocked candidate entries with
                // RemovePrefix | DeserializeValues | PickField1 and prefix
                // length 1 — each element is Struct[33-byte pubkey, Votes]. The
                // 4-byte iterator id is decoded back into an InteropInterface
                // by the dispatcher.
                let results = registered_candidate_entries(&engine.snapshot_cache())?;
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
                let price = register_price(&engine.snapshot_cache())?;
                if amount != BigInt::from(price) {
                    return Err(CoreError::invalid_operation(format!(
                        "NeoToken::onNEP17Payment: incorrect GAS amount; expected {price}, received {amount}"
                    )));
                }
                // `data` is an Any param (it arrives BinarySerialized); C#
                // decodes its span as a secp256r1 point, faulting on anything
                // that is not a valid public key (including Null).
                let data = args.get(2).map(Vec::as_slice).unwrap_or(&[]);
                let item =
                    BinarySerializer::deserialize(data, &ExecutionEngineLimits::default(), None)
                        .map_err(|e| {
                            CoreError::invalid_operation(format!(
                                "NeoToken::onNEP17Payment data: {e}"
                            ))
                        })?;
                let pubkey_bytes = item.as_bytes().map_err(|e| {
                    CoreError::invalid_operation(format!("NeoToken::onNEP17Payment data: {e}"))
                })?;
                let pubkey = ECPoint::from_bytes(&pubkey_bytes).map_err(|e| {
                    CoreError::invalid_operation(format!(
                        "NeoToken::onNEP17Payment: bad public key: {e}"
                    ))
                })?;
                if !register_internal(engine, &pubkey, "onNEP17Payment")? {
                    return Err(CoreError::invalid_operation(
                        "NeoToken::onNEP17Payment: failed to register candidate",
                    ));
                }
                // C# `await GAS.Burn(engine, Hash, amount)`: burn the GAS this
                // transfer just credited to the NEO contract's own account.
                crate::gas_token::gas_burn(engine, &Self::script_hash(), &amount)?;
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
                let key = candidate_key(&pubkey);
                let Some(item) = snapshot.get(&key) else {
                    return Ok(vec![1u8]); // not a candidate -> true
                };
                let (registered, votes) = decode_candidate_state(&item.value_bytes())?;
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
                check_candidate(&snapshot, &pubkey, false, &votes)?;
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
                let account_bytes = args.first().ok_or_else(|| {
                    CoreError::invalid_operation("NeoToken::vote requires an account")
                })?;
                let account = UInt160::from_bytes(account_bytes).map_err(|e| {
                    CoreError::invalid_operation(format!("NeoToken::vote: bad account: {e}"))
                })?;
                // voteTo is a nullable PublicKey (bit 1 of the arg null-mask).
                let vote_to_is_null = engine
                    .get_state::<NativeArgNullMask>()
                    .is_some_and(|mask| mask.0 & (1 << 1) != 0);
                let vote_to: Option<ECPoint> = if vote_to_is_null {
                    None
                } else {
                    let bytes = args.get(1).ok_or_else(|| {
                        CoreError::invalid_operation("NeoToken::vote requires a candidate (or null)")
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
                Ok(vec![u8::from(vote_internal(engine, &account, vote_to.as_ref())?)])
            }
            "getCommitteeAddress" => {
                let snapshot = engine.snapshot_cache();
                Ok(compute_committee_address(&snapshot)?.to_bytes())
            }
            "getAccountState" => {
                let account_bytes = args.first().ok_or_else(|| {
                    CoreError::invalid_operation("NeoToken::getAccountState requires an account")
                })?;
                let account = UInt160::from_bytes(account_bytes).map_err(|e| {
                    CoreError::invalid_operation(format!(
                        "NeoToken::getAccountState: bad account: {e}"
                    ))
                })?;
                let snapshot = engine.snapshot_cache();
                // C# returns the NeoAccountState struct, or null (empty payload)
                // when the account has no entry.
                Ok(read_account_state(&snapshot, &account).unwrap_or_default())
            }
            "unclaimedGas" => {
                // C# UnclaimedGas(account, end): `end` must equal the persisting
                // block index (or Ledger.CurrentIndex + 1); compute CalculateBonus
                // for the account's NeoAccountState (zero when it has no entry).
                let account_bytes = args.first().ok_or_else(|| {
                    CoreError::invalid_operation("NeoToken::unclaimedGas requires an account")
                })?;
                let account = UInt160::from_bytes(account_bytes).map_err(|e| {
                    CoreError::invalid_operation(format!("NeoToken::unclaimedGas: bad account: {e}"))
                })?;
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
                let bonus = match read_account_state(&snapshot, &account) {
                    Some(bytes) => {
                        let state = decode_neo_account_state(&bytes)?;
                        calculate_bonus(&snapshot, &state, end)?
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
mod tests {
    use super::*;

    #[test]
    fn native_contract_surface() {
        let c = NeoToken::new();
        assert_eq!(NativeContract::id(&c), -5);
        assert_eq!(NativeContract::name(&c), "NeoToken");
        assert_eq!(NativeContract::hash(&c), *NEO_TOKEN_HASH);
        let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "symbol",
                "decimals",
                "totalSupply",
                "balanceOf",
                "transfer",
                "getGasPerBlock",
                "getRegisterPrice",
                "getCommittee",
                "getCommitteeAddress",
                "getAccountState",
                "unclaimedGas",
                "getNextBlockValidators",
                "getCandidates",
                "getAllCandidates",
                "getCandidateVote",
                "setRegisterPrice",
                "setGasPerBlock",
                "registerCandidate", // V0 (genesis, DeprecatedIn Echidna)
                "registerCandidate", // V1 (ActiveIn Echidna, +AllowNotify)
                "unregisterCandidate", // V0
                "unregisterCandidate", // V1
                "vote",              // V0
                "vote",              // V1
                "onNEP17Payment"
            ]
        );
        // The governance writers: not safe, States, Integer -> Void, CpuFee 1<<15.
        for name in ["setRegisterPrice", "setGasPerBlock"] {
            let w = c.methods().iter().find(|m| m.name == name).unwrap();
            assert!(!w.safe);
            assert_eq!(w.required_call_flags, CallFlags::STATES.bits());
            assert_eq!(w.parameters, vec![ContractParameterType::Integer]);
            assert_eq!(w.return_type, ContractParameterType::Void);
            assert_eq!(w.cpu_fee, 1 << 15);
        }
        // Candidate writers: dual registration (C# V0/V1). V0 is genesis-active
        // with States + DeprecatedIn Echidna; V1 is ActiveIn Echidna and adds
        // AllowNotify. registerCandidate has no manifest CpuFee, unregister is 1<<16.
        let states = CallFlags::STATES.bits();
        let notify_flags = CallFlags::STATES.bits() | CallFlags::ALLOW_NOTIFY.bits();
        for (name, fee) in [("registerCandidate", 0i64), ("unregisterCandidate", 1 << 16)] {
            let versions: Vec<&NativeMethod> =
                c.methods().iter().filter(|m| m.name == name).collect();
            assert_eq!(versions.len(), 2, "{name} is a dual V0/V1 registration");
            let (v0, v1) = (versions[0], versions[1]);
            assert!(!v0.safe && !v1.safe, "{name} not safe");
            assert_eq!(v0.parameters, vec![ContractParameterType::PublicKey], "{name} params");
            assert_eq!(v0.return_type, ContractParameterType::Boolean, "{name} return");
            assert_eq!(v0.cpu_fee, fee, "{name} cpu_fee");
            // V0: genesis-active, States, deprecated at Echidna.
            assert_eq!(v0.required_call_flags, states, "{name} V0 flags");
            assert_eq!(v0.active_in, None, "{name} V0 genesis-active");
            assert_eq!(v0.deprecated_in, Some(Hardfork::HfEchidna), "{name} V0 deprecated");
            // V1: active at Echidna, States|AllowNotify.
            assert_eq!(v1.required_call_flags, notify_flags, "{name} V1 flags");
            assert_eq!(v1.active_in, Some(Hardfork::HfEchidna), "{name} V1 active");
        }
        let acct = c.methods().iter().find(|m| m.name == "getAccountState").unwrap();
        assert_eq!(acct.parameters, vec![ContractParameterType::Hash160]);
        assert_eq!(acct.return_type, ContractParameterType::Array);
        assert_eq!(acct.cpu_fee, 1 << 15);
        let nbv = c.methods().iter().find(|m| m.name == "getNextBlockValidators").unwrap();
        assert_eq!(nbv.return_type, ContractParameterType::Array);
        assert_eq!(nbv.cpu_fee, 1 << 16);
        assert!(nbv.parameters.is_empty());
        let symbol = c.methods().iter().find(|m| m.name == "symbol").unwrap();
        assert!(symbol.safe && symbol.cpu_fee == 0 && symbol.required_call_flags == 0);
        let balance = c.methods().iter().find(|m| m.name == "balanceOf").unwrap();
        assert_eq!(balance.required_call_flags, CallFlags::READ_STATES.bits());

        let committee = c.methods().iter().find(|m| m.name == "getCommittee").unwrap();
        assert_eq!(committee.cpu_fee, 1 << 16);
        assert_eq!(committee.return_type, ContractParameterType::Array);
        assert!(committee.active_in.is_none());
        let addr = c.methods().iter().find(|m| m.name == "getCommitteeAddress").unwrap();
        assert_eq!(addr.cpu_fee, 1 << 16);
        assert_eq!(addr.return_type, ContractParameterType::Hash160);
        assert_eq!(addr.active_in, Some(Hardfork::HfCockatrice));
        // getAllCandidates: safe ungated iterator reader (ReadStates, CpuFee
        // 1<<22, no params, InteropInterface).
        let all_cand = c.methods().iter().find(|m| m.name == "getAllCandidates").unwrap();
        assert!(all_cand.safe);
        assert_eq!(all_cand.cpu_fee, 1 << 22);
        assert_eq!(all_cand.required_call_flags, CallFlags::READ_STATES.bits());
        assert!(all_cand.parameters.is_empty());
        assert_eq!(all_cand.return_type, ContractParameterType::InteropInterface);
        assert!(all_cand.active_in.is_none());
        // onNEP17Payment: Echidna-gated candidate registration via GAS payment
        // — States|AllowNotify -> not safe, Void, no manifest CpuFee.
        let pay = c.methods().iter().find(|m| m.name == "onNEP17Payment").unwrap();
        assert!(!pay.safe);
        assert_eq!(pay.cpu_fee, 0);
        assert_eq!(pay.required_call_flags, notify_flags);
        assert_eq!(
            pay.parameters,
            vec![
                ContractParameterType::Hash160,
                ContractParameterType::Integer,
                ContractParameterType::Any
            ]
        );
        assert_eq!(pay.return_type, ContractParameterType::Void);
        assert_eq!(pay.active_in, Some(Hardfork::HfEchidna));
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// Stores a committee cache (Array of `Struct[pubkey, votes]`) under
    /// `Prefix_Committee`, mirroring C# `CachedCommittee.ToStackItem`.
    fn seed_committee(cache: &DataCache, points: &[ECPoint]) {
        use neo_storage::StorageItem;
        let array = StackItem::from_array(
            points
                .iter()
                .map(|p| {
                    StackItem::from_struct(vec![
                        StackItem::from_byte_string(p.to_bytes()),
                        StackItem::from_int(0),
                    ])
                })
                .collect::<Vec<_>>(),
        );
        let bytes =
            BinarySerializer::serialize(&array, &ExecutionEngineLimits::default()).unwrap();
        cache.add(
            StorageKey::new(NeoToken::ID, vec![PREFIX_COMMITTEE]),
            StorageItem::from_bytes(bytes),
        );
    }

    fn sample_committee() -> Vec<ECPoint> {
        // Three valid secp256r1 public keys (Neo N3 standby validators).
        [
            "03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c",
            "02df48f60e8f3e01c48ff40b9b7f1310d7a8b2a193188befe1c2e3df740e895093",
            "03b8d9d5771d8f513aa0869b9cc8d50986403b78c6da36890638c3d46a5adce04a",
        ]
        .iter()
        .map(|h| ECPoint::from_bytes(&hex(h)).unwrap())
        .collect()
    }

    #[test]
    fn committee_threshold_is_majority() {
        // m = n - (n - 1) / 2.
        assert_eq!(committee_threshold(1), 1);
        assert_eq!(committee_threshold(3), 2);
        assert_eq!(committee_threshold(4), 3);
        assert_eq!(committee_threshold(7), 4);
        assert_eq!(committee_threshold(21), 11);
    }

    #[test]
    fn committee_read_decodes_and_sorts() {
        let cache = DataCache::new(false);
        let points = sample_committee();
        seed_committee(&cache, &points);

        // Decoded points round-trip (stored order).
        let read = read_committee_points(&cache).unwrap();
        assert_eq!(read, points);

        // getCommittee returns them sorted ascending (C# OrderBy).
        let mut expected = points.clone();
        expected.sort();
        assert_eq!(committee_sorted(&cache).unwrap(), expected);
    }

    #[test]
    fn next_block_validators_takes_count_then_sorts() {
        let cache = DataCache::new(false);
        let points = sample_committee(); // 3 stored points
        seed_committee(&cache, &points);

        // Take the first 2 (stored order), then sort ascending.
        let result = next_block_validators(&cache, 2).unwrap();
        let mut expected: Vec<ECPoint> = points[..2].to_vec();
        expected.sort();
        assert_eq!(result, expected);

        // A count >= committee size returns all members, sorted.
        let mut all_expected = points.clone();
        all_expected.sort();
        assert_eq!(next_block_validators(&cache, 10).unwrap(), all_expected);
    }

    #[test]
    fn candidates_filters_registered_and_decodes_votes() {
        use neo_storage::StorageItem;
        let cache = DataCache::new(false);
        let points = sample_committee(); // 3 valid points

        // p0 registered w/ 100 votes, p1 unregistered, p2 registered w/ 50 votes.
        for (pk, registered, votes) in [
            (&points[0], true, 100i64),
            (&points[1], false, 0),
            (&points[2], true, 50),
        ] {
            let state = StackItem::from_struct(vec![
                StackItem::from_bool(registered),
                StackItem::from_int(votes),
            ]);
            let bytes =
                BinarySerializer::serialize(&state, &ExecutionEngineLimits::default()).unwrap();
            let mut key = vec![PREFIX_CANDIDATE];
            key.extend_from_slice(&pk.to_bytes());
            cache.add(StorageKey::new(NeoToken::ID, key), StorageItem::from_bytes(bytes));
        }

        let candidates = read_registered_candidates(&cache).unwrap();
        // Only the two registered candidates are returned.
        assert_eq!(candidates.len(), 2);
        let by_key: std::collections::HashMap<Vec<u8>, BigInt> =
            candidates.iter().map(|(pk, v)| (pk.to_bytes(), v.clone())).collect();
        assert_eq!(by_key.get(&points[0].to_bytes()), Some(&BigInt::from(100)));
        assert_eq!(by_key.get(&points[2].to_bytes()), Some(&BigInt::from(50)));
        assert!(!by_key.contains_key(&points[1].to_bytes()));
    }

    #[test]
    fn zero_bigint_storage_writes_match_csharp_empty_bytes() {
        // C# StorageItem stores BigInteger.ToByteArrayStandard(): EMPTY bytes for
        // zero (num-bigint's to_signed_bytes_le would give [0x00] — a raw stored-
        // bytes / state-root divergence). _votersCount can legitimately reach 0
        // when the last voter un-votes; gasPerBlock can be set to 0.
        let cache = DataCache::new(false);
        write_voters_count(&cache, &BigInt::from(0));
        let stored = cache.get(&voters_count_key()).expect("entry written");
        assert!(stored.value_bytes().is_empty(), "zero votersCount stores empty bytes");
        assert_eq!(read_voters_count(&cache), BigInt::from(0));

        put_gas_per_block(&cache, 7, &BigInt::from(0));
        let mut key = vec![PREFIX_GAS_PER_BLOCK];
        key.extend_from_slice(&7u32.to_be_bytes());
        let stored = cache.get(&StorageKey::new(NeoToken::ID, key)).expect("entry written");
        assert!(stored.value_bytes().is_empty(), "zero gasPerBlock stores empty bytes");

        // Non-zero values keep the signed-LE form.
        write_voters_count(&cache, &BigInt::from(300));
        let stored = cache.get(&voters_count_key()).expect("entry written");
        assert_eq!(stored.value_bytes().as_ref(), BigInt::from(300).to_signed_bytes_le());
    }

    #[test]
    fn calculate_bonus_matches_csharp_testcalculatebonus() {
        // C# UT_NeoToken.TestCalculateBonus "Normal 1": balance 100, no vote,
        // BalanceHeight 0, the genesis 5-GAS gasPerBlock record at index 0, end
        // 100 -> 100 * (5e8 * 100) * 10 / 100 / 100_000_000 = 5000.
        let cache = DataCache::new(false);
        put_gas_per_block(&cache, 0, &BigInt::from(DEFAULT_GAS_PER_BLOCK));
        let holder = NeoAccountStateView {
            balance: BigInt::from(100),
            balance_height: 0,
            vote_to: None,
            last_gas_per_vote: BigInt::from(0),
        };
        assert_eq!(calculate_bonus(&cache, &holder, 100).unwrap(), BigInt::from(5000));

        // balance == 0 -> 0; BalanceHeight >= end -> 0; balance < 0 -> fault.
        let zero = NeoAccountStateView { balance: BigInt::from(0), ..clone_view(&holder) };
        assert_eq!(calculate_bonus(&cache, &zero, 100).unwrap(), BigInt::from(0));
        let future = NeoAccountStateView { balance_height: 100, ..clone_view(&holder) };
        assert_eq!(calculate_bonus(&cache, &future, 100).unwrap(), BigInt::from(0));
        let negative = NeoAccountStateView { balance: BigInt::from(-100), ..clone_view(&holder) };
        assert!(calculate_bonus(&cache, &negative, 100).is_err());
    }

    fn clone_view(v: &NeoAccountStateView) -> NeoAccountStateView {
        NeoAccountStateView {
            balance: v.balance.clone(),
            balance_height: v.balance_height,
            vote_to: v.vote_to.clone(),
            last_gas_per_vote: v.last_gas_per_vote.clone(),
        }
    }

    #[test]
    fn neo_account_state_decodes_struct_fields() {
        // Struct[Balance, BalanceHeight, VoteTo(null), LastGasPerVote].
        let item = StackItem::from_struct(vec![
            StackItem::from_int(BigInt::from(100)),
            StackItem::from_int(BigInt::from(42)),
            StackItem::null(),
            StackItem::from_int(BigInt::from(7)),
        ]);
        let bytes = BinarySerializer::serialize(&item, &ExecutionEngineLimits::default()).unwrap();
        let state = decode_neo_account_state(&bytes).unwrap();
        assert_eq!(state.balance, BigInt::from(100));
        assert_eq!(state.balance_height, 42);
        assert!(state.vote_to.is_none());
        assert_eq!(state.last_gas_per_vote, BigInt::from(7));
    }

    #[test]
    fn candidate_vote_is_votes_or_minus_one() {
        use neo_storage::StorageItem;
        let cache = DataCache::new(false);
        let points = sample_committee();

        // No entry at all -> -1.
        assert_eq!(candidate_vote(&cache, &points[0]).unwrap(), BigInt::from(-1));

        let store = |pk: &ECPoint, registered: bool, votes: i64| {
            let state = StackItem::from_struct(vec![
                StackItem::from_bool(registered),
                StackItem::from_int(votes),
            ]);
            let bytes =
                BinarySerializer::serialize(&state, &ExecutionEngineLimits::default()).unwrap();
            let mut key = vec![PREFIX_CANDIDATE];
            key.extend_from_slice(&pk.to_bytes());
            cache.add(StorageKey::new(NeoToken::ID, key), StorageItem::from_bytes(bytes));
        };

        // Registered -> its votes; unregistered -> -1 even with a stored entry.
        store(&points[0], true, 250);
        store(&points[1], false, 999);
        assert_eq!(candidate_vote(&cache, &points[0]).unwrap(), BigInt::from(250));
        assert_eq!(candidate_vote(&cache, &points[1]).unwrap(), BigInt::from(-1));
    }

    #[test]
    fn committee_address_matches_multisig_script_hash() {
        let cache = DataCache::new(false);
        let points = sample_committee();
        seed_committee(&cache, &points);

        // For n=3, m=2; the address is the 2-of-3 multisig script hash. The
        // builder sorts the keys the same way C# CreateMultiSigRedeemScript does.
        let script = neo_redeem_script::multi_sig_redeem_script_from_points(2, &points).unwrap();
        assert_eq!(compute_committee_address(&cache).unwrap(), UInt160::from_script(&script));
    }

    #[test]
    fn committee_address_uninitialized_errors() {
        // C# indexes snapshot[Prefix_Committee] and throws when absent.
        let cache = DataCache::new(false);
        assert!(compute_committee_address(&cache).is_err());
        assert!(read_committee_points(&cache).is_err());
    }

    #[test]
    fn committee_address_trait_override_feeds_the_engine_seam() {
        // The `NativeContract::committee_address` override is what the engine's
        // check_committee_witness reaches through the provider seam; it must
        // return the computed address (Some), and fault on a missing committee.
        let cache = DataCache::new(false);
        seed_committee(&cache, &sample_committee());
        let neo = NeoToken::new();
        assert_eq!(
            NativeContract::committee_address(&neo, &cache).unwrap(),
            Some(compute_committee_address(&cache).unwrap())
        );
        assert!(NativeContract::committee_address(&neo, &DataCache::new(false)).is_err());
    }

    #[test]
    fn balance_of_absent_account_is_zero() {
        let cache = DataCache::new(false);
        let account = UInt160::from_bytes(&[2u8; 20]).unwrap();
        assert_eq!(
            crate::read_nep17_balance(&cache, NeoToken::ID, &account).unwrap(),
            BigInt::from(0)
        );
    }

    #[test]
    fn governance_reads_have_defaults_and_read_storage() {
        use neo_storage::StorageItem;
        let cache = DataCache::new(false);

        // Defaults when unset: 1000 GAS register price, 5 GAS per block.
        assert_eq!(register_price(&cache).unwrap(), DEFAULT_REGISTER_PRICE);
        assert_eq!(gas_per_block_at(&cache, 100), BigInt::from(DEFAULT_GAS_PER_BLOCK));

        // register price reads the prefix-13 BigInteger.
        cache.add(
            StorageKey::new(NeoToken::ID, vec![PREFIX_REGISTER_PRICE]),
            StorageItem::from_bytes(BigInt::from(500 * 100_000_000i64).to_signed_bytes_le()),
        );
        assert_eq!(register_price(&cache).unwrap(), 500 * 100_000_000);

        // gas-per-block backward seek: record at index 10 applies from 10 on.
        let mut key = vec![PREFIX_GAS_PER_BLOCK];
        key.extend_from_slice(&10u32.to_be_bytes());
        cache.add(
            StorageKey::new(NeoToken::ID, key),
            StorageItem::from_bytes(BigInt::from(3 * 100_000_000i64).to_signed_bytes_le()),
        );
        assert_eq!(gas_per_block_at(&cache, 9), BigInt::from(DEFAULT_GAS_PER_BLOCK));
        assert_eq!(gas_per_block_at(&cache, 20), BigInt::from(3 * 100_000_000i64));
    }

    #[test]
    fn set_register_price_write_round_trips() {
        // The setRegisterPrice storage effect (overwrite Prefix_RegisterPrice) is
        // observed by the getRegisterPrice reader, matching C#
        // GetAndChange(_registerPrice).Set(price).
        let cache = DataCache::new(false);
        assert_eq!(register_price(&cache).unwrap(), DEFAULT_REGISTER_PRICE);
        put_register_price(&cache, 500 * 100_000_000);
        assert_eq!(register_price(&cache).unwrap(), 500 * 100_000_000);
        // Overwrite (GetAndChange semantics), not insert-once.
        put_register_price(&cache, 2000 * 100_000_000);
        assert_eq!(register_price(&cache).unwrap(), 2000 * 100_000_000);
    }

    #[test]
    fn set_gas_per_block_write_round_trips() {
        // The setGasPerBlock storage effect (a Prefix_GasPerBlock record at a
        // big-endian uint index) is observed by gas_per_block_at's backward seek:
        // a record at index N applies from N onward, never before.
        let cache = DataCache::new(false);
        assert_eq!(gas_per_block_at(&cache, 50), BigInt::from(DEFAULT_GAS_PER_BLOCK));

        put_gas_per_block(&cache, 10, &BigInt::from(7 * 100_000_000i64));
        assert_eq!(gas_per_block_at(&cache, 9), BigInt::from(DEFAULT_GAS_PER_BLOCK));
        assert_eq!(gas_per_block_at(&cache, 10), BigInt::from(7 * 100_000_000i64));
        assert_eq!(gas_per_block_at(&cache, 100), BigInt::from(7 * 100_000_000i64));

        // Overwrite at the same index (GetAndChange semantics).
        put_gas_per_block(&cache, 10, &BigInt::from(2 * 100_000_000i64));
        assert_eq!(gas_per_block_at(&cache, 10), BigInt::from(2 * 100_000_000i64));
    }

    #[test]
    fn account_state_returns_stored_struct_or_none() {
        use neo_storage::StorageItem;
        let cache = DataCache::new(false);
        let account = UInt160::from_bytes(&[5u8; 20]).unwrap();

        // Absent -> None (invoke maps it to an empty payload = null).
        assert!(read_account_state(&cache, &account).is_none());

        // Store a NeoAccountState struct [balance, height, voteTo(Null),
        // lastGasPerVote] and read its raw bytes back unchanged.
        let state = StackItem::from_struct(vec![
            StackItem::from_int(123),
            StackItem::from_int(7),
            StackItem::null(),
            StackItem::from_int(0),
        ]);
        let bytes = BinarySerializer::serialize(&state, &ExecutionEngineLimits::default()).unwrap();
        let mut key_bytes = vec![crate::NEP17_PREFIX_ACCOUNT];
        key_bytes.extend_from_slice(&account.to_bytes());
        cache.add(
            StorageKey::new(NeoToken::ID, key_bytes),
            StorageItem::from_bytes(bytes.clone()),
        );
        assert_eq!(read_account_state(&cache, &account), Some(bytes.clone()));
        // The returned bytes deserialize to the 4-field struct.
        match BinarySerializer::deserialize(&bytes, &ExecutionEngineLimits::default(), None).unwrap()
        {
            StackItem::Struct(s) => assert_eq!(s.items().len(), 4),
            other => panic!("expected Struct, got {other:?}"),
        }
    }
}

/// Reusable harness proving a witness-gated native call can be exercised
/// end-to-end in a unit test — the prerequisite for verifying NeoToken's
/// governance writers (`registerCandidate` / `vote` / …), which all gate on
/// `engine.check_witness_hash`. A direct `invoke(...)` call has no execution
/// context, so the witness check only works through the VM: load a script that
/// reaches `System.Runtime.CheckWitness` into an `ApplicationEngine` whose
/// script container is a transaction carrying the relevant signer.
#[cfg(test)]
mod witness_harness_tests {
    use neo_config::ProtocolSettings;
    use neo_data_cache::DataCache;
    use neo_execution::ApplicationEngine;
    use neo_payloads::signer::Signer;
    use neo_payloads::transaction::Transaction;
    use neo_payloads::witness::Witness;
    use neo_primitives::{CallFlags, TriggerType, UInt160, Verifiable, WitnessScope};
    use neo_script_builder::ScriptBuilder;
    use neo_vm_rs::VmState;
    use std::sync::Arc;

    /// Builds a script that calls `System.Runtime.CheckWitness(hash)`.
    fn check_witness_script(hash: &UInt160) -> Vec<u8> {
        let mut builder = ScriptBuilder::new();
        builder.emit_push(&hash.to_array());
        builder
            .emit_syscall("System.Runtime.CheckWitness")
            .expect("CheckWitness syscall");
        builder.to_array()
    }

    /// Runs `script` through a fresh Application-trigger engine whose container
    /// is a transaction signed (Global scope) by each hash in `signers`.
    /// Returns the final VM state and the boolean on top of the result stack.
    fn run_signed(script: Vec<u8>, signers: &[UInt160]) -> (VmState, bool) {
        let mut tx = Transaction::new();
        tx.set_signers(
            signers
                .iter()
                .map(|h| Signer::new(*h, WitnessScope::GLOBAL))
                .collect(),
        );
        tx.set_witnesses(signers.iter().map(|_| Witness::empty()).collect());
        let container: Arc<dyn Verifiable> = Arc::new(tx);

        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            Some(container),
            Arc::new(DataCache::new(false)),
            None,
            ProtocolSettings::default(),
            10_000_000,
            None,
        )
        .expect("engine builds");
        engine
            .load_script(script, CallFlags::READ_ONLY, None)
            .expect("script loads");
        let state = engine.execute_allow_fault();
        let top = engine
            .result_stack()
            .peek(0)
            .ok()
            .and_then(|item| item.as_bool().ok())
            .unwrap_or(false);
        (state, top)
    }

    #[test]
    fn checkwitness_true_for_signer_false_for_others() {
        let signer = UInt160::from_bytes(&[0x11; 20]).unwrap();
        let stranger = UInt160::from_bytes(&[0x22; 20]).unwrap();

        // The signed hash → CheckWitness true.
        let (state, ok) = run_signed(check_witness_script(&signer), &[signer]);
        assert_eq!(state, VmState::HALT, "script must HALT");
        assert!(ok, "CheckWitness must be true for a Global-scope signer");

        // A different hash → CheckWitness false (still a clean HALT).
        let (state2, ok2) = run_signed(check_witness_script(&stranger), &[signer]);
        assert_eq!(state2, VmState::HALT, "script must HALT");
        assert!(!ok2, "CheckWitness must be false for a non-signer");
    }
}

/// End-to-end verification of the candidate-registration writers through the VM
/// (the witness-gated script-execution path proven by `witness_harness_tests`):
/// a script `System.Contract.Call`s NeoToken with the candidate as signer, and
/// the resulting candidate state is asserted against the shared snapshot.
#[cfg(test)]
mod governance_writer_tests {
    use super::*;
    use neo_config::ProtocolSettings;
    use neo_execution::contract_state::ContractState;
    use neo_execution::native_contract::build_native_contract_state;
    use neo_execution::{ApplicationEngine, Contract};
    use neo_io::{BinaryWriter, Serializable};
    use neo_payloads::signer::Signer;
    use neo_payloads::transaction::Transaction;
    use neo_payloads::witness::Witness;
    use neo_primitives::{CallFlags, TriggerType, Verifiable, WitnessScope};
    use neo_script_builder::ScriptBuilder;
    use neo_vm_rs::VmState;
    use std::sync::Arc;

    /// ContractManagement per-contract storage prefix (mirrors asset_descriptor).
    const CM_PREFIX_CONTRACT: u8 = 8;

    fn candidate_pubkey() -> ECPoint {
        // A valid secp256r1 public key (a Neo N3 standby validator).
        ECPoint::from_bytes(
            &hex::decode("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c")
                .unwrap(),
        )
        .unwrap()
    }

    fn deploy_native(cache: &DataCache, state: &ContractState) {
        let mut key = vec![CM_PREFIX_CONTRACT];
        key.extend_from_slice(&state.hash.to_bytes());
        cache.add(
            StorageKey::new(crate::ContractManagement::ID, key),
            StorageItem::from_bytes(
                state.serialize_contract_record().expect("record bytes"),
            ),
        );
    }

    /// Runs `method(pubkey)` on NeoToken via System.Contract.Call, signed (Global)
    /// by `signer`, against the shared `snapshot`. Returns the final VM state.
    fn call(snapshot: Arc<DataCache>, signer: UInt160, pubkey: &[u8], method: &str) -> VmState {
        let mut tx = Transaction::new();
        tx.set_signers(vec![Signer::new(signer, WitnessScope::GLOBAL)]);
        tx.set_witnesses(vec![Witness::empty()]);
        let container: Arc<dyn Verifiable> = Arc::new(tx);

        let mut builder = ScriptBuilder::new();
        builder.emit_push(pubkey);
        builder.emit_push_int(1);
        builder.emit_pack();
        builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
        builder.emit_push(method.as_bytes());
        builder.emit_push(&NeoToken::script_hash().to_array());
        builder
            .emit_syscall("System.Contract.Call")
            .expect("System.Contract.Call");

        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            Some(container),
            snapshot,
            None,
            ProtocolSettings::default(),
            2000_00000000, // > the 1000-GAS register price
            None,
        )
        .expect("engine builds");
        engine
            .load_script(builder.to_array(), CallFlags::ALL, None)
            .expect("script loads");
        engine.execute_allow_fault()
    }

    fn seeded_snapshot() -> Arc<DataCache> {
        crate::install();
        let cache = DataCache::new(false);
        let neo_state = build_native_contract_state(&NeoToken, &ProtocolSettings::default(), 0);
        deploy_native(&cache, &neo_state);
        Arc::new(cache)
    }

    #[test]
    fn register_then_unregister_candidate_round_trip() {
        let pubkey = candidate_pubkey();
        let pubkey_bytes = pubkey.to_bytes();
        let account =
            UInt160::from_script(&Contract::create_signature_redeem_script(pubkey.clone()));
        let snapshot = seeded_snapshot();

        // Register (signed by the candidate's account) → Registered with 0 votes.
        let state = call(Arc::clone(&snapshot), account, &pubkey_bytes, "registerCandidate");
        assert_eq!(state, VmState::HALT, "registerCandidate must HALT");
        let item = snapshot
            .get(&candidate_key(&pubkey))
            .expect("candidate entry written");
        let (registered, votes) = decode_candidate_state(&item.value_bytes()).unwrap();
        assert!(registered, "candidate is Registered");
        assert_eq!(votes, BigInt::from(0));
        assert_eq!(read_registered_candidates(&snapshot).unwrap().len(), 1);

        // Unregister → the zero-vote entry is removed.
        let state2 = call(Arc::clone(&snapshot), account, &pubkey_bytes, "unregisterCandidate");
        assert_eq!(state2, VmState::HALT, "unregisterCandidate must HALT");
        assert!(
            snapshot.get(&candidate_key(&pubkey)).is_none(),
            "zero-vote candidate entry removed"
        );
    }

    #[test]
    fn unregister_candidate_deletes_stale_voter_reward_entry() {
        // C# `CheckCandidate` (NeoToken.cs:191): unregistering a candidate with
        // no remaining votes deletes BOTH the candidate AND the
        // `Prefix_VoterRewardPerCommittee` entry. A candidate that accrued
        // committee voter rewards then lost all votes must not leave a stale
        // reward record (a state-root divergence vs C#).
        let pubkey = candidate_pubkey();
        let pubkey_bytes = pubkey.to_bytes();
        let account =
            UInt160::from_script(&Contract::create_signature_redeem_script(pubkey.clone()));
        let snapshot = seeded_snapshot();

        // Registered candidate, 0 votes, with a (stale) accrued voter-reward entry.
        snapshot.update(
            candidate_key(&pubkey),
            StorageItem::from_bytes(encode_candidate_state(true, &BigInt::from(0)).unwrap()),
        );
        let mut reward_key_bytes = vec![PREFIX_VOTER_REWARD_PER_COMMITTEE];
        reward_key_bytes.extend_from_slice(&pubkey.to_bytes());
        let reward_key = StorageKey::new(NeoToken::ID, reward_key_bytes);
        snapshot.add(
            reward_key.clone(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(123_456))),
        );

        let state = call(Arc::clone(&snapshot), account, &pubkey_bytes, "unregisterCandidate");
        assert_eq!(state, VmState::HALT, "unregisterCandidate must HALT");
        assert!(
            snapshot.get(&candidate_key(&pubkey)).is_none(),
            "candidate entry removed"
        );
        assert!(
            snapshot.get(&reward_key).is_none(),
            "stale voter-reward entry removed"
        );
    }

    #[test]
    fn register_candidate_requires_the_candidate_witness() {
        let pubkey = candidate_pubkey();
        let pubkey_bytes = pubkey.to_bytes();
        let wrong = UInt160::from_bytes(&[0x09; 20]).unwrap();
        let snapshot = seeded_snapshot();

        // Signed by the wrong account → no candidate is registered.
        let state = call(Arc::clone(&snapshot), wrong, &pubkey_bytes, "registerCandidate");
        assert_eq!(state, VmState::HALT);
        assert!(
            snapshot.get(&candidate_key(&pubkey)).is_none(),
            "no candidate registered without its witness"
        );
    }

    #[test]
    fn vote_assigns_weight_distributes_gas_and_records_target() {
        use neo_payloads::{Block, BlockHeader};

        let candidate = candidate_pubkey();
        let voter = UInt160::from_bytes(&[0x07; 20]).unwrap();

        crate::install();
        let cache = DataCache::new(false);
        deploy_native(&cache, &build_native_contract_state(&NeoToken, &ProtocolSettings::default(), 0));
        // A registered candidate (0 votes), the voter holding 100 NEO since height
        // 0, and the genesis 5-GAS gasPerBlock record (so CalculateBonus is nonzero).
        cache.update(
            candidate_key(&candidate),
            StorageItem::from_bytes(encode_candidate_state(true, &BigInt::from(0)).unwrap()),
        );
        let voter_state = NeoAccountStateView {
            balance: BigInt::from(100),
            balance_height: 0,
            vote_to: None,
            last_gas_per_vote: BigInt::from(0),
        };
        cache.update(
            neo_account_key(&voter),
            StorageItem::from_bytes(encode_neo_account_state(&voter_state).unwrap()),
        );
        put_gas_per_block(&cache, 0, &BigInt::from(DEFAULT_GAS_PER_BLOCK));
        let snapshot = Arc::new(cache);

        // vote(voter, candidate), signed by the voter, in a block at index 100.
        let mut tx = Transaction::new();
        tx.set_signers(vec![Signer::new(voter, WitnessScope::GLOBAL)]);
        tx.set_witnesses(vec![Witness::empty()]);
        let container: Arc<dyn Verifiable> = Arc::new(tx);
        let mut builder = ScriptBuilder::new();
        builder.emit_push(&candidate.to_bytes()); // voteTo (arg 1, deeper)
        builder.emit_push(&voter.to_array()); // account (arg 0, top)
        builder.emit_push_int(2);
        builder.emit_pack();
        builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
        builder.emit_push("vote".as_bytes());
        builder.emit_push(&NeoToken::script_hash().to_array());
        builder.emit_syscall("System.Contract.Call").expect("call");

        let mut header = BlockHeader::default();
        header.set_index(100);
        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            Some(container),
            Arc::clone(&snapshot),
            Some(Block::from_parts(header, vec![])),
            ProtocolSettings::default(),
            2000_00000000,
            None,
        )
        .expect("engine builds");
        engine.load_script(builder.to_array(), CallFlags::ALL, None).expect("loads");
        assert_eq!(engine.execute_allow_fault(), VmState::HALT, "vote must HALT");

        // The candidate gained the voter's 100-NEO weight.
        let (_, cand_votes) =
            decode_candidate_state(&snapshot.get(&candidate_key(&candidate)).unwrap().value_bytes())
                .unwrap();
        assert_eq!(cand_votes, BigInt::from(100));
        // The voter's VoteTo now points at the candidate.
        let acct = decode_neo_account_state(&read_account_state(&snapshot, &voter).unwrap()).unwrap();
        assert_eq!(acct.vote_to, Some(candidate));
        // DistributeGas minted the 5000-datoshi CalculateBonus reward to the voter.
        let mut gas_key_bytes = vec![crate::NEP17_PREFIX_ACCOUNT];
        gas_key_bytes.extend_from_slice(&voter.to_bytes());
        let gas_item = snapshot
            .get(&StorageKey::new(crate::GasToken::ID, gas_key_bytes))
            .expect("voter GAS account written");
        let decoded =
            BinarySerializer::deserialize(&gas_item.value_bytes(), &ExecutionEngineLimits::default(), None)
                .unwrap();
        let StackItem::Struct(fields) = decoded else {
            panic!("GAS account is not a struct");
        };
        let gas_balance = fields.items().first().unwrap().as_int().unwrap();
        assert_eq!(gas_balance, BigInt::from(5000));
    }

    #[test]
    fn transfer_moves_balance_and_follows_vote_weight() {
        let candidate = candidate_pubkey();
        let from = UInt160::from_bytes(&[0x0A; 20]).unwrap();
        let to = UInt160::from_bytes(&[0x0B; 20]).unwrap();

        crate::install();
        let cache = DataCache::new(false);
        deploy_native(&cache, &build_native_contract_state(&NeoToken, &ProtocolSettings::default(), 0));
        // Candidate with 100 votes; `from` holds 100 NEO and votes for it.
        cache.update(
            candidate_key(&candidate),
            StorageItem::from_bytes(encode_candidate_state(true, &BigInt::from(100)).unwrap()),
        );
        let from_state = NeoAccountStateView {
            balance: BigInt::from(100),
            balance_height: 0,
            vote_to: Some(candidate.clone()),
            last_gas_per_vote: BigInt::from(0),
        };
        cache.update(
            neo_account_key(&from),
            StorageItem::from_bytes(encode_neo_account_state(&from_state).unwrap()),
        );
        let snapshot = Arc::new(cache);

        // transfer(from, to, 30, <empty>), signed by `from`, no persisting block
        // (so DistributeGas is skipped and the test isolates the transfer/vote
        // bookkeeping).
        let mut tx = Transaction::new();
        tx.set_signers(vec![Signer::new(from, WitnessScope::GLOBAL)]);
        tx.set_witnesses(vec![Witness::empty()]);
        let container: Arc<dyn Verifiable> = Arc::new(tx);
        let mut b = ScriptBuilder::new();
        b.emit_push(&[]); // data (arg 3, pushed deepest)
        b.emit_push_int(30); // amount (arg 2)
        b.emit_push(&to.to_array()); // to (arg 1)
        b.emit_push(&from.to_array()); // from (arg 0, top)
        b.emit_push_int(4);
        b.emit_pack();
        b.emit_push_int(i64::from(CallFlags::ALL.bits()));
        b.emit_push("transfer".as_bytes());
        b.emit_push(&NeoToken::script_hash().to_array());
        b.emit_syscall("System.Contract.Call").expect("call");

        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            Some(container),
            Arc::clone(&snapshot),
            None,
            ProtocolSettings::default(),
            2000_00000000,
            None,
        )
        .expect("engine builds");
        engine.load_script(b.to_array(), CallFlags::ALL, None).expect("loads");
        assert_eq!(engine.execute_allow_fault(), VmState::HALT, "transfer must HALT");

        // Balances moved 30 NEO from `from` to `to`.
        let from_after = decode_neo_account_state(&read_account_state(&snapshot, &from).unwrap()).unwrap();
        assert_eq!(from_after.balance, BigInt::from(70));
        let to_after = decode_neo_account_state(&read_account_state(&snapshot, &to).unwrap()).unwrap();
        assert_eq!(to_after.balance, BigInt::from(30));
        // The candidate's vote weight followed `from`'s reduced balance (100 -> 70).
        let (_, cand_votes) =
            decode_candidate_state(&snapshot.get(&candidate_key(&candidate)).unwrap().value_bytes())
                .unwrap();
        assert_eq!(cand_votes, BigInt::from(70));
    }

    /// The GAS account storage key `(GasToken.ID, [Prefix_Account, account])`.
    fn gas_account_key(account: &UInt160) -> StorageKey {
        let mut key = vec![crate::NEP17_PREFIX_ACCOUNT];
        key.extend_from_slice(&account.to_bytes());
        StorageKey::new(crate::GasToken::ID, key)
    }

    /// Seeds a GAS balance entry (`Struct[Balance]`) and a matching total supply.
    fn seed_gas(cache: &DataCache, account: &UInt160, balance: &BigInt) {
        let state = StackItem::from_struct(vec![StackItem::from_int(balance.clone())]);
        let bytes =
            BinarySerializer::serialize(&state, &ExecutionEngineLimits::default()).unwrap();
        cache.add(gas_account_key(account), StorageItem::from_bytes(bytes));
        cache.add(
            StorageKey::new(crate::GasToken::ID, vec![crate::NEP17_PREFIX_TOTAL_SUPPLY]),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(balance)),
        );
    }

    /// A second valid secp256r1 public key, byte-wise distinct from
    /// [`candidate_pubkey`] (a Neo N3 standby validator).
    fn other_pubkey(index: u8) -> ECPoint {
        let keys = [
            "02df48f60e8f3e01c48ff40b9b7f1310d7a8b2a193188befe1c2e3df740e895093",
            "03b8d9d5771d8f513aa0869b9cc8d50986403b78c6da36890638c3d46a5adce04a",
        ];
        ECPoint::from_bytes(&hex::decode(keys[usize::from(index)]).unwrap()).unwrap()
    }

    /// C# `GetAllCandidates`: the iterator yields `Struct[pubkey, votes]` per
    /// registered candidate (RemovePrefix strips the 1-byte candidate prefix;
    /// PickField1 projects the Votes field), skipping unregistered entries and
    /// candidates whose signature-contract address PolicyContract blocks.
    #[test]
    fn get_all_candidates_iterator_filters_and_projects() {
        crate::install();
        let cache = DataCache::new(false);
        // 02df48… : registered with 7 votes -> the only iterator element.
        let kept = other_pubkey(0);
        cache.add(
            candidate_key(&kept),
            StorageItem::from_bytes(encode_candidate_state(true, &BigInt::from(7)).unwrap()),
        );
        // 03b209… : present but unregistered -> filtered out.
        cache.add(
            candidate_key(&candidate_pubkey()),
            StorageItem::from_bytes(encode_candidate_state(false, &BigInt::from(3)).unwrap()),
        );
        // 03b8d9… : registered but its signature account is blocked -> filtered out.
        let blocked = other_pubkey(1);
        cache.add(
            candidate_key(&blocked),
            StorageItem::from_bytes(encode_candidate_state(true, &BigInt::from(9)).unwrap()),
        );
        let blocked_account =
            UInt160::from_script(&Contract::create_signature_redeem_script(blocked));
        cache.add(
            crate::policy_contract::blocked_account_key(&blocked_account),
            StorageItem::from_bytes(Vec::new()),
        );

        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            None,
            Arc::new(cache),
            None,
            ProtocolSettings::default(),
            10_000_000,
            None,
        )
        .expect("engine builds");

        let result = NativeContract::invoke(&NeoToken::new(), &mut engine, "getAllCandidates", &[])
            .expect("getAllCandidates succeeds");
        let iterator_id = u32::from_le_bytes(result.try_into().expect("4-byte iterator id"));

        assert!(engine.iterator_next(iterator_id).expect("first next"));
        let StackItem::Struct(element) = engine.iterator_value(iterator_id).expect("value") else {
            panic!("iterator element must be a Struct[pubkey, votes]");
        };
        let items = element.items();
        assert_eq!(items[0].as_bytes().unwrap().as_slice(), kept.to_bytes().as_slice());
        assert_eq!(items[1].as_int().unwrap(), BigInt::from(7));
        assert!(!engine.iterator_next(iterator_id).expect("exhausted"), "single element");
    }

    /// Full Echidna flow (C# NeoToken.OnNEP17Payment, NeoToken.cs:374-389):
    /// `GAS.transfer(sender -> NEO, registerPrice, data = pubkey)` registers
    /// the candidate (witnessed by its signature account) and burns the GAS
    /// from NEO's own balance, shrinking the total supply.
    #[test]
    fn on_nep17_payment_registers_candidate_and_burns_gas() {
        let pubkey = candidate_pubkey();
        let candidate_account =
            UInt160::from_script(&Contract::create_signature_redeem_script(pubkey.clone()));
        let sender = UInt160::from_bytes(&[0x07; 20]).unwrap();
        let price = BigInt::from(DEFAULT_REGISTER_PRICE);

        crate::install();
        let cache = DataCache::new(false);
        // onNEP17Payment is Echidna-gated; an unconfigured hardfork is DISABLED
        // for method gating (C# IsHardforkEnabled), so schedule it at genesis.
        let mut settings = ProtocolSettings::default();
        settings.hardforks.insert(Hardfork::HfEchidna, 0);
        deploy_native(&cache, &build_native_contract_state(&NeoToken, &settings, 0));
        deploy_native(&cache, &build_native_contract_state(&crate::GasToken, &settings, 0));
        seed_gas(&cache, &sender, &price);
        let snapshot = Arc::new(cache);

        // Signed by the GAS sender (transfer witness) AND the candidate's
        // signature account (RegisterInternal witness).
        let mut tx = Transaction::new();
        tx.set_signers(vec![
            Signer::new(sender, WitnessScope::GLOBAL),
            Signer::new(candidate_account, WitnessScope::GLOBAL),
        ]);
        tx.set_witnesses(vec![Witness::empty(), Witness::empty()]);
        let container: Arc<dyn Verifiable> = Arc::new(tx);

        // GAS.transfer(sender, NEO, price, pubkey-bytes) — args packed in reverse.
        let mut b = ScriptBuilder::new();
        b.emit_push(&pubkey.to_bytes()); // data (arg 3, pushed deepest)
        b.emit_push_int(DEFAULT_REGISTER_PRICE); // amount (arg 2)
        b.emit_push(&NeoToken::script_hash().to_array()); // to (arg 1)
        b.emit_push(&sender.to_array()); // from (arg 0, top)
        b.emit_push_int(4);
        b.emit_pack();
        b.emit_push_int(i64::from(CallFlags::ALL.bits()));
        b.emit_push("transfer".as_bytes());
        b.emit_push(&crate::GasToken::script_hash().to_array());
        b.emit_syscall("System.Contract.Call").expect("call");

        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            Some(container),
            Arc::clone(&snapshot),
            None,
            settings,
            2000_00000000,
            None,
        )
        .expect("engine builds");
        engine.load_script(b.to_array(), CallFlags::ALL, None).expect("loads");
        assert_eq!(engine.execute_allow_fault(), VmState::HALT, "transfer must HALT");

        // The candidate is registered with zero votes.
        let item = snapshot.get(&candidate_key(&pubkey)).expect("candidate entry written");
        let (registered, votes) = decode_candidate_state(&item.value_bytes()).unwrap();
        assert!(registered, "candidate is Registered");
        assert_eq!(votes, BigInt::from(0));
        // The sender paid its whole balance (entry deleted by the transfer) and
        // NEO's credited balance was burned away again (entry deleted by Burn).
        assert!(snapshot.get(&gas_account_key(&sender)).is_none(), "sender spent all GAS");
        assert!(
            snapshot.get(&gas_account_key(&NeoToken::script_hash())).is_none(),
            "NEO's received GAS is burned"
        );
        // The burn shrank the total supply back to zero.
        let supply = snapshot
            .get(&StorageKey::new(crate::GasToken::ID, vec![crate::NEP17_PREFIX_TOTAL_SUPPLY]))
            .expect("supply entry");
        assert_eq!(BigInt::from_signed_bytes_le(&supply.value_bytes()), BigInt::from(0));
    }

    /// Direct-invocation engine with the calling script hash forced to `caller`
    /// and (optionally) `signer` witnessing the container.
    fn payment_engine(
        snapshot: Arc<DataCache>,
        caller: Option<UInt160>,
        signer: Option<UInt160>,
    ) -> ApplicationEngine {
        let container: Option<Arc<dyn Verifiable>> = signer.map(|hash| {
            let mut tx = Transaction::new();
            tx.set_signers(vec![Signer::new(hash, WitnessScope::GLOBAL)]);
            tx.set_witnesses(vec![Witness::empty()]);
            Arc::new(tx) as Arc<dyn Verifiable>
        });
        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            container,
            snapshot,
            None,
            ProtocolSettings::default(),
            10_000_000,
            None,
        )
        .expect("engine builds");
        engine.set_calling_script_hash(caller);
        engine
    }

    /// onNEP17Payment args `[from, amount, data]` as the dispatcher marshals
    /// them: Hash160 raw, Integer signed-LE, Any BinarySerialized.
    fn payment_args(from: &UInt160, amount: i64, data: &StackItem) -> Vec<Vec<u8>> {
        vec![
            from.to_bytes().to_vec(),
            BigInt::from(amount).to_signed_bytes_le(),
            BinarySerializer::serialize(data, &ExecutionEngineLimits::default()).unwrap(),
        ]
    }

    /// C# OnNEP17Payment faults unless the caller is the GAS contract, the
    /// amount equals the register price, `data` decodes as a secp256r1 point,
    /// and the candidate account witnesses the transaction.
    #[test]
    fn on_nep17_payment_rejects_bad_caller_amount_pubkey_and_witness() {
        crate::install();
        let pubkey = candidate_pubkey();
        let candidate_account =
            UInt160::from_script(&Contract::create_signature_redeem_script(pubkey.clone()));
        let sender = UInt160::from_bytes(&[0x07; 20]).unwrap();
        let snapshot = Arc::new(DataCache::new(false));
        let neo = NeoToken::new();
        let pubkey_item = StackItem::from_byte_string(pubkey.to_bytes());

        // Caller is not GAS (here: unset) -> fault.
        let mut engine =
            payment_engine(Arc::clone(&snapshot), None, Some(candidate_account));
        let err = NativeContract::invoke(
            &neo,
            &mut engine,
            "onNEP17Payment",
            &payment_args(&sender, DEFAULT_REGISTER_PRICE, &pubkey_item),
        )
        .unwrap_err();
        assert!(err.to_string().contains("only the GAS contract"), "{err}");

        // Wrong amount -> fault.
        let gas_caller = Some(crate::GasToken::script_hash());
        let mut engine =
            payment_engine(Arc::clone(&snapshot), gas_caller, Some(candidate_account));
        let err = NativeContract::invoke(
            &neo,
            &mut engine,
            "onNEP17Payment",
            &payment_args(&sender, DEFAULT_REGISTER_PRICE - 1, &pubkey_item),
        )
        .unwrap_err();
        assert!(err.to_string().contains("incorrect GAS amount"), "{err}");

        // `data` is not a public key -> fault.
        let mut engine =
            payment_engine(Arc::clone(&snapshot), gas_caller, Some(candidate_account));
        let err = NativeContract::invoke(
            &neo,
            &mut engine,
            "onNEP17Payment",
            &payment_args(
                &sender,
                DEFAULT_REGISTER_PRICE,
                &StackItem::from_byte_string(vec![1, 2, 3]),
            ),
        )
        .unwrap_err();
        assert!(err.to_string().contains("bad public key"), "{err}");

        // No witness from the candidate account -> RegisterInternal returns
        // false -> C# throws "Failed to register candidate".
        let mut engine = payment_engine(Arc::clone(&snapshot), gas_caller, Some(sender));
        let err = NativeContract::invoke(
            &neo,
            &mut engine,
            "onNEP17Payment",
            &payment_args(&sender, DEFAULT_REGISTER_PRICE, &pubkey_item),
        )
        .unwrap_err();
        assert!(err.to_string().contains("failed to register"), "{err}");
        assert!(snapshot.get(&candidate_key(&pubkey)).is_none(), "nothing registered");
    }
}

/// Unit tests for `ComputeCommitteeMembers` (C# NeoToken.cs:622-635): the
/// turnout boundary, the standby fallback (low turnout / too few candidates,
/// zipped with registered-candidate votes), and the top-m ordering
/// (votes descending, pubkey ascending).
#[cfg(test)]
mod committee_recompute_tests {
    use super::*;
    use neo_config::ProtocolSettings;

    /// `n` distinct valid secp256r1 points (the mainnet standby committee).
    fn points(n: usize) -> Vec<ECPoint> {
        let pts = ProtocolSettings::default().standby_committee;
        assert!(pts.len() >= n, "mainnet standby committee has 21 members");
        pts.into_iter().take(n).collect()
    }

    fn settings_with_committee(committee: Vec<ECPoint>) -> ProtocolSettings {
        ProtocolSettings {
            standby_committee: committee,
            validators_count: 1,
            ..ProtocolSettings::default()
        }
    }

    fn seed_voters_count(cache: &DataCache, value: i64) {
        cache.add(
            voters_count_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(value))),
        );
    }

    fn seed_candidate(cache: &DataCache, pubkey: &ECPoint, votes: i64) {
        cache.add(
            candidate_key(pubkey),
            StorageItem::from_bytes(encode_candidate_state(true, &BigInt::from(votes)).unwrap()),
        );
    }

    #[test]
    fn should_refresh_committee_matches_csharp_modulo() {
        // C# `height % committeeMembersCount == 0`.
        assert!(should_refresh_committee(0, 21));
        assert!(!should_refresh_committee(1, 21));
        assert!(!should_refresh_committee(20, 21));
        assert!(should_refresh_committee(21, 21));
        assert!(should_refresh_committee(42, 21));
        // A single-member committee refreshes every block.
        assert!(should_refresh_committee(5, 1));
    }

    #[test]
    fn standby_fallback_below_turnout_zips_registered_votes() {
        // Turnout one NEO short of the 20% boundary: votersCount * 5 =
        // 99_999_995 < TotalAmount, so even with >= m candidates the standby
        // committee wins — each member zipped with its registered-candidate
        // votes (zero when not a candidate). C#: `voterTurnout < 0.2M`.
        let all = points(6);
        let standby = all[..3].to_vec();
        let settings = settings_with_committee(standby.clone());
        let cache = DataCache::new(false);
        seed_voters_count(&cache, 19_999_999);
        seed_candidate(&cache, &standby[1], 42); // a standby member is a candidate
        seed_candidate(&cache, &all[3], 1000);
        seed_candidate(&cache, &all[4], 900);
        seed_candidate(&cache, &all[5], 800);

        let members = compute_committee_members(&cache, &settings).unwrap();
        assert_eq!(
            members,
            vec![
                (standby[0].clone(), BigInt::from(0)),
                (standby[1].clone(), BigInt::from(42)),
                (standby[2].clone(), BigInt::from(0)),
            ],
            "standby order is preserved; votes come from the candidate records"
        );
    }

    #[test]
    fn standby_fallback_when_fewer_candidates_than_committee() {
        // Turnout reached, but only 2 registered candidates for a 3-member
        // committee: C# `candidates.Length < settings.CommitteeMembersCount`
        // falls back to the standby committee.
        let all = points(5);
        let standby = all[..3].to_vec();
        let settings = settings_with_committee(standby.clone());
        let cache = DataCache::new(false);
        seed_voters_count(&cache, 20_000_000);
        seed_candidate(&cache, &all[3], 1000);
        seed_candidate(&cache, &all[4], 900);

        let members = compute_committee_members(&cache, &settings).unwrap();
        let keys: Vec<ECPoint> = members.into_iter().map(|(p, _)| p).collect();
        assert_eq!(keys, standby);
    }

    #[test]
    fn top_m_at_exact_turnout_boundary_orders_votes_desc_pubkey_asc() {
        // votersCount * 5 == TotalAmount exactly: C# `voterTurnout < 0.2M` is
        // false (>= 0.2 passes), so with enough candidates the elected
        // committee is the top m by (votes DESC, pubkey ASC).
        let all = points(5);
        let standby = all[..3].to_vec();
        let settings = settings_with_committee(standby);
        let cache = DataCache::new(false);
        seed_voters_count(&cache, 20_000_000);
        let (c0, c1, c2, c3) = (&all[1], &all[2], &all[3], &all[4]);
        seed_candidate(&cache, c0, 10);
        seed_candidate(&cache, c1, 7);
        seed_candidate(&cache, c2, 50);
        seed_candidate(&cache, c3, 5); // 4th candidate drops out of the top 3

        let members = compute_committee_members(&cache, &settings).unwrap();
        assert_eq!(
            members,
            vec![
                (c2.clone(), BigInt::from(50)),
                (c0.clone(), BigInt::from(10)),
                (c1.clone(), BigInt::from(7)),
            ]
        );
    }

    #[test]
    fn top_m_breaks_vote_ties_by_ascending_pubkey() {
        // C# `OrderByDescending(votes).ThenBy(pubkey)` — equal votes order by
        // the ECPoint comparison (X then Y), ascending.
        let all = points(4);
        let standby = vec![all[0].clone()];
        let settings = settings_with_committee(standby);
        let cache = DataCache::new(false);
        seed_voters_count(&cache, 20_000_000);
        let (a, b) = (all[2].clone(), all[3].clone());
        seed_candidate(&cache, &a, 9);
        seed_candidate(&cache, &b, 9);

        let members = compute_committee_members(&cache, &settings).unwrap();
        let (lo, hi) = if a < b { (a, b) } else { (b, a) };
        assert_eq!(members, vec![(lo, BigInt::from(9))], "m = 1 takes the lower pubkey");
        drop(hi);
    }

    #[test]
    fn bft_address_uses_the_bft_multisig_threshold() {
        // C# Contract.GetBFTAddress: m = n - (n - 1) / 3 (7 validators -> 5).
        let validators = ProtocolSettings::default().standby_validators();
        assert_eq!(validators.len(), 7);
        let script =
            neo_redeem_script::multi_sig_redeem_script_from_points(5, &validators).unwrap();
        assert_eq!(bft_address(&validators).unwrap(), UInt160::from_script(&script));
    }
}

/// Engine-level tests for the block-boundary hooks: `on_persist` (committee
/// recompute + `CommitteeChanged`, C# NeoToken.cs:222-251) and `post_persist`
/// (committee GAS reward + voter-reward accrual, C# NeoToken.cs:253-284),
/// with reward values hand-computed from the C# formulas.
#[cfg(test)]
mod persist_hook_tests {
    use super::*;
    use neo_config::ProtocolSettings;
    use neo_execution::ApplicationEngine;
    use neo_payloads::{Block, BlockHeader};
    use neo_primitives::TriggerType;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn engine_for(
        trigger: TriggerType,
        snapshot: Arc<DataCache>,
        index: u32,
        settings: ProtocolSettings,
    ) -> ApplicationEngine {
        let mut header = BlockHeader::default();
        header.set_index(index);
        ApplicationEngine::new(
            trigger,
            None,
            snapshot,
            Some(Block::from_parts(header, vec![])),
            settings,
            0,
            None,
        )
        .expect("engine builds")
    }

    fn committee_storage_key() -> StorageKey {
        StorageKey::new(NeoToken::ID, vec![PREFIX_COMMITTEE])
    }

    fn seed_committee_cache(cache: &DataCache, members: &[(ECPoint, BigInt)]) {
        cache.add(
            committee_storage_key(),
            StorageItem::from_bytes(encode_committee(members).unwrap()),
        );
    }

    fn voter_reward_key(pubkey: &ECPoint) -> StorageKey {
        let mut key = vec![PREFIX_VOTER_REWARD_PER_COMMITTEE];
        key.extend_from_slice(&pubkey.to_bytes());
        StorageKey::new(NeoToken::ID, key)
    }

    fn read_voter_reward(snapshot: &DataCache, pubkey: &ECPoint) -> Option<BigInt> {
        snapshot
            .get(&voter_reward_key(pubkey))
            .map(|item| BigInt::from_signed_bytes_le(&item.value_bytes()))
    }

    fn gas_balance(snapshot: &DataCache, account: &UInt160) -> Option<BigInt> {
        let mut key = vec![crate::NEP17_PREFIX_ACCOUNT];
        key.extend_from_slice(&account.to_bytes());
        let item = snapshot.get(&StorageKey::new(crate::GasToken::ID, key))?;
        let decoded = BinarySerializer::deserialize(
            &item.value_bytes(),
            &ExecutionEngineLimits::default(),
            None,
        )
        .unwrap();
        let StackItem::Struct(fields) = decoded else {
            panic!("GAS account is not a struct");
        };
        Some(fields.items().first().unwrap().as_int().unwrap())
    }

    fn signature_address(pubkey: &ECPoint) -> UInt160 {
        UInt160::from_script(&Contract::create_signature_redeem_script(pubkey.clone()))
    }

    #[test]
    fn on_persist_refresh_recomputes_committee_and_emits_committee_changed() {
        // Single-member committee (every block refreshes); HF_Cockatrice at 0
        // so the notification path is active. Seeded: standby K1 cached,
        // turnout exactly at the 20% boundary, candidate K2 registered with 7
        // votes -> recompute elects [K2] and emits CommitteeChanged([K1],[K2]).
        let all = ProtocolSettings::default().standby_committee;
        let (k1, k2) = (all[0].clone(), all[1].clone());
        let mut hardforks = HashMap::new();
        hardforks.insert(Hardfork::HfCockatrice, 0u32);
        let settings = ProtocolSettings {
            standby_committee: vec![k1.clone()],
            validators_count: 1,
            hardforks,
            ..ProtocolSettings::default()
        };
        let cache = DataCache::new(false);
        seed_committee_cache(&cache, &[(k1.clone(), BigInt::from(0))]);
        cache.add(
            voters_count_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(20_000_000))),
        );
        cache.add(
            candidate_key(&k2),
            StorageItem::from_bytes(encode_candidate_state(true, &BigInt::from(7)).unwrap()),
        );
        let snapshot = Arc::new(cache);

        let mut engine = engine_for(TriggerType::OnPersist, Arc::clone(&snapshot), 1, settings);
        NeoToken.on_persist(&mut engine).expect("on_persist");

        // The cache now holds the elected committee, CachedCommittee layout.
        let stored = snapshot.get(&committee_storage_key()).unwrap().value_bytes().into_owned();
        assert_eq!(stored, encode_committee(&[(k2.clone(), BigInt::from(7))]).unwrap());

        // CommitteeChanged([prev pubkeys], [new pubkeys]).
        let notes = engine.notifications();
        assert_eq!(notes.len(), 1, "exactly one notification");
        let note = &notes[0];
        assert_eq!(note.script_hash, NeoToken::script_hash());
        assert_eq!(note.event_name, "CommitteeChanged");
        assert_eq!(note.state.len(), 2);
        let keys_of = |item: &StackItem| -> Vec<Vec<u8>> {
            let StackItem::Array(array) = item else {
                panic!("CommitteeChanged arg is not an array");
            };
            array.items().iter().map(|i| i.as_bytes().unwrap().to_vec()).collect()
        };
        assert_eq!(keys_of(&note.state[0]), vec![k1.to_bytes()]);
        assert_eq!(keys_of(&note.state[1]), vec![k2.to_bytes()]);
    }

    #[test]
    fn on_persist_refresh_without_cockatrice_updates_committee_silently() {
        // Same election as above, but HF_Cockatrice is unscheduled: the
        // committee cache still updates, with no notification (pre-3158
        // behavior, the C# hardfork gate).
        let all = ProtocolSettings::default().standby_committee;
        let (k1, k2) = (all[0].clone(), all[1].clone());
        let settings = ProtocolSettings {
            standby_committee: vec![k1.clone()],
            validators_count: 1,
            hardforks: HashMap::new(),
            ..ProtocolSettings::default()
        };
        let cache = DataCache::new(false);
        seed_committee_cache(&cache, &[(k1.clone(), BigInt::from(0))]);
        cache.add(
            voters_count_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(20_000_000))),
        );
        cache.add(
            candidate_key(&k2),
            StorageItem::from_bytes(encode_candidate_state(true, &BigInt::from(7)).unwrap()),
        );
        let snapshot = Arc::new(cache);

        let mut engine = engine_for(TriggerType::OnPersist, Arc::clone(&snapshot), 1, settings);
        NeoToken.on_persist(&mut engine).expect("on_persist");

        let stored = snapshot.get(&committee_storage_key()).unwrap().value_bytes().into_owned();
        assert_eq!(stored, encode_committee(&[(k2, BigInt::from(7))]).unwrap());
        assert!(engine.notifications().is_empty(), "no CommitteeChanged before Cockatrice");
    }

    #[test]
    fn on_persist_skips_recompute_off_refresh_blocks() {
        // m = 3, block index 2: 2 % 3 != 0, so the committee cache must stay
        // untouched even though a recompute would elect different members.
        let all = ProtocolSettings::default().standby_committee;
        let standby = all[..3].to_vec();
        let settings = ProtocolSettings {
            standby_committee: standby.clone(),
            validators_count: 1,
            ..ProtocolSettings::default()
        };
        let seeded: Vec<(ECPoint, BigInt)> =
            standby.iter().map(|p| (p.clone(), BigInt::from(0))).collect();
        let cache = DataCache::new(false);
        seed_committee_cache(&cache, &seeded);
        cache.add(
            voters_count_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(20_000_000))),
        );
        for (i, candidate) in all[3..6].iter().enumerate() {
            cache.add(
                candidate_key(candidate),
                StorageItem::from_bytes(
                    encode_candidate_state(true, &BigInt::from(100 + i as i64)).unwrap(),
                ),
            );
        }
        let snapshot = Arc::new(cache);

        let mut engine = engine_for(TriggerType::OnPersist, Arc::clone(&snapshot), 2, settings);
        NeoToken.on_persist(&mut engine).expect("on_persist");

        let stored = snapshot.get(&committee_storage_key()).unwrap().value_bytes().into_owned();
        assert_eq!(stored, encode_committee(&seeded).unwrap(), "cache untouched off refresh");
        assert!(engine.notifications().is_empty());
    }

    /// Hand-computed C# PostPersistAsync values for the default settings
    /// (m = 21, n = 7) with gasPerBlock = 5 GAS:
    ///   committee reward      = 5_0000_0000 * 10 / 100        = 0.5 GAS
    ///   voterRewardOfEachCommittee
    ///     = 5e8 * 80 * 1e8 * 21 / (21 + 7) / 100              = 3e16
    ///   member 0 (validator, factor 2, 1000 votes): 2*3e16/1000 = 6e13
    ///   member 7 (non-validator, factor 1, 400 votes): 3e16/400 = 7.5e13
    #[test]
    fn post_persist_committee_and_voter_rewards_match_csharp_math() {
        let settings = ProtocolSettings::default();
        assert_eq!(settings.committee_members_count(), 21);
        assert_eq!(settings.validators_count, 7);
        let members: Vec<(ECPoint, BigInt)> = settings
            .standby_committee
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let votes = match i {
                    0 => 1000,
                    7 => 400,
                    _ => 0,
                };
                (p.clone(), BigInt::from(votes))
            })
            .collect();
        let cache = DataCache::new(false);
        seed_committee_cache(&cache, &members);
        put_gas_per_block(&cache, 0, &BigInt::from(DEFAULT_GAS_PER_BLOCK));
        // Pre-seed member 0's accumulator: C# `GetAndChange(key).Add(...)` is
        // read-modify-write, so the accrual must ADD to the existing value.
        cache.add(
            voter_reward_key(&members[0].0),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(5))),
        );
        let snapshot = Arc::new(cache);

        // Block 0 is a refresh block (0 % 21 == 0).
        let mut engine =
            engine_for(TriggerType::PostPersist, Arc::clone(&snapshot), 0, settings.clone());
        NeoToken.post_persist(&mut engine).expect("post_persist");

        // committee[0 % 21] earns 0.5 GAS at its signature address.
        let member0_addr = signature_address(&members[0].0);
        assert_eq!(gas_balance(&snapshot, &member0_addr), Some(BigInt::from(50_000_000)));
        // The mint emitted GAS Transfer(null, member0, 0.5 GAS).
        let transfer = engine
            .notifications()
            .iter()
            .find(|n| n.event_name == "Transfer")
            .expect("committee reward Transfer");
        assert_eq!(transfer.script_hash, crate::GasToken::script_hash());
        assert!(matches!(transfer.state[0], StackItem::Null));
        assert_eq!(transfer.state[1].as_bytes().unwrap().to_vec(), member0_addr.to_bytes());
        assert_eq!(transfer.state[2].as_int().unwrap(), BigInt::from(50_000_000));

        // Voter-reward accruals (zoomed by VoteFactor), added to any existing value.
        assert_eq!(
            read_voter_reward(&snapshot, &members[0].0),
            Some(BigInt::from(60_000_000_000_005i64)),
            "validator voter reward: pre-seeded 5 + 2 * 3e16 / 1000"
        );
        assert_eq!(
            read_voter_reward(&snapshot, &members[7].0),
            Some(BigInt::from(75_000_000_000_000i64)),
            "non-validator voter reward: 3e16 / 400"
        );
        assert_eq!(
            read_voter_reward(&snapshot, &members[1].0),
            None,
            "zero-vote members accrue nothing"
        );
    }

    #[test]
    fn post_persist_off_refresh_blocks_only_mints_the_rotating_reward() {
        // Block 1 (1 % 21 != 0): committee[1] earns 0.5 GAS; no voter-reward
        // accrual happens even for members with votes.
        let settings = ProtocolSettings::default();
        let members: Vec<(ECPoint, BigInt)> = settings
            .standby_committee
            .iter()
            .enumerate()
            .map(|(i, p)| (p.clone(), BigInt::from(if i == 0 { 1000 } else { 0 })))
            .collect();
        let cache = DataCache::new(false);
        seed_committee_cache(&cache, &members);
        put_gas_per_block(&cache, 0, &BigInt::from(DEFAULT_GAS_PER_BLOCK));
        let snapshot = Arc::new(cache);

        let mut engine =
            engine_for(TriggerType::PostPersist, Arc::clone(&snapshot), 1, settings.clone());
        NeoToken.post_persist(&mut engine).expect("post_persist");

        let member1_addr = signature_address(&members[1].0);
        assert_eq!(gas_balance(&snapshot, &member1_addr), Some(BigInt::from(50_000_000)));
        assert_eq!(gas_balance(&snapshot, &signature_address(&members[0].0)), None);
        assert_eq!(
            read_voter_reward(&snapshot, &members[0].0),
            None,
            "no accrual off refresh blocks"
        );
    }

    /// C# `NeoToken.OnManifestCompose` (NeoToken.cs:112-122): NEP-27 joins
    /// NEP-17 once HF_Echidna is enabled at the height — and Echidna is a
    /// manifest-refresh hardfork for NEO (C# carries it in `_usedHardforks`
    /// via the Echidna-gated method registrations).
    #[test]
    fn manifest_standards_gain_nep27_at_echidna() {
        use neo_execution::native_contract::build_native_contract_state;

        let mut settings = ProtocolSettings::default();
        settings.hardforks.insert(Hardfork::HfEchidna, 10);
        let before = build_native_contract_state(&NeoToken, &settings, 9);
        assert_eq!(before.manifest.supported_standards, ["NEP-17"]);
        let after = build_native_contract_state(&NeoToken, &settings, 10);
        assert_eq!(after.manifest.supported_standards, ["NEP-17", "NEP-27"]);

        assert!(NativeContract::used_hardforks(&NeoToken).contains(&Hardfork::HfEchidna));
    }
}
