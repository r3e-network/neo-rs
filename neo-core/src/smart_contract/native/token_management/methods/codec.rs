impl TokenManagement {
    fn serialize_stack_item(item: &StackItem) -> CoreResult<Vec<u8>> {
        BinarySerializer::serialize_default(item).map_err(CoreError::native_contract)
    }

    fn deserialize_stack_item(bytes: &[u8]) -> CoreResult<StackItem> {
        BinarySerializer::deserialize_default(bytes).map_err(CoreError::native_contract)
    }

    fn serialize_interoperable<T: IInteroperable>(value: &T) -> CoreResult<Vec<u8>> {
        let stack_item = value.to_stack_item()?;
        Self::serialize_stack_item(&stack_item)
    }

    fn deserialize_interoperable<T: IInteroperable + Default>(bytes: &[u8]) -> CoreResult<T> {
        let stack_item = Self::deserialize_stack_item(bytes)?;
        let mut state = T::default();
        state.from_stack_item(stack_item)?;
        Ok(state)
    }

    fn deserialize_interoperable_opt<T: IInteroperable + Default>(bytes: &[u8]) -> Option<T> {
        Self::deserialize_interoperable(bytes).ok()
    }

    fn deserialize_token_state(data: &[u8]) -> Option<TokenState> {
        Self::deserialize_interoperable_opt(data)
    }

    fn deserialize_account_state(data: &[u8]) -> Option<AccountState> {
        Self::deserialize_interoperable_opt(data)
    }

    fn deserialize_nft_state(data: &[u8]) -> Option<NFTState> {
        Self::deserialize_interoperable_opt(data)
    }

    fn encode_bigint(value: &BigInt) -> Vec<u8> {
        let mut bytes = value.to_signed_bytes_le();
        if bytes.is_empty() {
            bytes.push(0);
        }
        bytes
    }

    fn token_state_key(asset_id: &UInt160) -> StorageKey {
        StorageKey::create_with_uint160(ID, PREFIX_TOKEN_STATE, asset_id)
    }

    fn token_state_suffix(asset_id: &UInt160) -> Vec<u8> {
        Self::token_state_key(asset_id).suffix().to_vec()
    }

    fn nft_state_suffix(nft_id: &UInt160) -> Vec<u8> {
        StorageKey::create_with_uint160(ID, PREFIX_NFT_STATE, nft_id)
            .suffix()
            .to_vec()
    }

    fn account_state_suffix(account: &UInt160, asset_id: &UInt160) -> Vec<u8> {
        [
            vec![PREFIX_ACCOUNT_STATE],
            account.to_bytes().to_vec(),
            asset_id.to_bytes().to_vec(),
        ]
        .concat()
    }

    fn parse_non_negative_amount(arg: &[u8], negative_error: &'static str) -> CoreResult<BigInt> {
        let amount = BigInt::from_signed_bytes_le(arg);
        if amount.is_negative() {
            return Err(CoreError::native_contract(negative_error));
        }
        Ok(amount)
    }

    fn parse_non_negative_optional_amount(
        args: &[Vec<u8>],
        index: usize,
        default: i64,
        negative_error: &'static str,
    ) -> CoreResult<BigInt> {
        if let Some(value) = args.get(index) {
            return Self::parse_non_negative_amount(value, negative_error);
        }
        Ok(BigInt::from(default))
    }

    fn write_token_state(
        &self,
        context: &StorageContext,
        engine: &mut ApplicationEngine,
        asset_id: &UInt160,
        state: &TokenState,
    ) -> CoreResult<()> {
        let key = Self::token_state_suffix(asset_id);
        let bytes = Self::serialize_interoperable(state)?;
        engine.put_storage_item(context, &key, &bytes)?;
        Ok(())
    }

    fn write_nft_state(
        &self,
        context: &StorageContext,
        engine: &mut ApplicationEngine,
        nft_id: &UInt160,
        state: &NFTState,
    ) -> CoreResult<()> {
        let key = Self::nft_state_suffix(nft_id);
        let bytes = Self::serialize_interoperable(state)?;
        engine.put_storage_item(context, &key, &bytes)?;
        Ok(())
    }

    fn merge_entries_from_snapshots(
        engine: &ApplicationEngine,
        prefix: &StorageKey,
    ) -> Vec<(StorageKey, StorageItem)> {
        let snapshot = engine.snapshot_cache();
        let mut entries_map = std::collections::BTreeMap::new();

        for (key, value) in snapshot.as_ref().find(Some(prefix), SeekDirection::Forward) {
            entries_map.insert(key, value);
        }
        for (key, value) in engine
            .original_snapshot_cache()
            .find(Some(prefix), SeekDirection::Forward)
        {
            entries_map.entry(key).or_insert(value);
        }

        let mut entries: Vec<(StorageKey, StorageItem)> = entries_map.into_iter().collect();
        entries.sort_by(|a, b| a.0.suffix().cmp(b.0.suffix()));
        entries
    }

    fn merge_entries_from_snapshots_with_tracking(
        engine: &ApplicationEngine,
        prefix: &StorageKey,
        prefix_byte: u8,
    ) -> Vec<(StorageKey, StorageItem)> {
        let snapshot = engine.snapshot_cache();
        let mut entries_map = std::collections::BTreeMap::new();
        let mut snapshot_keys: std::collections::HashSet<Vec<u8>> =
            std::collections::HashSet::new();

        for (key, value) in snapshot.as_ref().find(Some(prefix), SeekDirection::Forward) {
            entries_map.insert(key.clone(), value);
            snapshot_keys.insert(key.suffix().to_vec());
        }

        for (key, _trackable) in snapshot.tracked_items() {
            if key.id != ID {
                continue;
            }
            let suffix = key.suffix();
            if suffix.is_empty() || suffix[0] != prefix_byte {
                continue;
            }
            snapshot_keys.insert(suffix.to_vec());
        }

        for (key, value) in engine
            .original_snapshot_cache()
            .find(Some(prefix), SeekDirection::Forward)
        {
            if !snapshot_keys.contains(key.suffix()) {
                entries_map.entry(key).or_insert(value);
            }
        }

        let mut entries: Vec<(StorageKey, StorageItem)> = entries_map.into_iter().collect();
        entries.sort_by(|a, b| a.0.suffix().cmp(b.0.suffix()));
        entries
    }

    fn filter_entries_by_hash_suffix(
        entries: Vec<(StorageKey, StorageItem)>,
        min_suffix_len: usize,
        hash_offset: usize,
        target: UInt160,
    ) -> Vec<(StorageKey, StorageItem)> {
        entries
            .into_iter()
            .filter(|(key, _)| {
                let suffix = key.suffix();
                if suffix.len() < min_suffix_len {
                    return false;
                }
                let parsed = Self::try_parse_uint160(&suffix[hash_offset..hash_offset + 20]);
                parsed == Some(target)
            })
            .collect()
    }

    fn store_iterator_id_bytes(
        engine: &mut ApplicationEngine,
        entries: Vec<(StorageKey, StorageItem)>,
        prefix_len: usize,
        options: FindOptions,
    ) -> CoreResult<Vec<u8>> {
        let iterator = StorageIterator::new(entries, prefix_len, options);
        let iterator_id = engine
            .store_storage_iterator(iterator)
            .map_err(CoreError::native_contract)?;
        Ok(iterator_id.to_le_bytes().to_vec())
    }

    fn parse_uint160(bytes: &[u8], error: &'static str) -> CoreResult<UInt160> {
        UInt160::from_bytes(bytes).map_err(|_| CoreError::native_contract(error))
    }

    fn try_parse_uint160(bytes: &[u8]) -> Option<UInt160> {
        UInt160::from_bytes(bytes).ok()
    }

    pub fn get_asset_id(owner: &UInt160, name: &str) -> UInt160 {
        let name_bytes = name.as_bytes();
        let mut buffer = Vec::with_capacity(20 + name_bytes.len());
        buffer.extend_from_slice(&owner.as_bytes());
        buffer.extend_from_slice(name_bytes);
        let hash = NeoHash::hash160(&buffer);
        UInt160::from_bytes(&hash).unwrap_or_default()
    }
}
