use super::{
    TokenManagement, ID, NFT_INDEX_KEY_SIZE, PREFIX_NFT_ASSET_ID_UNIQUE_ID_INDEX,
    PREFIX_NFT_OWNER_UNIQUE_ID_INDEX,
};
use crate::error::CoreResult;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::storage_context::StorageContext;
use crate::smart_contract::StorageKey;
use crate::UInt160;

impl TokenManagement {
    fn update_nft_index(
        &self,
        context: &StorageContext,
        engine: &mut ApplicationEngine,
        prefix: u8,
        address: &UInt160,
        nft_id: &UInt160,
        is_add: bool,
    ) -> CoreResult<()> {
        let mut index_key = Vec::with_capacity(NFT_INDEX_KEY_SIZE);
        index_key.push(prefix);
        index_key.extend_from_slice(&address.as_bytes());
        index_key.extend_from_slice(&nft_id.as_bytes());
        let index_key = StorageKey::new(ID, index_key);
        if is_add {
            engine.put_storage_item(context, index_key.suffix(), &[0])?;
        } else {
            engine.delete_storage_item(context, index_key.suffix())?;
        }
        Ok(())
    }

    pub(super) fn add_nft_to_asset_index(
        &self,
        context: &StorageContext,
        engine: &mut ApplicationEngine,
        asset_id: &UInt160,
        nft_id: &UInt160,
    ) -> CoreResult<()> {
        self.update_nft_index(
            context,
            engine,
            PREFIX_NFT_ASSET_ID_UNIQUE_ID_INDEX,
            asset_id,
            nft_id,
            true,
        )
    }

    pub(super) fn remove_nft_from_asset_index(
        &self,
        context: &StorageContext,
        engine: &mut ApplicationEngine,
        asset_id: &UInt160,
        nft_id: &UInt160,
    ) -> CoreResult<()> {
        self.update_nft_index(
            context,
            engine,
            PREFIX_NFT_ASSET_ID_UNIQUE_ID_INDEX,
            asset_id,
            nft_id,
            false,
        )
    }

    pub(super) fn add_nft_to_owner_index(
        &self,
        context: &StorageContext,
        engine: &mut ApplicationEngine,
        owner: &UInt160,
        nft_id: &UInt160,
    ) -> CoreResult<()> {
        self.update_nft_index(
            context,
            engine,
            PREFIX_NFT_OWNER_UNIQUE_ID_INDEX,
            owner,
            nft_id,
            true,
        )
    }

    pub(super) fn remove_nft_from_owner_index(
        &self,
        context: &StorageContext,
        engine: &mut ApplicationEngine,
        owner: &UInt160,
        nft_id: &UInt160,
    ) -> CoreResult<()> {
        self.update_nft_index(
            context,
            engine,
            PREFIX_NFT_OWNER_UNIQUE_ID_INDEX,
            owner,
            nft_id,
            false,
        )
    }
}
