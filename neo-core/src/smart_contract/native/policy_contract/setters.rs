//
// setters.rs - Setter methods for PolicyContract
//

use super::*;

impl PolicyContract {
    pub(super) fn set_fee_per_byte(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.len() != 1 {
            return Err(Error::native_contract(
                "setFeePerByte requires value argument".to_string(),
            ));
        }

        let value = Self::parse_i64_argument(&args[0], "value")?;
        if !(0..=100_000_000).contains(&value) {
            return Err(Error::invalid_operation(format!(
                "FeePerByte must be between [0, 100000000], got {value}"
            )));
        }

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
        if value == 0 || value > Self::MAX_EXEC_FEE_FACTOR {
            return Err(Error::invalid_operation(format!(
                "ExecFeeFactor must be between [1, {}], got {value}",
                Self::MAX_EXEC_FEE_FACTOR
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
}
