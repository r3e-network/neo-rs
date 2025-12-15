//
// committee.rs - Committee management and validator selection
//

use super::*;

impl NeoToken {
    /// Determines whether the committee should be refreshed at the specified height.
    /// Committee is refreshed when height is a multiple of committee_members_count.
    /// Matches C# NeoToken.ShouldRefreshCommittee.
    pub fn should_refresh_committee(height: u32, committee_members_count: usize) -> bool {
        if committee_members_count == 0 {
            return false;
        }
        height.is_multiple_of(committee_members_count as u32)
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

    pub(super) fn decode_committee_stack_item(item: StackItem) -> Result<Vec<ECPoint>, String> {
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

    pub(super) fn committee_from_cache_with_votes<S>(
        &self,
        snapshot: &S,
        settings: &ProtocolSettings,
    ) -> CoreResult<Vec<(ECPoint, BigInt)>>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let key = StorageKey::create(Self::ID, Self::PREFIX_COMMITTEE);
        if let Some(item) = snapshot.try_get(&key) {
            let bytes = item.get_value();
            if !bytes.is_empty() {
                if let Ok(stack_item) =
                    BinarySerializer::deserialize(&bytes, &ExecutionEngineLimits::default(), None)
                {
                    if let Ok(values) = Self::decode_committee_with_votes(stack_item) {
                        if !values.is_empty() {
                            return Ok(values);
                        }
                    }
                }
            }
        }
        self.compute_committee_members(snapshot, settings)
    }

    pub(super) fn decode_committee_with_votes(item: StackItem) -> Result<Vec<(ECPoint, BigInt)>, String> {
        fn stack_item_to_bytes(item: &StackItem) -> Option<Vec<u8>> {
            match item {
                StackItem::ByteString(bytes) => Some(bytes.clone()),
                StackItem::Buffer(buffer) => Some(buffer.data().to_vec()),
                _ => None,
            }
        }

        fn decode_entry(entry: &StackItem) -> Result<Option<(ECPoint, BigInt)>, String> {
            let elements: Vec<StackItem> = match entry {
                StackItem::Struct(structure) => structure.items().to_vec(),
                StackItem::Array(array) => array.items().to_vec(),
                _ => return Ok(None),
            };

            if elements.len() < 2 {
                return Ok(None);
            }

            let key_bytes = stack_item_to_bytes(&elements[0])
                .ok_or_else(|| "committee entry public key must be byte array".to_string())?;
            let point = ECPoint::from_bytes(&key_bytes)
                .map_err(|e| format!("invalid committee public key: {e}"))?;
            let votes = elements[1]
                .as_int()
                .map_err(|e| format!("invalid committee votes: {e}"))?;

            Ok(Some((point, votes)))
        }

        match item {
            StackItem::Array(array) => {
                let mut committee = Vec::with_capacity(array.len());
                for entry in array.items() {
                    if let Some(value) = decode_entry(entry)? {
                        committee.push(value);
                    }
                }
                Ok(committee)
            }
            StackItem::Struct(structure) => {
                let mut committee = Vec::with_capacity(structure.len());
                for entry in structure.items() {
                    if let Some(value) = decode_entry(entry)? {
                        committee.push(value);
                    }
                }
                Ok(committee)
            }
            _ => Err("unexpected committee cache format".to_string()),
        }
    }

    pub(super) fn encode_committee_with_votes(committee: &[(ECPoint, BigInt)]) -> CoreResult<Vec<u8>> {
        let items: Vec<StackItem> = committee
            .iter()
            .map(|(pk, votes)| {
                StackItem::from_struct(vec![
                    StackItem::from_byte_string(pk.as_bytes().to_vec()),
                    StackItem::from_int(votes.clone()),
                ])
            })
            .collect();
        let array = StackItem::from_array(items);
        BinarySerializer::serialize(&array, &ExecutionEngineLimits::default())
            .map_err(CoreError::native_contract)
    }

    pub(super) fn compute_committee_members<S>(
        &self,
        snapshot: &S,
        settings: &ProtocolSettings,
    ) -> CoreResult<Vec<(ECPoint, BigInt)>>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let voters_key = StorageKey::create(Self::ID, Self::PREFIX_VOTERS_COUNT);
        let voters_count = snapshot
            .try_get(&voters_key)
            .map(|item| item.to_bigint())
            .unwrap_or_else(BigInt::zero);

        let candidates = self.get_candidates_internal(snapshot)?;

        let committee_members_count = settings.committee_members_count();
        let turnout_low = &voters_count * BigInt::from(5i64) < BigInt::from(Self::TOTAL_SUPPLY);

        if turnout_low || candidates.len() < committee_members_count {
            let mut standby = Vec::with_capacity(settings.standby_committee.len());
            for pk in &settings.standby_committee {
                let votes = candidates
                    .iter()
                    .find(|(c_pk, _)| c_pk == pk)
                    .map(|(_, v)| v.clone())
                    .unwrap_or_else(BigInt::zero);
                standby.push((pk.clone(), votes));
            }
            return Ok(standby);
        }

        let mut ordered = candidates;
        ordered.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        Ok(ordered.into_iter().take(committee_members_count).collect())
    }

    pub fn compute_next_block_validators_snapshot<S>(
        &self,
        snapshot: &S,
        settings: &ProtocolSettings,
    ) -> CoreResult<Vec<ECPoint>>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let validators_count = settings.validators_count as usize;
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
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
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
