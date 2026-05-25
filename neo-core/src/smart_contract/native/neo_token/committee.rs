//
// committee.rs - Committee management and validator selection
//

use super::*;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, HashMap};

fn committee_public_key_from_stack_value(value: &StackValue) -> Result<ECPoint, String> {
    let bytes = match value {
        StackValue::ByteString(bytes) | StackValue::Buffer(bytes) => bytes,
        _ => return Err("committee entry public key must be byte array".to_string()),
    };

    ECPoint::from_bytes(bytes).map_err(|e| format!("invalid committee public key: {e}"))
}

impl NeoToken {
    /// Determines whether the committee should be refreshed at the specified height.
    /// Committee is refreshed when height is a multiple of committee_members_count.
    /// Matches C# NeoToken.ShouldRefreshCommittee.
    pub fn should_refresh_committee(height: u32, committee_members_count: usize) -> bool {
        if committee_members_count == 0 {
            return false;
        }
        let count_u32 = u32::try_from(committee_members_count).unwrap_or(u32::MAX);
        height % count_u32 == 0
    }

    /// Attempts to read the current committee from the snapshot-backed storage used by the
    /// native NEO contract. Returns `None` when the committee cache has not been populated yet.
    pub fn committee_from_snapshot<S>(&self, snapshot: &S) -> Option<Vec<ECPoint>>
    where
        S: ReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let key = StorageKey::create(Self::ID, Self::PREFIX_COMMITTEE);
        let item = snapshot.try_get(&key)?;
        let bytes = item.value_bytes();
        let stack_value = BinarySerializer::deserialize_stack_value(&bytes).ok()?;

        Self::decode_committee_stack_value(stack_value).ok()
    }

    pub(super) fn decode_committee_stack_value(value: StackValue) -> Result<Vec<ECPoint>, String> {
        fn decode_entry(entry: &StackValue) -> Result<Option<ECPoint>, String> {
            let elements = match entry {
                StackValue::Struct(items) | StackValue::Array(items) => items,
                _ => return Ok(None),
            };

            let first = elements
                .first()
                .ok_or_else(|| "committee entry missing public key".to_string())?;
            Ok(Some(committee_public_key_from_stack_value(first)?))
        }

        match value {
            StackValue::Array(items) | StackValue::Struct(items) => {
                let mut committee = Vec::with_capacity(items.len());
                for entry in &items {
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

    pub(super) fn committee_from_cache_with_votes<S>(
        &self,
        snapshot: &S,
        settings: &ProtocolSettings,
    ) -> CoreResult<Vec<(ECPoint, BigInt)>>
    where
        S: ReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let key = StorageKey::create(Self::ID, Self::PREFIX_COMMITTEE);
        if let Some(item) = snapshot.try_get(&key) {
            let bytes = item.value_bytes();
            if !bytes.is_empty() {
                if let Ok(stack_value) = BinarySerializer::deserialize_stack_value(&bytes) {
                    if let Ok(values) = Self::decode_committee_with_votes_value(stack_value) {
                        if !values.is_empty() {
                            return Ok(values);
                        }
                    }
                }
            }
        }
        self.compute_committee_members(snapshot, settings)
    }

    pub(super) fn decode_committee_with_votes_value(
        value: StackValue,
    ) -> Result<Vec<(ECPoint, BigInt)>, String> {
        fn stack_value_to_bigint(value: &StackValue) -> Option<BigInt> {
            match value {
                StackValue::Integer(value) => Some(BigInt::from(*value)),
                StackValue::Boolean(value) => Some(BigInt::from(i32::from(*value))),
                StackValue::BigInteger(bytes)
                | StackValue::ByteString(bytes)
                | StackValue::Buffer(bytes) => Some(BigInt::from_signed_bytes_le(bytes)),
                _ => None,
            }
        }

        fn decode_entry(entry: &StackValue) -> Result<Option<(ECPoint, BigInt)>, String> {
            let elements = match entry {
                StackValue::Struct(items) | StackValue::Array(items) => items,
                _ => return Ok(None),
            };

            if elements.len() < 2 {
                return Ok(None);
            }

            let point = committee_public_key_from_stack_value(&elements[0])?;
            let votes = stack_value_to_bigint(&elements[1])
                .ok_or_else(|| "invalid committee votes".to_string())?;

            Ok(Some((point, votes)))
        }

        match value {
            StackValue::Array(items) | StackValue::Struct(items) => {
                let mut committee = Vec::with_capacity(items.len());
                for entry in &items {
                    if let Some(value) = decode_entry(entry)? {
                        committee.push(value);
                    }
                }
                Ok(committee)
            }
            _ => Err("unexpected committee cache format".to_string()),
        }
    }

    pub(super) fn encode_committee_with_votes(
        committee: &[(ECPoint, BigInt)],
    ) -> CoreResult<Vec<u8>> {
        let value = StackValue::Array(
            committee
                .iter()
                .map(|(pk, votes)| {
                    StackValue::Struct(vec![
                        StackValue::ByteString(pk.as_bytes().to_vec()),
                        StackValue::BigInteger(votes.to_signed_bytes_le()),
                    ])
                })
                .collect(),
        );
        BinarySerializer::serialize_stack_value(&value, &ExecutionEngineLimits::default())
            .map_err(CoreError::native_contract)
    }

    pub(super) fn compute_committee_members<S>(
        &self,
        snapshot: &S,
        settings: &ProtocolSettings,
    ) -> CoreResult<Vec<(ECPoint, BigInt)>>
    where
        S: ReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let voters_key = StorageKey::create(Self::ID, Self::PREFIX_VOTERS_COUNT);
        let voters_count = snapshot
            .try_get(&voters_key)
            .map(|item| item.to_bigint())
            .unwrap_or_else(BigInt::zero);

        let committee_members_count = settings.committee_members_count();
        let turnout_low = &voters_count * BigInt::from(5i64) < BigInt::from(Self::TOTAL_SUPPLY);

        if turnout_low {
            return self.standby_committee_votes(snapshot, settings);
        }

        let candidates = self.get_candidates_internal(snapshot)?;
        if candidates.len() < committee_members_count {
            let mut candidate_votes: BTreeMap<ECPoint, BigInt> = BTreeMap::new();
            for (pk, votes) in candidates {
                candidate_votes.insert(pk, votes);
            }
            return Ok(settings
                .standby_committee
                .iter()
                .map(|pk| {
                    let votes = candidate_votes
                        .get(pk)
                        .cloned()
                        .unwrap_or_else(BigInt::zero);
                    (pk.clone(), votes)
                })
                .collect());
        }

        #[derive(Clone, Eq, PartialEq)]
        struct RankedCandidate {
            pubkey: ECPoint,
            votes: BigInt,
        }

        impl Ord for RankedCandidate {
            fn cmp(&self, other: &Self) -> std::cmp::Ordering {
                self.votes
                    .cmp(&other.votes)
                    .then_with(|| other.pubkey.cmp(&self.pubkey))
            }
        }

        impl PartialOrd for RankedCandidate {
            fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(other))
            }
        }

        let mut top_candidates: BinaryHeap<Reverse<RankedCandidate>> =
            BinaryHeap::with_capacity(committee_members_count.saturating_add(1));
        for (pubkey, votes) in candidates {
            top_candidates.push(Reverse(RankedCandidate { pubkey, votes }));
            if top_candidates.len() > committee_members_count {
                let _ = top_candidates.pop();
            }
        }

        let mut ordered: Vec<(ECPoint, BigInt)> = top_candidates
            .into_iter()
            .map(|Reverse(candidate)| (candidate.pubkey, candidate.votes))
            .collect();
        ordered.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        Ok(ordered)
    }

    fn standby_committee_votes<S>(
        &self,
        snapshot: &S,
        settings: &ProtocolSettings,
    ) -> CoreResult<Vec<(ECPoint, BigInt)>>
    where
        S: ReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let standby = &settings.standby_committee;
        if standby.is_empty() {
            return Ok(Vec::new());
        }

        let mut standby_index: HashMap<Vec<u8>, usize> = HashMap::with_capacity(standby.len());
        for (index, pk) in standby.iter().enumerate() {
            standby_index.insert(pk.as_bytes().to_vec(), index);
        }

        let policy = PolicyContract::new();
        let blocked_accounts = policy.blocked_accounts_snapshot(snapshot);
        let has_blocked_accounts = !blocked_accounts.is_empty();

        let mut votes_by_index = vec![BigInt::zero(); standby.len()];
        let prefix = StorageKey::create(Self::ID, Self::PREFIX_CANDIDATE);
        for (key, item) in snapshot.find(Some(&prefix), SeekDirection::Forward) {
            if key.id != Self::ID {
                continue;
            }
            let suffix = key.suffix();
            if suffix.first().copied() != Some(Self::PREFIX_CANDIDATE) {
                continue;
            }
            let pk_bytes = &suffix[1..];
            let Some(&index) = standby_index.get(pk_bytes) else {
                continue;
            };

            let state =
                CandidateState::from_storage_item(&item).map_err(CoreError::native_contract)?;
            if !state.registered {
                continue;
            }

            if has_blocked_accounts {
                let Ok(pk) = ECPoint::from_bytes(pk_bytes) else {
                    continue;
                };
                let account = Contract::create_signature_contract(pk).script_hash();
                if blocked_accounts.contains(&account) {
                    continue;
                }
            }

            votes_by_index[index] = state.votes;
        }

        Ok(standby
            .iter()
            .cloned()
            .zip(votes_by_index)
            .collect::<Vec<_>>())
    }

    pub fn compute_next_block_validators_snapshot<S>(
        &self,
        snapshot: &S,
        settings: &ProtocolSettings,
    ) -> CoreResult<Vec<ECPoint>>
    where
        S: ReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let validators_count = usize::try_from(settings.validators_count.max(0)).unwrap_or(0);
        let committee = self.compute_committee_members(snapshot, settings)?;
        let mut validators: Vec<ECPoint> = committee
            .into_iter()
            .take(validators_count)
            .map(|(pk, _)| pk)
            .collect();
        validators.sort();
        Ok(validators)
    }

    pub fn get_next_block_validators_snapshot<S>(
        &self,
        snapshot: &S,
        validators_count: usize,
        settings: &ProtocolSettings,
    ) -> CoreResult<Vec<ECPoint>>
    where
        S: ReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let committee = self.committee_from_cache_with_votes(snapshot, settings)?;
        let mut validators: Vec<ECPoint> = committee
            .into_iter()
            .take(validators_count)
            .map(|(pk, _)| pk)
            .collect();
        validators.sort();
        Ok(validators)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_committee_point() -> ECPoint {
        let encoded =
            hex::decode("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c")
                .expect("hex");
        ECPoint::from_bytes(&encoded).expect("valid ECPoint")
    }

    #[test]
    fn committee_public_key_from_stack_value_accepts_byte_arrays() {
        let point = sample_committee_point();
        let bytes = point.as_bytes().to_vec();

        assert_eq!(
            committee_public_key_from_stack_value(&StackValue::ByteString(bytes.clone())).unwrap(),
            point
        );
        assert_eq!(
            committee_public_key_from_stack_value(&StackValue::Buffer(bytes)).unwrap(),
            point
        );
    }

    #[test]
    fn committee_public_key_from_stack_value_rejects_non_byte_arrays() {
        let invalid_values = [
            StackValue::Integer(1),
            StackValue::Boolean(true),
            StackValue::BigInteger(vec![1]),
            StackValue::Null,
        ];

        for value in invalid_values {
            assert_eq!(
                committee_public_key_from_stack_value(&value).unwrap_err(),
                "committee entry public key must be byte array"
            );
        }
    }

    #[test]
    fn committee_public_key_from_stack_value_reports_invalid_points() {
        let err = committee_public_key_from_stack_value(&StackValue::ByteString(vec![1, 2, 3]))
            .unwrap_err();

        assert!(err.starts_with("invalid committee public key:"));
    }
}
