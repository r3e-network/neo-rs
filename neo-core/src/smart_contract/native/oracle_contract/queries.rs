use super::{OracleContract, PendingRequest, PREFIX_REQUEST};
use crate::error::CoreResult as Result;
use crate::persistence::{seek_direction::SeekDirection, DataCache};
use crate::smart_contract::native::oracle_request::OracleRequest;
use crate::smart_contract::storage_key::StorageKey;

impl OracleContract {
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

    pub fn get_requests(&self, snapshot: &DataCache) -> Result<Vec<(u64, OracleRequest)>> {
        let prefix = StorageKey::create(Self::ID, PREFIX_REQUEST);
        let mut results = Vec::new();
        for (key, item) in snapshot.find(Some(&prefix), SeekDirection::Forward) {
            let id = match Self::parse_request_id(&key) {
                Some(value) => value,
                None => continue,
            };
            let request = self.deserialize_request(&item.value_bytes())?;
            results.push((id, self.to_public_request(&request)));
        }
        Ok(results)
    }

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

    pub fn get_price(&self, snapshot: &DataCache) -> i64 {
        self.get_price_value(snapshot)
    }
}
