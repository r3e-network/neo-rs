use super::*;
use neo_error::CoreError;
use neo_execution::Contract;
use neo_runtime::sync_metrics::{
    self, NeoTokenCommitteeCandidateCount, NeoTokenCommitteeComputeStage,
};
use neo_serialization::BinarySerializer;
use std::{
    collections::{HashMap, HashSet},
    sync::{OnceLock, RwLock},
    time::Instant,
};

static CANDIDATE_SIGNATURE_ACCOUNT_CACHE: OnceLock<RwLock<HashMap<ECPoint, UInt160>>> =
    OnceLock::new();

#[derive(Default)]
pub(super) struct CandidateScanCounts {
    /// Candidate storage rows visited under `Prefix_Candidate`.
    pub(super) storage_entries: u64,
    /// Rows whose key is too short, or whose registered candidate public key
    /// cannot be decompressed.
    pub(super) malformed_keys: u64,
    /// Rows whose candidate state decoded successfully. The committee scan
    /// decodes state before public keys so unregistered rows do not pay the
    /// ECPoint decompression cost.
    pub(super) decoded_entries: u64,
    /// Decoded rows flagged as registered before blocked-account filtering.
    pub(super) registered_entries: u64,
    /// Registered candidate rows skipped because their signature account is
    /// blocked by the policy contract.
    pub(super) blocked_registered: u64,
    /// Registered and unblocked candidates considered for committee ranking.
    pub(super) eligible_candidates: u64,
}

impl CandidateScanCounts {
    pub(super) fn record(&self, top_candidates: u64) {
        sync_metrics::record_neo_token_committee_candidate_count(
            NeoTokenCommitteeCandidateCount::StorageEntries,
            self.storage_entries,
        );
        sync_metrics::record_neo_token_committee_candidate_count(
            NeoTokenCommitteeCandidateCount::MalformedKeys,
            self.malformed_keys,
        );
        sync_metrics::record_neo_token_committee_candidate_count(
            NeoTokenCommitteeCandidateCount::DecodedEntries,
            self.decoded_entries,
        );
        sync_metrics::record_neo_token_committee_candidate_count(
            NeoTokenCommitteeCandidateCount::RegisteredEntries,
            self.registered_entries,
        );
        sync_metrics::record_neo_token_committee_candidate_count(
            NeoTokenCommitteeCandidateCount::BlockedRegistered,
            self.blocked_registered,
        );
        sync_metrics::record_neo_token_committee_candidate_count(
            NeoTokenCommitteeCandidateCount::EligibleCandidates,
            self.eligible_candidates,
        );
        sync_metrics::record_neo_token_committee_candidate_count(
            NeoTokenCommitteeCandidateCount::TopCandidates,
            top_candidates,
        );
    }
}

impl NeoToken {
    /// C# `NeoToken.CheckCandidate`: when a candidate is unregistered and has no
    /// remaining votes, delete its candidate + voter-reward entries.
    pub(in crate::neo_token) fn check_candidate<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
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
    pub(in crate::neo_token) fn compute_committee_members<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &neo_config::ProtocolSettings,
    ) -> CoreResult<Vec<(ECPoint, BigInt)>> {
        let stage_start = Instant::now();
        let voters_count = self.read_voters_count(snapshot);
        sync_metrics::record_neo_token_committee_compute_stage(
            NeoTokenCommitteeComputeStage::ReadVotersCount,
            neo_runtime::time::elapsed_us(stage_start.elapsed()),
        );
        let committee_count = settings.committee_members_count();
        if committee_count == 0 {
            return Err(CoreError::invalid_operation(
                "ComputeCommitteeMembers requires a non-empty standby committee",
            ));
        }
        let turnout_reached = voters_count * 5 >= BigInt::from(NEO_TOTAL_AMOUNT);
        if !turnout_reached {
            return self
                .standby_committee_with_registered_votes(snapshot, &settings.standby_committee);
        }

        let (candidate_count, top_candidates) =
            self.top_registered_candidates(snapshot, committee_count)?;
        if candidate_count < committee_count {
            return self
                .standby_committee_with_registered_votes(snapshot, &settings.standby_committee);
        }
        Ok(top_candidates)
    }

    fn standby_committee_with_registered_votes<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        standby_committee: &[ECPoint],
    ) -> CoreResult<Vec<(ECPoint, BigInt)>> {
        let stage_start = Instant::now();
        let members = standby_committee
            .iter()
            .map(|point| {
                let votes = match snapshot.get(&Self::candidate_key(point)) {
                    Some(item) => {
                        let (registered, votes) =
                            Self::decode_candidate_state(&item.value_bytes())?;
                        if registered && !candidate_is_blocked(snapshot, point) {
                            votes
                        } else {
                            BigInt::from(0)
                        }
                    }
                    None => BigInt::from(0),
                };
                Ok((point.clone(), votes))
            })
            .collect();
        sync_metrics::record_neo_token_committee_compute_stage(
            NeoTokenCommitteeComputeStage::StandbyLookup,
            neo_runtime::time::elapsed_us(stage_start.elapsed()),
        );
        members
    }

    fn top_registered_candidates<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        limit: usize,
    ) -> CoreResult<(usize, Vec<(ECPoint, BigInt)>)> {
        let total_start = Instant::now();
        let prefix = Self::candidate_prefix_key();
        let mut counts = CandidateScanCounts::default();
        let mut eligible_count = 0usize;
        let mut top = Vec::with_capacity(limit);
        let stage_start = Instant::now();
        let blocked_accounts = crate::PolicyContract::blocked_accounts_snapshot(snapshot);
        sync_metrics::record_neo_token_committee_compute_stage(
            NeoTokenCommitteeComputeStage::CandidateBlockedPrefetch,
            neo_runtime::time::elapsed_us(stage_start.elapsed()),
        );
        for (key, item) in snapshot.find(Some(&prefix), SeekDirection::Forward) {
            counts.storage_entries += 1;
            let key_bytes = key.key();
            if key_bytes.len() < 34 {
                counts.malformed_keys += 1;
                continue;
            }
            let stage_start = Instant::now();
            let state = Self::decode_candidate_state(&item.value_bytes());
            sync_metrics::record_neo_token_committee_compute_stage(
                NeoTokenCommitteeComputeStage::CandidateStateDecode,
                neo_runtime::time::elapsed_us(stage_start.elapsed()),
            );
            let (registered, votes) = match state {
                Ok(state) => state,
                Err(error) => {
                    let stage_start = Instant::now();
                    let pubkey = ECPoint::from_bytes(&key_bytes[1..34]);
                    sync_metrics::record_neo_token_committee_compute_stage(
                        NeoTokenCommitteeComputeStage::CandidatePubkeyDecode,
                        neo_runtime::time::elapsed_us(stage_start.elapsed()),
                    );
                    if pubkey.is_err() {
                        counts.malformed_keys += 1;
                        continue;
                    }
                    return Err(error);
                }
            };
            counts.decoded_entries += 1;
            if !registered {
                continue;
            }
            counts.registered_entries += 1;
            let stage_start = Instant::now();
            let pubkey = ECPoint::from_bytes(&key_bytes[1..34]);
            sync_metrics::record_neo_token_committee_compute_stage(
                NeoTokenCommitteeComputeStage::CandidatePubkeyDecode,
                neo_runtime::time::elapsed_us(stage_start.elapsed()),
            );
            let Ok(pubkey) = pubkey else {
                counts.malformed_keys += 1;
                continue;
            };
            let stage_start = Instant::now();
            let blocked = candidate_is_blocked_in(&blocked_accounts, &pubkey);
            sync_metrics::record_neo_token_committee_compute_stage(
                NeoTokenCommitteeComputeStage::CandidateBlockedLookup,
                neo_runtime::time::elapsed_us(stage_start.elapsed()),
            );
            if blocked {
                counts.blocked_registered += 1;
                continue;
            }
            eligible_count += 1;
            counts.eligible_candidates += 1;
            let stage_start = Instant::now();
            push_top_committee_candidate(&mut top, limit, (pubkey, votes));
            sync_metrics::record_neo_token_committee_compute_stage(
                NeoTokenCommitteeComputeStage::TopCandidateMaintenance,
                neo_runtime::time::elapsed_us(stage_start.elapsed()),
            );
        }
        counts.record(top.len() as u64);
        sync_metrics::record_neo_token_committee_compute_stage(
            NeoTokenCommitteeComputeStage::CandidateScanTotal,
            neo_runtime::time::elapsed_us(total_start.elapsed()),
        );
        Ok((eligible_count, top))
    }

    /// Decodes a `CandidateState` storage value - a `Struct[Registered(bool), Votes]`
    /// - into `(registered, votes)`.
    pub(in crate::neo_token) fn decode_candidate_state(value: &[u8]) -> CoreResult<(bool, BigInt)> {
        if let Some(decoded) = Self::decode_canonical_candidate_state(value)? {
            return Ok(decoded);
        }
        let decoded = crate::support::codec::decode_stack_value(value, "candidate state")?;
        let state = CandidateState::from_stack_value(decoded)?;
        Ok((state.registered, state.votes))
    }

    pub(in crate::neo_token) fn decode_canonical_candidate_state(
        value: &[u8],
    ) -> CoreResult<Option<(bool, BigInt)>> {
        const STRUCT: u8 = neo_vm_rs::NEOVM_STACK_ITEM_TYPE_STRUCT;
        const BOOLEAN: u8 = neo_vm_rs::NEOVM_STACK_ITEM_TYPE_BOOLEAN;
        const INTEGER: u8 = neo_vm_rs::NEOVM_STACK_ITEM_TYPE_INTEGER;

        let Some((&STRUCT, tail)) = value.split_first() else {
            return Ok(None);
        };
        let Some((&2, tail)) = tail.split_first() else {
            return Ok(None);
        };
        let Some((&BOOLEAN, tail)) = tail.split_first() else {
            return Ok(None);
        };
        let Some((&registered_byte, tail)) = tail.split_first() else {
            return Ok(None);
        };
        let registered = match registered_byte {
            0 => false,
            1 => true,
            _ => return Ok(None),
        };
        let Some((&INTEGER, tail)) = tail.split_first() else {
            return Ok(None);
        };
        let Some((vote_len, vote_len_width)) = neo_io::var_int::VarInt::read_var_int_prefix(tail)
        else {
            return Ok(None);
        };
        if vote_len > 32 {
            return Ok(None);
        }
        let vote_len = vote_len as usize;
        let vote_start = vote_len_width;
        let vote_end = vote_start + vote_len;
        if tail.len() != vote_end {
            return Ok(None);
        }

        Ok(Some((
            registered,
            BigInt::from_signed_bytes_le(&tail[vote_start..vote_end]),
        )))
    }

    /// Encodes a `CandidateState` storage value - a `Struct[Registered(bool),
    /// Votes]` - the write counterpart of [`decode_candidate_state`].
    pub(in crate::neo_token) fn encode_candidate_state(
        registered: bool,
        votes: &BigInt,
    ) -> CoreResult<Vec<u8>> {
        crate::support::codec::encode_storage_struct(
            &CandidateState::new(registered, votes.clone()),
            "candidate state",
        )
    }

    /// C# `GetCandidatesInternal`: scan `Prefix_Candidate` (key = prefix ++ 33-byte
    /// pubkey; value = CandidateState `Struct[Registered(bool), Votes]`), returning
    /// the raw `(key, value)` storage entries of the registered candidates in
    /// storage-scan order, excluding candidates whose signature-contract address is
    /// blocked by `PolicyContract` (`!Policy.IsBlocked(snapshot, sigScriptHash)`).
    pub(in crate::neo_token) fn registered_candidate_entries<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> CoreResult<Vec<(ECPoint, BigInt, StorageKey, StorageItem)>> {
        let prefix = Self::candidate_prefix_key();
        let mut out = Vec::new();
        let blocked_accounts = crate::PolicyContract::blocked_accounts_snapshot(snapshot);
        for (key, item) in snapshot.find(Some(&prefix), SeekDirection::Forward) {
            let key_bytes = key.key();
            if key_bytes.len() < 34 {
                continue;
            }
            // Decode state before pubkey decompression so unregistered rows stay
            // cheap, then carry the decoded pubkey/votes through to avoid a
            // second EC-point decompression in read_registered_candidates.
            let (registered, votes) = match Self::decode_candidate_state(&item.value_bytes()) {
                Ok(state) => state,
                Err(error) => {
                    if ECPoint::from_bytes(&key_bytes[1..34]).is_err() {
                        continue;
                    }
                    return Err(error);
                }
            };
            if !registered {
                continue;
            }
            let Ok(pubkey) = ECPoint::from_bytes(&key_bytes[1..34]) else {
                continue;
            };
            if !candidate_is_blocked_in(&blocked_accounts, &pubkey) {
                out.push((pubkey, votes, key, item));
            }
        }
        Ok(out)
    }

    /// [`registered_candidate_entries`] projected to `(pubkey, votes)` pairs - the
    /// shape consumed by `getCandidates` and the committee recompute.
    pub(in crate::neo_token) fn read_registered_candidates<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
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
    /// `CandidateStateChanged` for a fresh registration unconditionally - C#
    /// `RegisterInternal` calls `SendNotification` with no hardfork guard, and
    /// native `SendNotification` ignores the method's AllowNotify call flag.
    /// `method` labels errors with the invoking ABI method.
    pub(in crate::neo_token) fn register_internal<
        P: neo_execution::native_contract_provider::NativeContractProvider + 'static,
        D: neo_execution::Diagnostic + 'static,
        B: neo_storage::CacheRead,
    >(
        &self,
        engine: &mut ApplicationEngine<P, D, B>,
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
    pub(in crate::neo_token) fn candidate_vote<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
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
    pub(in crate::neo_token) fn candidates_to_array_bytes(
        candidates: &[(ECPoint, BigInt)],
    ) -> CoreResult<Vec<u8>> {
        let array = StackValue::Array(
            candidates
                .iter()
                .map(|(pk, votes)| {
                    StackValue::Struct(vec![
                        StackValue::ByteString(pk.to_bytes()),
                        StackValue::BigInteger(votes.to_signed_bytes_le()),
                    ])
                })
                .collect::<Vec<_>>(),
        );
        BinarySerializer::serialize_stack_value_default(&array)
            .map_err(|e| CoreError::invalid_operation(format!("getCandidates: {e}")))
    }
}

pub(super) fn candidate_is_blocked<B: neo_storage::CacheRead>(
    snapshot: &DataCache<B>,
    pubkey: &ECPoint,
) -> bool {
    let account = candidate_signature_account(pubkey);
    snapshot
        .get(&crate::PolicyContract::blocked_account_key(&account))
        .is_some()
}

pub(super) fn candidate_is_blocked_in(
    blocked_accounts: &HashSet<UInt160>,
    pubkey: &ECPoint,
) -> bool {
    blocked_accounts.contains(&candidate_signature_account(pubkey))
}

pub(crate) fn candidate_signature_account(pubkey: &ECPoint) -> UInt160 {
    let cache = CANDIDATE_SIGNATURE_ACCOUNT_CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    if let Some(account) = cache.read().unwrap_or_else(|e| e.into_inner()).get(pubkey) {
        return *account;
    }
    let account = UInt160::from_script(&Contract::create_signature_redeem_script(pubkey.clone()));
    *cache
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .entry(pubkey.clone())
        .or_insert(account)
}

pub(super) fn push_top_committee_candidate(
    top: &mut Vec<(ECPoint, BigInt)>,
    limit: usize,
    candidate: (ECPoint, BigInt),
) {
    if limit == 0 {
        return;
    }
    if top.len() == limit {
        if let Some(worst) = top.last() {
            if !committee_candidate_order(&candidate, worst).is_lt() {
                return;
            }
        }
    }

    let insert_at = top
        .binary_search_by(|existing| committee_candidate_order(existing, &candidate))
        .unwrap_or_else(|index| index);
    top.insert(insert_at, candidate);
    if top.len() > limit {
        top.pop();
    }
}

fn committee_candidate_order(a: &(ECPoint, BigInt), b: &(ECPoint, BigInt)) -> std::cmp::Ordering {
    b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0))
}
