use super::*;

impl LedgerContract {
    pub(super) fn parse_uint256_argument(data: &[u8], name: &str) -> Result<UInt256> {
        UInt256::from_bytes(data)
            .map_err(|e| Error::invalid_argument(format!("Invalid {name}: {e}")))
    }

    pub(super) fn parse_uint256_storage(data: &[u8], name: &str) -> Result<UInt256> {
        UInt256::from_bytes(data).map_err(|e| Error::invalid_data(format!("Invalid {name}: {e}")))
    }

    pub(super) fn parse_transaction_hash(data: &[u8]) -> Result<UInt256> {
        Self::parse_uint256_argument(data, "transaction hash")
    }

    pub(super) fn parse_index_or_hash(&self, data: &[u8], name: &str) -> Result<HashOrIndex> {
        if data.len() == 32 {
            let hash = Self::parse_uint256_argument(data, name)?;
            Ok(HashOrIndex::Hash(hash))
        } else if data.len() < 32 {
            let index = BigInt::from_signed_bytes_le(data)
                .to_u32()
                .ok_or_else(|| Error::invalid_argument(format!("Invalid {name} value")))?;
            Ok(HashOrIndex::Index(index))
        } else {
            Err(Error::invalid_argument(format!(
                "Invalid {name} length: {}",
                data.len()
            )))
        }
    }

    pub(super) fn serialize_stack_item(item: StackItem) -> Result<Vec<u8>> {
        BinarySerializer::serialize_default(&item)
            .map_err(|e| Error::serialization(format!("Failed to serialize ledger result: {e}")))
    }

    pub(super) fn resolve_max_traceable_blocks<S>(
        &self,
        engine: &ApplicationEngine,
        snapshot: &S,
    ) -> u32
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let settings = engine.protocol_settings();
        let mut value = if engine.is_hardfork_enabled(Hardfork::HfEchidna) {
            max_traceable_blocks_from_snapshot(snapshot, settings.max_traceable_blocks)
        } else {
            settings.max_traceable_blocks
        };

        if value == 0 {
            value = settings.max_traceable_blocks;
        }

        value = value.min(PolicyContract::MAX_MAX_TRACEABLE_BLOCKS);
        value.max(1)
    }

    pub(super) fn is_traceable_block(
        current_index: u32,
        target_index: u32,
        max_traceable: u32,
    ) -> bool {
        if target_index > current_index {
            return false;
        }
        let window_end = target_index.saturating_add(max_traceable);
        window_end > current_index
    }

    pub(super) fn get_transaction_state_if_traceable<S>(
        &self,
        snapshot: &S,
        hash: &UInt256,
        current_index: u32,
        max_traceable: u32,
    ) -> Result<Option<PersistedTransactionState>>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        if let Some(state) = self.try_read_transaction_state(snapshot, hash)? {
            if Self::is_traceable_block(current_index, state.block_index(), max_traceable) {
                return Ok(Some(state));
            }
        }
        Ok(None)
    }

    pub(super) fn get_transaction_from_block<S>(
        &self,
        snapshot: &S,
        block_hash: &UInt256,
        tx_index: u32,
        current_index: u32,
        max_traceable: u32,
    ) -> Result<Option<PersistedTransactionState>>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        if let Some(block) = self.try_read_block(snapshot, block_hash)? {
            if !Self::is_traceable_block(current_index, block.index(), max_traceable) {
                return Ok(None);
            }

            let tx_index = tx_index as usize;
            if tx_index >= block.transactions.len() {
                return Err(Error::invalid_argument(
                    "Transaction index out of range".to_string(),
                ));
            }

            let tx = &block.transactions[tx_index];
            return self.get_transaction_state_if_traceable(
                snapshot,
                &tx.hash(),
                current_index,
                max_traceable,
            );
        }
        Ok(None)
    }
}
