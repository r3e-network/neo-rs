use super::*;
use neo_serialization::BinarySerializer;
use neo_vm_rs::ExecutionEngineLimits;
use num_traits::ToPrimitive;

mod views;

pub(crate) use views::CachedCommittee;
pub(super) use views::{CandidateState, NeoAccountStateView};

/// Process-global memoization for the deserialized committee, keyed by the exact
/// `Prefix_Committee` storage bytes. A pure function of those bytes (same bytes
/// always deserialize to the same members), so it is correct across snapshots,
/// heights, and reverts. Eliminates the per-block EC-point decompression of the
/// committee pubkeys on the hot path. See [`NeoToken::read_committee_with_votes`].
static COMMITTEE_DESERIALIZE_CACHE: std::sync::Mutex<
    Option<(Vec<u8>, Vec<(ECPoint, BigInt)>)>,
> = std::sync::Mutex::new(None);

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

    pub(super) fn register_price_key() -> StorageKey {
        crate::keys::prefixed_key(Self::ID, PREFIX_REGISTER_PRICE, &[])
    }

    /// The `Prefix_GasPerBlock` prefix key used for backward gas-record scans.
    pub(super) fn gas_per_block_prefix_key() -> StorageKey {
        crate::keys::prefixed_key(Self::ID, PREFIX_GAS_PER_BLOCK, &[])
    }

    /// The `Prefix_GasPerBlock` storage key for a record index.
    pub(super) fn gas_per_block_key(index: u32) -> StorageKey {
        crate::keys::prefixed_u32_be_key(Self::ID, PREFIX_GAS_PER_BLOCK, index)
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

    /// Decodes a stored `NeoAccountState` struct into its fields.
    pub(super) fn decode_neo_account_state(value: &[u8]) -> CoreResult<NeoAccountStateView> {
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
    pub(super) fn encode_neo_account_state(state: &NeoAccountStateView) -> CoreResult<Vec<u8>> {
        let item = state.to_stack_value();
        BinarySerializer::serialize_stack_value_default(&item)
            .map_err(|e| CoreError::invalid_operation(format!("encode neo account state: {e}")))
    }

    /// The `Prefix_VotersCount` storage key (a single key, no suffix).
    pub(crate) fn voters_count_key() -> StorageKey {
        crate::keys::prefixed_key(Self::ID, PREFIX_VOTERS_COUNT, &[])
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

    /// C# `NeoToken.CheckCandidate`: when a candidate is unregistered and has no
    /// remaining votes, delete its candidate + voter-reward entries.
    pub(super) fn check_candidate(
        &self,
        snapshot: &DataCache,
        pubkey: &ECPoint,
        registered: bool,
        votes: &BigInt,
    ) -> CoreResult<()> {
        if !registered && *votes == BigInt::from(0) {
            let reward_key = Self::voter_reward_per_committee_key(pubkey);
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

    /// Reads the cached committee from `Prefix_Committee` (C#
    /// `GetCommitteeFromCache`) as `(pubkey, votes)` pairs in stored order. The
    /// value is a `BinarySerializer` array whose elements are `Struct[pubkey(33-byte
    /// compressed), votes]` (C# `CachedCommittee.ElementToStackItem`). Errors when
    /// the cache has never been initialized, matching the C# indexer/`GetAndChange`
    /// null deref.
    pub(super) fn read_committee_with_votes(
        &self,
        snapshot: &DataCache,
    ) -> CoreResult<Vec<(ECPoint, BigInt)>> {
        let key = Self::committee_key();
        let item = snapshot.get(&key).ok_or_else(|| {
            CoreError::invalid_operation("NeoToken committee cache is not initialized")
        })?;
        let raw = item.value_bytes();

        // Memoize the deserialized committee keyed by the exact stored bytes.
        // `read_committee_with_votes` is on the per-block hot path (GasToken
        // OnPersist primary reward, extensible-witness whitelist), and each
        // deserialization EC-point-decompresses all committee pubkeys — the
        // single dominant CPU cost during catch-up. The committee bytes only
        // change on a refresh block (every `committee_count` blocks), so this is
        // a pure function of the bytes (same bytes => same members): correct
        // across snapshots/heights/reverts, mirroring C#'s in-memory committee
        // cache (`GetCommitteeFromCache`).
        {
            let cache = COMMITTEE_DESERIALIZE_CACHE.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((cached_bytes, cached_members)) = cache.as_ref() {
                if cached_bytes.as_slice() == raw.as_ref() {
                    return Ok(cached_members.clone());
                }
            }
        }

        let limits = ExecutionEngineLimits::default();
        let decoded = BinarySerializer::deserialize_stack_value_with_limits(
            &raw,
            limits.max_item_size as usize,
            limits.max_stack_size as usize,
        )
        .map_err(|e| CoreError::deserialization(format!("committee cache: {e}")))?;
        let members = CachedCommittee::from_stack_value(decoded)?.into_members();

        let mut cache = COMMITTEE_DESERIALIZE_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        *cache = Some((raw.into_owned(), members.clone()));
        Ok(members)
    }

    /// Reads only the cached committee public keys, in stored order.
    pub(super) fn read_committee_points(&self, snapshot: &DataCache) -> CoreResult<Vec<ECPoint>> {
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
    pub(super) fn encode_committee(members: &[(ECPoint, BigInt)]) -> CoreResult<Vec<u8>> {
        let array = CachedCommittee::new(members.to_vec()).to_stack_value();
        BinarySerializer::serialize_stack_value_default(&array)
            .map_err(|e| CoreError::invalid_operation(format!("encode committee cache: {e}")))
    }

    /// C# `NeoToken.ShouldRefreshCommittee(height, committeeMembersCount)`:
    /// the committee is recounted on every block whose index is a multiple of the
    /// committee size. `committee_count` must be non-zero (validated by callers,
    /// like the C# division-by-zero).
    pub(super) fn should_refresh_committee(height: u32, committee_count: usize) -> bool {
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
    pub(super) fn compute_committee_members(
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
        neo_vm::script_builder::RedeemScript::bft_address(pubkeys)
            .ok_or_else(|| CoreError::invalid_operation("BFT address requires at least one key"))
    }

    /// C# `GetCommittee` = committee public keys sorted ascending (`OrderBy(p => p)`).
    pub(super) fn committee_sorted(&self, snapshot: &DataCache) -> CoreResult<Vec<ECPoint>> {
        let mut points = self.read_committee_points(snapshot)?;
        points.sort();
        Ok(points)
    }

    /// Decodes a `CandidateState` storage value — a `Struct[Registered(bool), Votes]`
    /// — into `(registered, votes)`.
    pub(super) fn decode_candidate_state(value: &[u8]) -> CoreResult<(bool, BigInt)> {
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
    pub(super) fn encode_candidate_state(registered: bool, votes: &BigInt) -> CoreResult<Vec<u8>> {
        let item = CandidateState::new(registered, votes.clone()).to_stack_value();
        BinarySerializer::serialize_stack_value_default(&item)
            .map_err(|e| CoreError::invalid_operation(format!("encode candidate state: {e}")))
    }

    /// The `Prefix_Candidate` storage key for `pubkey` (`prefix ++ 33-byte pubkey`).
    pub(crate) fn candidate_key(pubkey: &ECPoint) -> StorageKey {
        crate::keys::prefixed_key(Self::ID, PREFIX_CANDIDATE, &pubkey.to_bytes())
    }

    /// The `Prefix_Candidate` prefix key used for candidate scans.
    pub(super) fn candidate_prefix_key() -> StorageKey {
        crate::keys::prefixed_key(Self::ID, PREFIX_CANDIDATE, &[])
    }

    /// The `Prefix_Committee` storage key (a single key, no suffix).
    pub(crate) fn committee_key() -> StorageKey {
        crate::keys::prefixed_key(Self::ID, PREFIX_COMMITTEE, &[])
    }

    /// The `Prefix_VoterRewardPerCommittee` storage key for `pubkey`.
    pub(crate) fn voter_reward_per_committee_key(pubkey: &ECPoint) -> StorageKey {
        crate::keys::prefixed_key(
            Self::ID,
            PREFIX_VOTER_REWARD_PER_COMMITTEE,
            &pubkey.to_bytes(),
        )
    }

    /// The `Prefix_Account` storage key for `account` (NEP-17 account prefix).
    pub(crate) fn account_key(account: &UInt160) -> StorageKey {
        crate::nep17_account_key(Self::ID, account)
    }

    /// The NEP-17 total-supply storage key for NEO (`Prefix_TotalSupply`).
    pub(crate) fn total_supply_key() -> StorageKey {
        crate::nep17_total_supply_key(Self::ID)
    }

    /// C# `GetCandidatesInternal`: scan `Prefix_Candidate` (key = prefix ++ 33-byte
    /// pubkey; value = CandidateState `Struct[Registered(bool), Votes]`), returning
    /// the raw `(key, value)` storage entries of the registered candidates in
    /// storage-scan order, excluding candidates whose signature-contract address is
    /// blocked by `PolicyContract` (`!Policy.IsBlocked(snapshot, sigScriptHash)`).
    pub(super) fn registered_candidate_entries(
        &self,
        snapshot: &DataCache,
    ) -> CoreResult<Vec<(ECPoint, BigInt, StorageKey, StorageItem)>> {
        let prefix = Self::candidate_prefix_key();
        let mut out = Vec::new();
        for (key, item) in snapshot.find(Some(&prefix), SeekDirection::Forward) {
            let key_bytes = key.key();
            if key_bytes.len() < 34 {
                continue;
            }
            let Ok(pubkey) = ECPoint::from_bytes(&key_bytes[1..34]) else {
                continue;
            };
            // Decode the pubkey + votes ONCE here and carry them through, so
            // `read_registered_candidates` (the committee/getCandidates path) does
            // not EC-point-decompress every candidate a second time.
            let (registered, votes) = Self::decode_candidate_state(&item.value_bytes())?;
            if registered {
                let account = UInt160::from_script(&Contract::create_signature_redeem_script(
                    pubkey.clone(),
                ));
                if snapshot
                    .get(&crate::PolicyContract::blocked_account_key(&account))
                    .is_none()
                {
                    out.push((pubkey, votes, key, item));
                }
            }
        }
        Ok(out)
    }

    /// [`registered_candidate_entries`] projected to `(pubkey, votes)` pairs — the
    /// shape consumed by `getCandidates` and the committee recompute.
    pub(super) fn read_registered_candidates(
        &self,
        snapshot: &DataCache,
    ) -> CoreResult<Vec<(ECPoint, BigInt)>> {
        Ok(self
            .registered_candidate_entries(snapshot)?
            .into_iter()
            .map(|(pubkey, votes, _key, _item)| (pubkey, votes))
            .collect())
    }

    /// C# `RegisterInternal` (NeoToken.cs:411-423), shared by `registerCandidate`
    /// and the Echidna `onNEP17Payment` GAS-payment path: requires a witness from
    /// the candidate's signature-contract account (returning `false` without one),
    /// creates/flips the CandidateState to Registered, and emits
    /// `CandidateStateChanged` for a fresh registration unconditionally — C#
    /// `RegisterInternal` calls `SendNotification` with no hardfork guard, and
    /// native `SendNotification` ignores the method's AllowNotify call flag.
    /// `method` labels errors with the invoking ABI method.
    pub(super) fn register_internal(
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
        engine
            .send_notification(
                NeoToken::script_hash(),
                NEO_CANDIDATE_STATE_CHANGED_EVENT.to_owned(),
                vec![
                    StackItem::from_byte_string(pubkey.to_bytes()),
                    StackItem::from_bool(true),
                    StackItem::from_int(votes),
                ],
            )
            .map_err(|e| {
                CoreError::invalid_operation(format!("NeoToken::{method}: notify: {e}"))
            })?;
        Ok(true)
    }

    /// C# `GetCandidateVote`: the votes for `pubkey` if it is a registered candidate,
    /// else -1 (also -1 when there is no candidate entry at all).
    pub(super) fn candidate_vote(
        &self,
        snapshot: &DataCache,
        pubkey: &ECPoint,
    ) -> CoreResult<BigInt> {
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
    pub(super) fn candidates_to_array_bytes(
        candidates: &[(ECPoint, BigInt)],
    ) -> CoreResult<Vec<u8>> {
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
    pub(super) fn points_to_stack_value<'a, I>(points: I) -> StackValue
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

    pub(super) fn points_to_array_bytes(points: &[ECPoint]) -> CoreResult<Vec<u8>> {
        let array = Self::points_to_stack_value(points.iter());
        BinarySerializer::serialize_stack_value_default(&array)
            .map_err(|e| CoreError::invalid_operation(format!("NeoToken point array: {e}")))
    }

    pub(super) fn points_to_stack_item<'a, I>(points: I) -> CoreResult<StackItem>
    where
        I: IntoIterator<Item = &'a ECPoint>,
    {
        StackItem::try_from(Self::points_to_stack_value(points))
            .map_err(|e| CoreError::invalid_operation(format!("NeoToken point array: {e}")))
    }

    /// The committee multisig threshold `m = n - (n - 1) / 2` (committee majority,
    /// matching C# `GetCommitteeAddress`). `n` must be non-zero. The single source
    /// of this term; `PolicyContract::assert_almost_full_committee` reuses it.
    pub(crate) fn committee_threshold(n: usize) -> usize {
        n - (n - 1) / 2
    }

    /// C# `GetCommitteeAddress` = script hash of the `m`-of-`n` multisig over the
    /// committee public keys, where `m = n - (n - 1) / 2`. The multisig builder sorts
    /// the keys ascending exactly as C# `Contract.CreateMultiSigRedeemScript` does.
    pub(super) fn compute_committee_address(&self, snapshot: &DataCache) -> CoreResult<UInt160> {
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
    pub(super) fn read_account_state(
        &self,
        snapshot: &DataCache,
        account: &UInt160,
    ) -> Option<Vec<u8>> {
        let key = Self::account_key(account);
        snapshot
            .get(&key)
            .map(|item| item.value_bytes().into_owned())
    }

    /// Reads the NEO balance from the NEO-specific account state.
    pub(crate) fn balance_of(&self, snapshot: &DataCache, account: &UInt160) -> CoreResult<BigInt> {
        let Some(bytes) = self.read_account_state(snapshot, account) else {
            return Ok(BigInt::from(0));
        };
        Ok(Self::decode_neo_account_state(&bytes)?.balance)
    }
}
