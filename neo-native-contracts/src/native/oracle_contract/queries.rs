//! Oracle read-only queries.
//!
//! Migrated from `neo-core/src/smart_contract/native/oracle_contract/queries.rs`.
//! Import adaptation: `crate::error::*` → `neo_smart_contract::error::*`;
//! `crate::persistence::*` → `neo_storage::persistence::*`;
//! `crate::smart_contract::native::oracle_request::*` →
//! `crate::native::oracle_request::*`; `crate::smart_contract::storage_key::StorageKey`
//! → `neo_storage::StorageKey`.

use super::{OracleContract, PREFIX_REQUEST, PendingRequest};
use crate::native::oracle_request::OracleRequest;
use neo_smart_contract::error::CoreResult as Result;
use neo_storage::StorageKey;
use neo_storage::persistence::{DataCache, seek_direction::SeekDirection};

impl OracleContract {
    /// Returns the oracle request stored under `id`, if any.
    pub fn get_request(&self, snapshot: &DataCache, id: u64) -> Result<Option<OracleRequest>> {
        Ok(self
            .read_request(snapshot, id)?
            .map(|pending| self.to_public_request(&pending)))
    }

    fn to_public_request(&self, request: &PendingRequest) -> OracleRequest {
        OracleRequest::new(
            request.original_tx_id,
            request.gas_for_response,
            request.url.clone(),
            request.filter.clone(),
            request.callback_contract,
            request.callback_method.clone(),
            request.user_data.clone(),
        )
    }

    /// Returns every stored oracle request together with its id.
    pub fn get_requests(&self, snapshot: &DataCache) -> Result<Vec<(u64, OracleRequest)>> {
        let prefix = StorageKey::create(Self::ID, PREFIX_REQUEST);
        let mut results = Vec::new();
        for (key, item) in snapshot.find(Some(&prefix), SeekDirection::Forward) {
            let Some(id) = Self::parse_request_id(&key) else {
                continue;
            };
            let request = self.deserialize_request(&item.value_bytes())?;
            results.push((id, self.to_public_request(&request)));
        }
        Ok(results)
    }

    /// Returns all oracle requests recorded for the given URL.
    pub fn get_requests_by_url(
        &self,
        snapshot: &DataCache,
        url: &str,
    ) -> Result<Vec<(u64, OracleRequest)>> {
        let hash = self.compute_url_hash(url);
        let mut results = Vec::new();
        for id in self.read_id_list(snapshot, &hash)? {
            if let Some(request) = self.read_request(snapshot, id)? {
                results.push((id, self.to_public_request(&request)));
            }
        }
        Ok(results)
    }

    /// Returns the current oracle response price in GAS (C# `getPrice`).
    pub fn get_price(&self, snapshot: &DataCache) -> i64 {
        self.get_price_value(snapshot)
    }
}
