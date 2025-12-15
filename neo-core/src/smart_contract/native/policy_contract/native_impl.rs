//
// native_impl.rs - NativeContract trait implementation for PolicyContract
//

use super::*;

impl NativeContract for PolicyContract {
    fn id(&self) -> i32 {
        self.id
    }

    fn hash(&self) -> UInt160 {
        self.hash
    }

    fn name(&self) -> &str {
        "PolicyContract"
    }

    fn methods(&self) -> &[NativeMethod] {
        &self.methods
    }

    fn events(
        &self,
        settings: &ProtocolSettings,
        block_height: u32,
    ) -> Vec<ContractEventDescriptor> {
        if !settings.is_hardfork_enabled(Hardfork::HfEchidna, block_height) {
            return Vec::new();
        }

        vec![ContractEventDescriptor::new(
            Self::MILLISECONDS_PER_BLOCK_CHANGED_EVENT_NAME.to_string(),
            vec![
                ContractParameterDefinition::new("old".to_string(), ContractParameterType::Integer)
                    .expect("MillisecondsPerBlockChanged.old"),
                ContractParameterDefinition::new("new".to_string(), ContractParameterType::Integer)
                    .expect("MillisecondsPerBlockChanged.new"),
            ],
        )
        .expect("MillisecondsPerBlockChanged event descriptor")]
    }

    fn initialize(&self, engine: &mut ApplicationEngine) -> Result<()> {
        let snapshot = engine.snapshot_cache();
        let snapshot_ref = snapshot.as_ref();

        if snapshot_ref.try_get(&Self::fee_per_byte_key()).is_none() {
            engine.set_storage(
                Self::fee_per_byte_key(),
                StorageItem::from_bytes(Self::encode_i64(Self::DEFAULT_FEE_PER_BYTE as i64)),
            )?;
        }

        if snapshot_ref.try_get(&Self::exec_fee_factor_key()).is_none() {
            engine.set_storage(
                Self::exec_fee_factor_key(),
                StorageItem::from_bytes(Self::encode_u32(Self::DEFAULT_EXEC_FEE_FACTOR)),
            )?;
        }

        if snapshot_ref.try_get(&Self::storage_price_key()).is_none() {
            engine.set_storage(
                Self::storage_price_key(),
                StorageItem::from_bytes(Self::encode_u32(Self::DEFAULT_STORAGE_PRICE)),
            )?;
        }

        if engine.is_hardfork_enabled(Hardfork::HfEchidna) {
            let notary_key =
                Self::attribute_fee_key(TransactionAttributeType::NotaryAssisted as u8);
            if snapshot_ref.try_get(&notary_key).is_none() {
                engine.set_storage(
                    notary_key,
                    StorageItem::from_bytes(Self::encode_u32(
                        Self::DEFAULT_NOTARY_ASSISTED_ATTRIBUTE_FEE,
                    )),
                )?;
            }

            if snapshot_ref
                .try_get(&Self::milliseconds_per_block_key())
                .is_none()
            {
                engine.set_storage(
                    Self::milliseconds_per_block_key(),
                    StorageItem::from_bytes(Self::encode_u32(
                        engine.protocol_settings().milliseconds_per_block,
                    )),
                )?;
            }

            if snapshot_ref
                .try_get(&Self::max_valid_until_block_increment_key())
                .is_none()
            {
                engine.set_storage(
                    Self::max_valid_until_block_increment_key(),
                    StorageItem::from_bytes(Self::encode_u32(
                        engine.protocol_settings().max_valid_until_block_increment,
                    )),
                )?;
            }

            if snapshot_ref
                .try_get(&Self::max_traceable_blocks_key())
                .is_none()
            {
                engine.set_storage(
                    Self::max_traceable_blocks_key(),
                    StorageItem::from_bytes(Self::encode_u32(
                        engine.protocol_settings().max_traceable_blocks,
                    )),
                )?;
            }
        }

        Ok(())
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        self.invoke_method(engine, method, args)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Default for PolicyContract {
    fn default() -> Self {
        Self::new()
    }
}
