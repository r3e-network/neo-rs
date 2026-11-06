//! TokenManagement identifier derivation (NFT unique ids and asset ids).
//!
//! Migrated from `neo-core/src/smart_contract/native/token_management/ids.rs`.
//!
//! Import adaptation:
//! - `crate::UInt160` -> `neo_primitives::UInt160`
//! - `crate::cryptography::Crypto` -> `neo_crypto::Crypto`
//! - `crate::error::{CoreError, CoreResult}`
//!   -> `neo_smart_contract::error::{CoreError, CoreResult}`
//! - `crate::smart_contract::StorageKey` -> `neo_storage::StorageKey`
//! - `crate::smart_contract::application_engine::ApplicationEngine`
//!   -> `neo_smart_contract::application_engine::ApplicationEngine`
//! - `crate::smart_contract::native::NativeContract` -> `crate::native::NativeContract`

use super::{ID, PREFIX_NFT_UNIQUE_ID_SEED, TokenManagement};
use crate::native::NativeContract;
use neo_crypto::Crypto;
use neo_primitives::UInt160;
use neo_smart_contract::application_engine::ApplicationEngine;
use neo_smart_contract::error::{CoreError, CoreResult};
use neo_storage::StorageKey;
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
            Some(block) => block.clone().hash(),
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

    /// Derives a deterministic NFT asset identifier as `HASH160(owner || name)`.
    pub fn get_asset_id(owner: &UInt160, name: &str) -> UInt160 {
        let name_bytes = name.as_bytes();
        let mut buffer = Vec::with_capacity(20 + name_bytes.len());
        buffer.extend_from_slice(&owner.as_bytes());
        buffer.extend_from_slice(name_bytes);
        let hash = Crypto::hash160(&buffer);
        UInt160::from_bytes(&hash).unwrap_or_default()
    }
}
