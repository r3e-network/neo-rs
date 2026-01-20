//
// getters.rs - Getter methods for PolicyContract
//

use super::*;

impl PolicyContract {
    pub(super) fn get_fee_per_byte(&self, engine: &mut ApplicationEngine) -> Result<Vec<u8>> {
        let value = self.read_i64_setting(
            engine,
            Self::fee_per_byte_key().suffix(),
            Self::DEFAULT_FEE_PER_BYTE as i64,
        )?;
        Ok(Self::encode_i64(value))
    }

    pub(super) fn get_exec_fee_factor(&self, engine: &mut ApplicationEngine) -> Result<Vec<u8>> {
        let value = self.read_u32_setting(
            engine,
            Self::exec_fee_factor_key().suffix(),
            Self::DEFAULT_EXEC_FEE_FACTOR,
        )?;
        
        if engine.is_hardfork_enabled(Hardfork::HfFaun) {
             // In Faun and later, value is stored in picoGAS, need to scale down for legacy accessor
             // FeeFactor is 10000.
             // value is u32, so division is safe.
             let scaled = value / crate::smart_contract::application_engine::FEE_FACTOR as u32;
             Ok(Self::encode_u32(scaled))
        } else {
             Ok(Self::encode_u32(value))
        }
    }

    pub(super) fn get_exec_pico_fee_factor(&self, engine: &mut ApplicationEngine) -> Result<Vec<u8>> {
        let value = self.read_u32_setting(
            engine,
            Self::exec_fee_factor_key().suffix(),
            Self::DEFAULT_EXEC_FEE_FACTOR,
        )?;
        // Return raw value (picoGAS)
        // Return BigInteger bytes for C# compatibility
        Ok(Self::encode_u32(value))
    }

    pub(super) fn get_storage_price(&self, engine: &mut ApplicationEngine) -> Result<Vec<u8>> {
        let value = self.read_u32_setting(
            engine,
            Self::storage_price_key().suffix(),
            Self::DEFAULT_STORAGE_PRICE,
        )?;
        Ok(Self::encode_u32(value))
    }

    pub(super) fn get_milliseconds_per_block(
        &self,
        engine: &mut ApplicationEngine,
    ) -> Result<Vec<u8>> {
        let default = engine.protocol_settings().milliseconds_per_block;
        let value =
            self.read_u32_setting(engine, Self::milliseconds_per_block_key().suffix(), default)?;
        Ok(Self::encode_u32(value))
    }

    pub(super) fn get_max_valid_until_block_increment(
        &self,
        engine: &mut ApplicationEngine,
    ) -> Result<Vec<u8>> {
        let default = engine.protocol_settings().max_valid_until_block_increment;
        let value = self.read_u32_setting(
            engine,
            Self::max_valid_until_block_increment_key().suffix(),
            default,
        )?;
        Ok(Self::encode_u32(value))
    }

    pub(super) fn get_max_traceable_blocks(
        &self,
        engine: &mut ApplicationEngine,
    ) -> Result<Vec<u8>> {
        let default = engine.protocol_settings().max_traceable_blocks;
        let value =
            self.read_u32_setting(engine, Self::max_traceable_blocks_key().suffix(), default)?;
        Ok(Self::encode_u32(value))
    }

    pub(super) fn get_attribute_fee(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.len() != 1 {
            return Err(Error::native_contract(
                "getAttributeFee requires attributeType argument".to_string(),
            ));
        }

        let attribute_type = Self::parse_u8_argument(&args[0], "attributeType")?;
        let allow_notary_assisted = engine.is_hardfork_enabled(Hardfork::HfEchidna);
        Self::validate_attribute_type(attribute_type, allow_notary_assisted)?;

        let value = self.read_u32_setting(
            engine,
            &Self::attribute_fee_suffix(attribute_type),
            Self::DEFAULT_ATTRIBUTE_FEE,
        )?;
        Ok(Self::encode_u32(value))
    }

    pub(super) fn get_whitelist_fee_contracts(&self, engine: &mut ApplicationEngine) -> Result<Vec<u8>> {
        let context = engine.get_native_storage_context(&self.hash)?;
        
        let options = FindOptions::RemovePrefix | FindOptions::ValuesOnly | FindOptions::DeserializeValues;
        
        let iterator = engine
            .find_storage_entries(
                &context,
                &[Self::PREFIX_WHITELISTED_FEE_CONTRACTS],
                options,
            )
            .map_err(Error::native_contract)?;

        let iterator_id = engine
            .store_storage_iterator(iterator)
            .map_err(Error::native_contract)?;
        Ok(iterator_id.to_le_bytes().to_vec())
    }
}
