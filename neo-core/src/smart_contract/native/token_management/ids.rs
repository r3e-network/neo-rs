use super::{TokenManagement, ID, PREFIX_NFT_UNIQUE_ID_SEED};
use neo_crypto::Crypto;
use crate::error::{CoreError, CoreResult};
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::native::NativeContract;
use crate::smart_contract::StorageKey;
use crate::UInt160;
use num_bigint::BigInt;

impl TokenManagement {
    pub(super) fn get_next_nft_unique_id(
        &self,
        engine: &mut ApplicationEngine,
    ) -> CoreResult<UInt160> {
        let context = engine.get_native_storage_context(&self.hash())?;
        let seed_key = StorageKey::create(ID, PREFIX_NFT_UNIQUE_ID_SEED)
            .suffix()
            .to_vec();

        let seed = match engine.get_storage_item(&context, &seed_key) {
            Some(data) => BigInt::from_signed_bytes_be(&data),
            None => BigInt::from(0),
        };

        let new_seed = seed + 1;
        let seed_bytes = Self::encode_bigint(&new_seed);
        engine.put_storage_item(&context, &seed_key, &seed_bytes)?;

        let block_hash = match engine.persisting_block() {
            Some(block) => block.hash(),
            None => {
                return Err(CoreError::native_contract(
                    "TokenManagement.getNextNFTUniqueId: no persisting block",
                ));
            }
        };

        let mut buffer = Vec::with_capacity(32 + seed_bytes.len());
        buffer.extend_from_slice(&block_hash.as_bytes());
        buffer.extend_from_slice(&seed_bytes);
        let hash = Crypto::hash160(&buffer);
        let unique_id = UInt160::from_bytes(&hash).unwrap_or_default();
        Ok(unique_id)
    }

    fn encode_bigint(value: &BigInt) -> Vec<u8> {
        let mut bytes = value.to_signed_bytes_le();
        if bytes.is_empty() {
            bytes.push(0);
        }
        bytes
    }

    pub fn get_asset_id(owner: &UInt160, name: &str) -> UInt160 {
        let name_bytes = name.as_bytes();
        let mut buffer = Vec::with_capacity(20 + name_bytes.len());
        buffer.extend_from_slice(&owner.as_bytes());
        buffer.extend_from_slice(name_bytes);
        let hash = Crypto::hash160(&buffer);
        UInt160::from_bytes(&hash).unwrap_or_default()
    }
}
