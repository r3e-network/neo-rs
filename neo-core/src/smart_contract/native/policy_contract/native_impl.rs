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

        let mut events = vec![
            ContractEventDescriptor::new(
                Self::MILLISECONDS_PER_BLOCK_CHANGED_EVENT_NAME.to_string(),
                vec![
                    ContractParameterDefinition::new(
                        "old".to_string(),
                        ContractParameterType::Integer,
                    )
                    .expect("MillisecondsPerBlockChanged.old"),
                    ContractParameterDefinition::new(
                        "new".to_string(),
                        ContractParameterType::Integer,
                    )
                    .expect("MillisecondsPerBlockChanged.new"),
                ],
            )
            .expect("MillisecondsPerBlockChanged event descriptor"),
        ];

        if settings.is_hardfork_enabled(Hardfork::HfFaun, block_height) {
            events.push(
                ContractEventDescriptor::new(
                    "WhitelistFeeChanged".to_string(),
                    vec![
                        ContractParameterDefinition::new(
                            "contract".to_string(),
                            ContractParameterType::Hash160,
                        )
                        .expect("WhitelistFeeChanged.contract"),
                        ContractParameterDefinition::new(
                            "method".to_string(),
                            ContractParameterType::String,
                        )
                        .expect("WhitelistFeeChanged.method"),
                        ContractParameterDefinition::new(
                            "argCount".to_string(),
                            ContractParameterType::Integer,
                        )
                        .expect("WhitelistFeeChanged.argCount"),
                        ContractParameterDefinition::new(
                            "fee".to_string(),
                            ContractParameterType::Any,
                        )
                        .expect("WhitelistFeeChanged.fee"),
                    ],
                )
                .expect("WhitelistFeeChanged event descriptor"),
            );

            events.push(
                ContractEventDescriptor::new(
                    "RecoveredFund".to_string(),
                    vec![
                        ContractParameterDefinition::new(
                            "account".to_string(),
                            ContractParameterType::Hash160,
                        )
                        .expect("RecoveredFund.account"),
                    ],
                )
                .expect("RecoveredFund event descriptor"),
            );
        }

        events
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

        if engine.is_hardfork_enabled(Hardfork::HfFaun) {
            if let Some(&faun_height) = engine.protocol_settings().hardforks.get(&Hardfork::HfFaun)
            {
                if engine.current_block_index() == faun_height {
                    // v3.9.1: Scale exec fee factor by ApplicationEngine.FeeFactor at Faun activation.
                    //
                    // C# ref: `var item = engine.SnapshotCache.GetAndChange(_execFeeFactor)
                    //           ?? throw new InvalidOperationException();
                    //          item.Set((uint)(BigInteger)item * ApplicationEngine.FeeFactor);`
                    //
                    // NOTE: The `if let Some` + `value <= MAX_EXEC_FEE_FACTOR` guard is
                    // intentional.  In our Rust implementation, `initialize` is called
                    // TWICE per block: once from `register_native_contracts` (engine
                    // constructor) and once from `ContractManagement::on_persist`.  The
                    // C# code only calls `InitializeAsync` once (from OnPersist).  The
                    // guard prevents double-scaling (30 → 300000 → 3B) by ensuring the
                    // multiplication is idempotent: after the first scaling the value
                    // exceeds MAX_EXEC_FEE_FACTOR (100), so subsequent calls are no-ops.
                    if let Some(item) = snapshot_ref.try_get(&Self::exec_fee_factor_key()) {
                        let value = BigInt::from_signed_bytes_le(&item.get_value())
                            .to_u32()
                            .ok_or_else(|| {
                                Error::native_contract(
                                    "ExecFeeFactor exceeds u32 capacity".to_string(),
                                )
                            })?;
                        if value <= Self::MAX_EXEC_FEE_FACTOR {
                            let scaled = value.saturating_mul(
                                crate::smart_contract::application_engine::FEE_FACTOR as u32,
                            );
                            engine.set_storage(
                                Self::exec_fee_factor_key(),
                                StorageItem::from_bytes(Self::encode_u32(scaled)),
                            )?;
                        }
                    }

                    // v3.9.1: Add timestamp to ALL blocked accounts at Faun activation.
                    //
                    // C# ref: `engine.SnapshotCache.GetAndChange(key).Set(time)`
                    // for every entry under Prefix_BlockedAccount.  This ensures
                    // recoverFund has a valid timestamp for the 1-year waiting period.
                    let timestamp = engine
                        .get_current_block_time()
                        .map_err(Error::invalid_operation)?;
                    let timestamp_bytes = Self::encode_u64(timestamp);
                    let prefix_key = StorageKey::new(Self::ID, vec![Self::PREFIX_BLOCKED_ACCOUNT]);
                    let all_keys: Vec<StorageKey> = snapshot_ref
                        .find(
                            Some(&prefix_key),
                            crate::persistence::seek_direction::SeekDirection::Forward,
                        )
                        .map(|(key, _)| key)
                        .collect();
                    for key in all_keys {
                        engine
                            .set_storage(key, StorageItem::from_bytes(timestamp_bytes.clone()))?;
                    }
                }
            }
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
