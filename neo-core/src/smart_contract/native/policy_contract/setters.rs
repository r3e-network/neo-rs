//
// setters.rs - Setter methods for PolicyContract
//

use super::*;
use crate::smart_contract::native::security_fixes::{
    PermissionValidator, ReentrancyGuardType, SecurityContext,
};

impl PolicyContract {
    pub(super) fn set_fee_per_byte(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        // Enter reentrancy guard
        let _guard = SecurityContext::enter_guard(ReentrancyGuardType::PolicyUpdate)?;

        if args.len() != 1 {
            return Err(Error::native_contract(
                "setFeePerByte requires value argument".to_string(),
            ));
        }

        let value = Self::parse_i64_argument(&args[0], "value")?;

        // Validate range using security validator
        PermissionValidator::validate_range(value, 0, 100_000_000, "FeePerByte")
            .map_err(|e| Error::invalid_operation(e.to_string()))?;

        Self::assert_committee(engine)?;

        let context = engine.get_native_storage_context(&self.hash)?;
        engine.put_storage_item(
            &context,
            Self::fee_per_byte_key().suffix(),
            &Self::encode_i64(value),
        )?;

        Ok(Vec::new())
    }

    pub(super) fn set_exec_fee_factor(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.len() != 1 {
            return Err(Error::native_contract(
                "setExecFeeFactor requires value argument".to_string(),
            ));
        }

        let value = Self::parse_u32_argument(&args[0], "value")?;

        let max_value = if engine.is_hardfork_enabled(Hardfork::HfFaun) {
            Self::MAX_EXEC_FEE_FACTOR * crate::smart_contract::application_engine::FEE_FACTOR as u32
        } else {
            Self::MAX_EXEC_FEE_FACTOR
        };

        if value == 0 || value > max_value {
            return Err(Error::invalid_operation(format!(
                "ExecFeeFactor must be between [1, {}], got {value}",
                max_value
            )));
        }

        Self::assert_committee(engine)?;

        let context = engine.get_native_storage_context(&self.hash)?;
        engine.put_storage_item(
            &context,
            Self::exec_fee_factor_key().suffix(),
            &Self::encode_u32(value),
        )?;
        Ok(Vec::new())
    }

    pub(super) fn set_storage_price(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.len() != 1 {
            return Err(Error::native_contract(
                "setStoragePrice requires value argument".to_string(),
            ));
        }

        let value = Self::parse_u32_argument(&args[0], "value")?;
        if value == 0 || value > Self::MAX_STORAGE_PRICE {
            return Err(Error::invalid_operation(format!(
                "StoragePrice must be between [1, {}], got {value}",
                Self::MAX_STORAGE_PRICE
            )));
        }

        Self::assert_committee(engine)?;

        let context = engine.get_native_storage_context(&self.hash)?;
        engine.put_storage_item(
            &context,
            Self::storage_price_key().suffix(),
            &Self::encode_u32(value),
        )?;
        Ok(Vec::new())
    }

    pub(super) fn set_milliseconds_per_block(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.len() != 1 {
            return Err(Error::native_contract(
                "setMillisecondsPerBlock requires value argument".to_string(),
            ));
        }

        let value = Self::parse_u32_argument(&args[0], "value")?;
        if value == 0 || value > Self::MAX_MILLISECONDS_PER_BLOCK {
            return Err(Error::invalid_operation(format!(
                "MillisecondsPerBlock must be between [1, {}], got {value}",
                Self::MAX_MILLISECONDS_PER_BLOCK
            )));
        }

        Self::assert_committee(engine)?;

        let old_time = self.read_u32_setting(
            engine,
            Self::milliseconds_per_block_key().suffix(),
            engine.protocol_settings().milliseconds_per_block,
        )?;

        let context = engine.get_native_storage_context(&self.hash)?;
        engine.put_storage_item(
            &context,
            Self::milliseconds_per_block_key().suffix(),
            &Self::encode_u32(value),
        )?;

        engine
            .send_notification(
                self.hash,
                Self::MILLISECONDS_PER_BLOCK_CHANGED_EVENT_NAME.to_string(),
                vec![
                    StackItem::from_int(old_time as i64),
                    StackItem::from_int(value as i64),
                ],
            )
            .map_err(Error::native_contract)?;

        Ok(Vec::new())
    }

    pub(super) fn set_max_valid_until_block_increment(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.len() != 1 {
            return Err(Error::native_contract(
                "setMaxValidUntilBlockIncrement requires value argument".to_string(),
            ));
        }

        let value = Self::parse_u32_argument(&args[0], "value")?;
        if value == 0 || value > Self::MAX_MAX_VALID_UNTIL_BLOCK_INCREMENT {
            return Err(Error::invalid_operation(format!(
                "MaxValidUntilBlockIncrement must be between [1, {}], got {value}",
                Self::MAX_MAX_VALID_UNTIL_BLOCK_INCREMENT
            )));
        }

        let max_traceable_blocks = self.read_u32_setting(
            engine,
            Self::max_traceable_blocks_key().suffix(),
            engine.protocol_settings().max_traceable_blocks,
        )?;
        if value >= max_traceable_blocks {
            return Err(Error::invalid_operation(format!(
                "MaxValidUntilBlockIncrement must be lower than MaxTraceableBlocks ({value} vs {max_traceable_blocks})"
            )));
        }

        Self::assert_committee(engine)?;

        let context = engine.get_native_storage_context(&self.hash)?;
        engine.put_storage_item(
            &context,
            Self::max_valid_until_block_increment_key().suffix(),
            &Self::encode_u32(value),
        )?;
        Ok(Vec::new())
    }

    pub(super) fn set_max_traceable_blocks(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.len() != 1 {
            return Err(Error::native_contract(
                "setMaxTraceableBlocks requires value argument".to_string(),
            ));
        }

        let value = Self::parse_u32_argument(&args[0], "value")?;
        if value == 0 || value > Self::MAX_MAX_TRACEABLE_BLOCKS {
            return Err(Error::invalid_operation(format!(
                "MaxTraceableBlocks must be between [1, {}], got {value}",
                Self::MAX_MAX_TRACEABLE_BLOCKS
            )));
        }

        let old_value = self.read_u32_setting(
            engine,
            Self::max_traceable_blocks_key().suffix(),
            engine.protocol_settings().max_traceable_blocks,
        )?;
        if value > old_value {
            return Err(Error::invalid_operation(format!(
                "MaxTraceableBlocks can not be increased (old {old_value}, new {value})"
            )));
        }

        let max_valid_until = self.read_u32_setting(
            engine,
            Self::max_valid_until_block_increment_key().suffix(),
            engine.protocol_settings().max_valid_until_block_increment,
        )?;
        if value <= max_valid_until {
            return Err(Error::invalid_operation(format!(
                "MaxTraceableBlocks must be larger than MaxValidUntilBlockIncrement ({value} vs {max_valid_until})"
            )));
        }

        Self::assert_committee(engine)?;

        let context = engine.get_native_storage_context(&self.hash)?;
        engine.put_storage_item(
            &context,
            Self::max_traceable_blocks_key().suffix(),
            &Self::encode_u32(value),
        )?;
        Ok(Vec::new())
    }

    pub(super) fn set_attribute_fee(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.len() != 2 {
            return Err(Error::native_contract(
                "setAttributeFee requires attributeType and value arguments".to_string(),
            ));
        }

        let attribute_type = Self::parse_u8_argument(&args[0], "attributeType")?;
        let allow_notary_assisted = engine.is_hardfork_enabled(Hardfork::HfEchidna);
        Self::validate_attribute_type(attribute_type, allow_notary_assisted)?;

        let value = Self::parse_u32_argument(&args[1], "value")?;
        if value > Self::MAX_ATTRIBUTE_FEE {
            return Err(Error::invalid_operation(format!(
                "AttributeFee must be less than {}",
                Self::MAX_ATTRIBUTE_FEE
            )));
        }

        Self::assert_committee(engine)?;

        let context = engine.get_native_storage_context(&self.hash)?;
        engine.put_storage_item(
            &context,
            &Self::attribute_fee_suffix(attribute_type),
            &Self::encode_u32(value),
        )?;
        Ok(Vec::new())
    }

    pub(super) fn set_whitelist_fee_contract(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.len() != 4 {
            return Err(Error::native_contract(
                "setWhitelistFeeContract requires 4 arguments".to_string(),
            ));
        }

        let contract_hash = UInt160::from_bytes(&args[0])
            .map_err(|e| Error::invalid_argument(format!("Invalid contract hash: {e}")))?;
        let method = std::str::from_utf8(&args[1])
            .map_err(|_| Error::native_contract("Invalid method name string"))?;

        let arg_count = Self::parse_u32_argument(&args[2], "argCount")?;
        let fixed_fee = Self::parse_i64_argument(&args[3], "fixedFee")?;

        if fixed_fee < 0 {
            return Err(Error::native_contract(format!(
                "FixedFee must be non-negative, got {fixed_fee}"
            )));
        }

        Self::assert_committee(engine)?;

        let contract = crate::smart_contract::native::contract_management::ContractManagement::get_contract_from_snapshot(engine.snapshot_cache().as_ref(), &contract_hash)?
            .ok_or_else(|| Error::invalid_operation("Is not a valid contract"))?;

        let method_descriptor = contract
            .manifest
            .abi
            .methods
            .iter()
            .find(|m| m.name == method && m.parameters.len() == arg_count as usize)
            .ok_or_else(|| {
                Error::invalid_operation(format!(
                    "Method {method} with {arg_count} args was not found in {contract_hash}"
                ))
            })?;

        let key = Self::whitelist_fee_contract_key(&contract_hash, method_descriptor.offset);

        let whitelisted = WhitelistedContract {
            contract_hash,
            method: method.to_string(),
            arg_count,
            fixed_fee,
        };

        let bytes = crate::smart_contract::binary_serializer::BinarySerializer::serialize(
            &whitelisted.to_stack_item()?,
            &neo_vm::execution_engine_limits::ExecutionEngineLimits::default(),
        )
        .map_err(|e| Error::native_contract(format!("Failed to serialize whitelist info: {e}")))?;

        let context = engine.get_native_storage_context(&self.hash)?;
        engine.put_storage_item(&context, key.suffix(), &bytes)?;

        // Emit event
        // WhitelistChangedEventName = "WhitelistFeeChanged"
        engine
            .send_notification(
                self.hash,
                "WhitelistFeeChanged".to_string(),
                vec![
                    StackItem::ByteString(contract_hash.to_bytes().to_vec()),
                    StackItem::ByteString(method.as_bytes().to_vec()),
                    StackItem::Integer(BigInt::from(arg_count)),
                    StackItem::Integer(BigInt::from(fixed_fee)),
                ],
            )
            .map_err(Error::native_contract)?;

        Ok(Vec::new())
    }

    pub(super) fn remove_whitelist_fee_contract(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.len() != 3 {
            return Err(Error::native_contract(
                "removeWhitelistFeeContract requires 3 arguments".to_string(),
            ));
        }

        let contract_hash = UInt160::from_bytes(&args[0])
            .map_err(|e| Error::invalid_argument(format!("Invalid contract hash: {e}")))?;
        let method = std::str::from_utf8(&args[1])
            .map_err(|_| Error::native_contract("Invalid method name string"))?;
        let arg_count = Self::parse_u32_argument(&args[2], "argCount")?;

        Self::assert_committee(engine)?;

        let contract = crate::smart_contract::native::contract_management::ContractManagement::get_contract_from_snapshot(engine.snapshot_cache().as_ref(), &contract_hash)?
            .ok_or_else(|| Error::invalid_operation("Is not a valid contract"))?;

        let method_descriptor = contract
            .manifest
            .abi
            .methods
            .iter()
            .find(|m| m.name == method && m.parameters.len() == arg_count as usize)
            .ok_or_else(|| {
                Error::invalid_operation(format!(
                    "Method {method} with {arg_count} args was not found in {contract_hash}"
                ))
            })?;

        let key = Self::whitelist_fee_contract_key(&contract_hash, method_descriptor.offset);

        let context = engine.get_native_storage_context(&self.hash)?;

        // Check existence?
        if engine.get_storage_item(&context, key.suffix()).is_none() {
            return Err(Error::invalid_operation("Whitelist not found"));
        }

        engine.delete_storage_item(&context, key.suffix())?;

        engine
            .send_notification(
                self.hash,
                "WhitelistFeeChanged".to_string(),
                vec![
                    StackItem::ByteString(contract_hash.to_bytes().to_vec()),
                    StackItem::ByteString(method.as_bytes().to_vec()),
                    StackItem::Integer(BigInt::from(arg_count)),
                    StackItem::Null,
                ],
            )
            .map_err(Error::native_contract)?;

        Ok(Vec::new())
    }

    pub(crate) fn clean_whitelist(
        &self,
        engine: &mut ApplicationEngine,
        contract: &crate::smart_contract::ContractState,
    ) -> Result<usize> {
        let snapshot = engine.snapshot_cache();
        let prefix = StorageKey::new(self.id, Self::whitelist_fee_contract_prefix(&contract.hash));
        let mut count = 0usize;

        for (key, item) in snapshot.find(
            Some(&prefix),
            crate::persistence::seek_direction::SeekDirection::Forward,
        ) {
            snapshot.delete(&key);
            count += 1;

            let bytes = item.get_value();
            if bytes.is_empty() {
                continue;
            }

            let stack_item =
                crate::smart_contract::binary_serializer::BinarySerializer::deserialize(
                    &bytes,
                    &neo_vm::execution_engine_limits::ExecutionEngineLimits::default(),
                    None,
                )
                .map_err(|e| {
                    Error::native_contract(format!("Failed to deserialize whitelist info: {e}"))
                })?;

            let mut whitelist = WhitelistedContract::default();
            whitelist.from_stack_item(stack_item).map_err(|e| {
                Error::native_contract(format!("Failed to deserialize WhitelistedContract: {e}"))
            })?;

            engine
                .send_notification(
                    self.hash,
                    "WhitelistFeeChanged".to_string(),
                    vec![
                        StackItem::ByteString(contract.hash.to_bytes().to_vec()),
                        StackItem::ByteString(whitelist.method.as_bytes().to_vec()),
                        StackItem::Integer(BigInt::from(whitelist.arg_count)),
                        StackItem::Null,
                    ],
                )
                .map_err(Error::native_contract)?;
        }

        Ok(count)
    }
}
