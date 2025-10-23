// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Extensions.LedgerContractExtensions`.

use crate::rest_server::models::blockchain::account_details::AccountDetails;
use neo_core::persistence::seek_direction::SeekDirection;
use neo_core::persistence::{IReadOnlyStoreGeneric, StoreCache};
use neo_core::smart_contract::native::fungible_token::PREFIX_ACCOUNT as TOKEN_ACCOUNT_PREFIX;
use neo_core::smart_contract::storage_key::StorageKey;
use neo_core::smart_contract::StorageItem;
use neo_core::wallets::helper::Helper as WalletHelper;
use neo_core::UInt160;

/// Helper routines for enumerating native token accounts.
pub struct LedgerContractExtensions;

impl LedgerContractExtensions {
    /// Lists all accounts for the supplied token identifier, mirroring the C# helpers.
    pub fn list_accounts(
        snapshot: &StoreCache,
        token_id: i32,
        decimals: i32,
        address_version: u8,
    ) -> Result<Vec<AccountDetails>, String> {
        let prefix = StorageKey::create(token_id, TOKEN_ACCOUNT_PREFIX);
        let mut accounts = Vec::new();

        for (key, value) in snapshot.find(Some(&prefix), SeekDirection::Forward) {
            if let Some(account) =
                Self::account_from_storage(&key, &value, decimals, address_version)?
            {
                accounts.push(account);
            }
        }

        Ok(accounts)
    }

    fn account_from_storage(
        key: &StorageKey,
        value: &StorageItem,
        decimals: i32,
        address_version: u8,
    ) -> Result<Option<AccountDetails>, String> {
        let suffix = key.suffix();
        if suffix.is_empty() || suffix[0] != TOKEN_ACCOUNT_PREFIX {
            return Ok(None);
        }

        let hash_bytes = &suffix[1..];
        if hash_bytes.len() != UInt160::LENGTH {
            return Ok(None);
        }

        let script_hash =
            UInt160::from_bytes(hash_bytes).map_err(|err| format!("Invalid script hash: {err}"))?;
        let balance = value.to_bigint();
        let address = WalletHelper::to_address(&script_hash, address_version);

        Ok(Some(AccountDetails::new(
            script_hash,
            address,
            balance,
            decimals,
        )))
    }
}
