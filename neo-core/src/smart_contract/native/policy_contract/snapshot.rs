//
// snapshot.rs - Snapshot reading methods for PolicyContract
//

use super::*;

impl PolicyContract {
    /// Reads the MaxTraceableBlocks value directly from storage.
    pub fn get_max_traceable_blocks_snapshot<S>(snapshot: &S) -> Option<u32>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let key = Self::max_traceable_blocks_key();
        snapshot
            .try_get(&key)
            .and_then(|item| BigInt::from_signed_bytes_le(&item.get_value()).to_u32())
    }

    /// Reads the MillisecondsPerBlock value directly from storage.
    pub fn get_milliseconds_per_block_snapshot<S>(snapshot: &S) -> Option<u32>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let key = Self::milliseconds_per_block_key();
        snapshot
            .try_get(&key)
            .and_then(|item| BigInt::from_signed_bytes_le(&item.get_value()).to_u32())
    }

    /// Reads FeePerByte from a snapshot, falling back to defaults if not configured.
    pub fn get_fee_per_byte_snapshot<S>(&self, snapshot: &S) -> Result<i64>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let key = Self::fee_per_byte_key();
        match snapshot.try_get(&key) {
            Some(item) => BigInt::from_signed_bytes_le(&item.get_value())
                .to_i64()
                .ok_or_else(|| Error::native_contract("FeePerByte exceeds i64 capacity")),
            None => Ok(Self::DEFAULT_FEE_PER_BYTE as i64),
        }
    }

    /// Reads ExecFeeFactor from a snapshot, falling back to defaults if not configured.
    pub fn get_exec_fee_factor_snapshot<S>(
        &self,
        snapshot: &S,
        settings: &ProtocolSettings,
        block_height: u32,
    ) -> Result<u32>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let key = Self::exec_fee_factor_key();
        match snapshot.try_get(&key) {
            Some(item) => {
                let value = BigInt::from_signed_bytes_le(&item.get_value())
                    .to_u32()
                    .ok_or_else(|| Error::native_contract("ExecFeeFactor exceeds u32 capacity"))?;
                if settings.is_hardfork_enabled(Hardfork::HfFaun, block_height) {
                    Ok(value / crate::smart_contract::application_engine::FEE_FACTOR as u32)
                } else {
                    Ok(value)
                }
            }
            None => Ok(Self::DEFAULT_EXEC_FEE_FACTOR),
        }
    }

    /// Reads the attribute fee for the given attribute type from the snapshot.
    pub fn get_attribute_fee_snapshot<S>(
        &self,
        snapshot: &S,
        attribute_type: TransactionAttributeType,
    ) -> Result<i64>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        self.get_attribute_fee_for_type(snapshot, attribute_type as u8)
    }

    /// Reads the attribute fee for the given attribute type (byte) from the snapshot.
    pub fn get_attribute_fee_for_type<S>(&self, snapshot: &S, attribute_type: u8) -> Result<i64>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let _ = TransactionAttributeType::from_byte(attribute_type).ok_or_else(|| {
            Error::invalid_operation(format!("Attribute type {attribute_type} is not supported."))
        })?;

        let key = Self::attribute_fee_key(attribute_type);
        match snapshot.try_get(&key) {
            Some(item) => BigInt::from_signed_bytes_le(&item.get_value())
                .to_i64()
                .ok_or_else(|| Error::native_contract("AttributeFee exceeds i64 capacity")),
            None => Ok(Self::DEFAULT_ATTRIBUTE_FEE as i64),
        }
    }

    /// Reads MaxValidUntilBlockIncrement from storage, defaulting to protocol settings.
    pub fn get_max_valid_until_block_increment_snapshot<S>(
        &self,
        snapshot: &S,
        settings: &ProtocolSettings,
    ) -> Result<u32>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let key = Self::max_valid_until_block_increment_key();
        match snapshot.try_get(&key) {
            Some(item) => BigInt::from_signed_bytes_le(&item.get_value())
                .to_u32()
                .ok_or_else(|| {
                    Error::native_contract(
                        "MaxValidUntilBlockIncrement exceeds u32 capacity".to_string(),
                    )
                }),
            None => Ok(settings.max_valid_until_block_increment),
        }
    }

    /// Checks whether the provided account hash is blocked in the snapshot.
    pub fn is_blocked_snapshot<S>(&self, snapshot: &S, account: &UInt160) -> Result<bool>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        Ok(snapshot
            .try_get(&Self::blocked_account_key(account))
            .is_some())
    }

    /// Gets the fixed fee for a whitelisted contract method, if it exists.
    pub fn get_whitelisted_fee(
        &self,
        snapshot: &crate::persistence::DataCache,
        contract_hash: &UInt160,
        method: &str,
        arg_count: u32,
    ) -> Result<Option<i64>> {
        use crate::smart_contract::binary_serializer::BinarySerializer;
        use crate::smart_contract::native::ContractManagement;
        use neo_vm::execution_engine_limits::ExecutionEngineLimits;

        // Retrieve the contract state
        let contract = ContractManagement::get_contract_from_snapshot(snapshot, contract_hash)?;

        if let Some(contract) = contract {
            // Find method descriptor
            let method_descriptor = contract
                .manifest
                .abi
                .get_method_ref(method, arg_count as usize);

            if let Some(descriptor) = method_descriptor {
                let key = Self::whitelist_fee_contract_key(contract_hash, descriptor.offset);

                // Check storage
                if let Some(item) = snapshot.try_get(&key) {
                    let bytes = item.get_value();
                    if bytes.is_empty() {
                        return Ok(None);
                    }

                    let stack_item = BinarySerializer::deserialize(
                        &bytes,
                        &ExecutionEngineLimits::default(),
                        None,
                    )
                    .map_err(|e| {
                        Error::native_contract(format!("Failed to deserialize whitelist info: {e}"))
                    })?;

                    let mut whitelist = WhitelistedContract::default();
                    whitelist.from_stack_item(stack_item);
                    return Ok(Some(whitelist.fixed_fee));
                }
            }
        }
        Ok(None)
    }
}
