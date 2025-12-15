//
// helpers.rs - Helper methods for PolicyContract
//

use super::*;

impl PolicyContract {
    pub(super) fn encode_bigint(value: &BigInt) -> Vec<u8> {
        if value.is_zero() {
            return Vec::new();
        }

        let mut bytes = value.to_signed_bytes_le();
        let negative = value.sign() == Sign::Minus;
        if let Some(last) = bytes.last() {
            let sign_bit_set = last & 0x80 != 0;
            if !negative && sign_bit_set {
                bytes.push(0);
            } else if negative && !sign_bit_set {
                bytes.push(0xFF);
            }
        }
        bytes
    }

    pub(super) fn encode_i64(value: i64) -> Vec<u8> {
        Self::encode_bigint(&BigInt::from(value))
    }

    pub(super) fn encode_u32(value: u32) -> Vec<u8> {
        Self::encode_bigint(&BigInt::from(value))
    }

    pub(super) fn parse_u8_argument(arg: &[u8], name: &str) -> Result<u8> {
        BigInt::from_signed_bytes_le(arg)
            .to_u8()
            .ok_or_else(|| Error::native_contract(format!("{name} is out of range")))
    }

    pub(super) fn parse_u32_argument(arg: &[u8], name: &str) -> Result<u32> {
        BigInt::from_signed_bytes_le(arg)
            .to_u32()
            .ok_or_else(|| Error::native_contract(format!("{name} is out of range")))
    }

    pub(super) fn parse_i64_argument(arg: &[u8], name: &str) -> Result<i64> {
        BigInt::from_signed_bytes_le(arg)
            .to_i64()
            .ok_or_else(|| Error::native_contract(format!("{name} is out of range")))
    }

    pub(super) fn fee_per_byte_key() -> StorageKey {
        StorageKey::create(Self::ID, Self::PREFIX_FEE_PER_BYTE)
    }

    pub(super) fn exec_fee_factor_key() -> StorageKey {
        StorageKey::create(Self::ID, Self::PREFIX_EXEC_FEE_FACTOR)
    }

    pub(super) fn storage_price_key() -> StorageKey {
        StorageKey::create(Self::ID, Self::PREFIX_STORAGE_PRICE)
    }

    pub(super) fn milliseconds_per_block_key() -> StorageKey {
        StorageKey::create(Self::ID, Self::PREFIX_MILLISECONDS_PER_BLOCK)
    }

    pub(super) fn max_valid_until_block_increment_key() -> StorageKey {
        StorageKey::create(Self::ID, Self::PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT)
    }

    pub(super) fn max_traceable_blocks_key() -> StorageKey {
        StorageKey::create(Self::ID, Self::PREFIX_MAX_TRACEABLE_BLOCKS)
    }

    pub(super) fn blocked_account_key(account: &UInt160) -> StorageKey {
        StorageKey::create_with_uint160(Self::ID, Self::PREFIX_BLOCKED_ACCOUNT, account)
    }

    pub(super) fn blocked_account_suffix(account: &UInt160) -> Vec<u8> {
        Self::blocked_account_key(account).suffix().to_vec()
    }

    pub(super) fn attribute_fee_key(attribute_type: u8) -> StorageKey {
        StorageKey::create_with_byte(Self::ID, Self::PREFIX_ATTRIBUTE_FEE, attribute_type)
    }

    pub(super) fn attribute_fee_suffix(attribute_type: u8) -> Vec<u8> {
        Self::attribute_fee_key(attribute_type).suffix().to_vec()
    }

    pub(super) fn assert_committee(engine: &ApplicationEngine) -> Result<()> {
        if !engine.check_committee_witness()? {
            return Err(Error::invalid_operation(
                "Committee authorization required".to_string(),
            ));
        }
        Ok(())
    }

    pub(super) fn validate_attribute_type(
        attribute_type: u8,
        allow_notary_assisted: bool,
    ) -> Result<TransactionAttributeType> {
        let Some(attr) = TransactionAttributeType::from_byte(attribute_type) else {
            return Err(Error::invalid_operation(format!(
                "Attribute type {attribute_type} is not supported."
            )));
        };

        if !allow_notary_assisted && attr == TransactionAttributeType::NotaryAssisted {
            return Err(Error::invalid_operation(format!(
                "Attribute type {attribute_type} is not supported."
            )));
        }

        Ok(attr)
    }

    pub(super) fn read_u32_setting(
        &self,
        engine: &mut ApplicationEngine,
        key_suffix: &[u8],
        default: u32,
    ) -> Result<u32> {
        let context = engine.get_native_storage_context(&self.hash)?;
        let Some(bytes) = engine.get_storage_item(&context, key_suffix) else {
            return Ok(default);
        };
        BigInt::from_signed_bytes_le(&bytes)
            .to_u32()
            .ok_or_else(|| {
                Error::native_contract("Stored policy value exceeds u32 capacity".to_string())
            })
    }

    pub(super) fn read_i64_setting(
        &self,
        engine: &mut ApplicationEngine,
        key_suffix: &[u8],
        default: i64,
    ) -> Result<i64> {
        let context = engine.get_native_storage_context(&self.hash)?;
        let Some(bytes) = engine.get_storage_item(&context, key_suffix) else {
            return Ok(default);
        };
        BigInt::from_signed_bytes_le(&bytes)
            .to_i64()
            .ok_or_else(|| {
                Error::native_contract("Stored policy value exceeds i64 capacity".to_string())
            })
    }
}
