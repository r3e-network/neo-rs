use super::{
    OracleContract, PendingRequest, DEFAULT_PRICE, MAX_PENDING_PER_URL, PREFIX_ID_LIST,
    PREFIX_PRICE, PREFIX_REQUEST, PREFIX_REQUEST_ID,
};
use neo_crypto::Crypto;
use crate::error::{CoreError as Error, CoreResult as Result};
use crate::persistence::{read_only_store::ReadOnlyStoreGeneric, DataCache};
use crate::smart_contract::binary_serializer::BinarySerializer;
use crate::smart_contract::native::oracle_request::OracleRequest;
use crate::smart_contract::storage_key::StorageKey;
use crate::smart_contract::StorageItem;
use crate::neo_vm::{StackItem};
use crate::{UInt160, UInt256};
use neo_vm_rs::ExecutionEngineLimits;
use neo_vm_rs::StackValue;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

impl OracleContract {
    pub(super) fn get_price_value(&self, snapshot: &DataCache) -> i64 {
        // C# parity: stored as variable-width signed-LE BigInteger bytes (StorageItem.Value),
        // not a fixed 8-byte i64. Empty bytes => 0; parse via BigInt::from_signed_bytes_le.
        let key = self.price_key();
        snapshot
            .try_get(&key)
            .and_then(|item| {
                let bytes = item.value_bytes();
                if bytes.is_empty() {
                    Some(0)
                } else {
                    BigInt::from_signed_bytes_le(&bytes).to_i64()
                }
            })
            .unwrap_or(DEFAULT_PRICE)
    }

    pub(super) fn next_request_id(&self, snapshot: &DataCache) -> Result<u64> {
        // C# parity: request ID counter is stored as a BigInt via StorageItem,
        // encoded as signed little-endian bytes (Integer format).
        let key = self.request_id_key();
        let current = snapshot
            .try_get(&key)
            .and_then(|item| {
                let bytes = item.value_bytes();
                if bytes.is_empty() {
                    Some(0u64)
                } else {
                    BigInt::from_signed_bytes_le(&bytes).to_u64()
                }
            })
            .unwrap_or(0);
        let next = current
            .checked_add(1)
            .ok_or_else(|| Error::runtime_error("Next request id overflowed"))?;
        let next_bytes = BigInt::from(next).to_signed_bytes_le();
        self.put_item(snapshot, key, StorageItem::from_bytes(next_bytes));
        Ok(current)
    }

    pub(super) fn append_request_id(
        &self,
        snapshot: &DataCache,
        hash: &[u8; 20],
        id: u64,
    ) -> Result<()> {
        let mut list = self.read_id_list(snapshot, hash)?;
        if list.len() >= MAX_PENDING_PER_URL {
            return Err(Error::invalid_operation(
                "There are too many pending responses for this url".to_string(),
            ));
        }
        list.push(id);
        self.write_id_list(snapshot, hash, &list)?;
        Ok(())
    }

    pub(super) fn remove_request_id(
        &self,
        snapshot: &DataCache,
        hash: &[u8; 20],
        id: u64,
    ) -> Result<()> {
        let mut list = self.read_id_list(snapshot, hash)?;
        if let Some(pos) = list.iter().position(|existing| *existing == id) {
            list.remove(pos);
        }
        if list.is_empty() {
            snapshot.delete(&self.id_list_key(hash));
        } else {
            self.write_id_list(snapshot, hash, &list)?;
        }
        Ok(())
    }

    pub(super) fn read_request(
        &self,
        snapshot: &DataCache,
        id: u64,
    ) -> Result<Option<PendingRequest>> {
        let key = self.request_storage_key(id);
        snapshot
            .try_get(&key)
            .map(|item| self.deserialize_request(&item.value_bytes()))
            .transpose()
    }

    pub(super) fn write_request(
        &self,
        snapshot: &DataCache,
        request: &PendingRequest,
    ) -> Result<()> {
        let key = self.request_storage_key(request.id);
        let bytes = self.serialize_request(request)?;
        self.put_item(snapshot, key, StorageItem::from_bytes(bytes));
        Ok(())
    }

    pub(super) fn delete_request(&self, snapshot: &DataCache, id: u64) {
        let key = self.request_storage_key(id);
        snapshot.delete(&key);
    }

    fn serialize_request(&self, request: &PendingRequest) -> Result<Vec<u8>> {
        // C# parity: OracleRequest stored as Array(7) via BinarySerializer.
        // Items: [OriginalTxid(32B), GasForResponse(int), Url(str), Filter(str|null),
        //         CallbackContract(20B), CallbackMethod(str), UserData(bytes)]
        let request_value = OracleRequest::new(
            request.original_tx_id,
            request.gas_for_response,
            request.url.clone(),
            request.filter.clone(),
            request.callback_contract,
            request.callback_method.clone(),
            request.user_data.clone(),
        )
        .to_stack_value();
        BinarySerializer::serialize_stack_value(&request_value, &ExecutionEngineLimits::default())
            .map_err(Error::serialization)
    }

    pub(super) fn deserialize_request(&self, bytes: &[u8]) -> Result<PendingRequest> {
        let stack_value =
            BinarySerializer::deserialize_stack_value(bytes).map_err(Error::serialization)?;
        let items = match stack_value {
            StackValue::Array(items) => items,
            _ => return Err(Error::serialization("OracleRequest must be an Array")),
        };
        if items.len() < 7 {
            return Err(Error::serialization(format!(
                "OracleRequest expected 7 items, got {}",
                items.len()
            )));
        }
        let original_tx_id = {
            let b = Self::request_bytes(&items[0])?;
            UInt256::from_bytes(&b).map_err(|e| Error::serialization(e.to_string()))?
        };
        let gas_for_response = items[1]
            .to_i128()
            .and_then(|integer| i64::try_from(integer).ok())
            .ok_or_else(|| Error::serialization("gas_for_response overflow".to_string()))?;
        let url = {
            let b = Self::request_bytes(&items[2])?;
            String::from_utf8(b).map_err(|e| Error::serialization(e.to_string()))?
        };
        let filter = if matches!(items[3], StackValue::Null) {
            None
        } else {
            let b = Self::request_bytes(&items[3])?;
            Some(String::from_utf8(b).map_err(|e| Error::serialization(e.to_string()))?)
        };
        let callback_contract = {
            let b = Self::request_bytes(&items[4])?;
            UInt160::from_bytes(&b).map_err(|e| Error::serialization(e.to_string()))?
        };
        let callback_method = {
            let b = Self::request_bytes(&items[5])?;
            String::from_utf8(b).map_err(|e| Error::serialization(e.to_string()))?
        };
        let user_data = Self::request_bytes(&items[6])?;

        Ok(PendingRequest {
            id: 0, // id is encoded in the storage key, not the value
            original_tx_id,
            gas_for_response,
            url,
            filter,
            callback_contract,
            callback_method,
            user_data,
        })
    }

    fn request_bytes(value: &StackValue) -> Result<Vec<u8>> {
        value
            .to_byte_string_bytes()
            .ok_or_else(|| Error::serialization("Cannot convert to ByteArray"))
    }

    pub(super) fn read_id_list(&self, snapshot: &DataCache, hash: &[u8; 20]) -> Result<Vec<u64>> {
        let key = self.id_list_key(hash);
        if let Some(item) = snapshot.try_get(&key) {
            self.deserialize_id_list(&item.value_bytes())
        } else {
            Ok(Vec::new())
        }
    }

    fn write_id_list(&self, snapshot: &DataCache, hash: &[u8; 20], list: &[u64]) -> Result<()> {
        let key = self.id_list_key(hash);
        let bytes = self.serialize_id_list(list)?;
        self.put_item(snapshot, key, StorageItem::from_bytes(bytes));
        Ok(())
    }

    pub(super) fn price_key(&self) -> StorageKey {
        StorageKey::create(Self::ID, PREFIX_PRICE)
    }

    pub(super) fn request_id_key(&self) -> StorageKey {
        StorageKey::create(Self::ID, PREFIX_REQUEST_ID)
    }

    fn request_storage_key(&self, id: u64) -> StorageKey {
        StorageKey::create_with_bytes(Self::ID, PREFIX_REQUEST, &id.to_be_bytes())
    }

    fn id_list_key(&self, hash: &[u8; 20]) -> StorageKey {
        StorageKey::create_with_bytes(Self::ID, PREFIX_ID_LIST, hash)
    }

    pub(super) fn parse_request_id(key: &StorageKey) -> Option<u64> {
        let suffix = key.suffix();
        if suffix.len() != 1 + std::mem::size_of::<u64>() {
            return None;
        }
        if suffix[0] != PREFIX_REQUEST {
            return None;
        }
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&suffix[1..]);
        Some(u64::from_be_bytes(buf))
    }

    pub(super) fn put_item(&self, snapshot: &DataCache, key: StorageKey, item: StorageItem) {
        if snapshot.get(&key).is_some() {
            snapshot.update(key, item);
        } else {
            snapshot.add(key, item);
        }
    }

    pub(super) fn compute_url_hash(&self, url: &str) -> [u8; 20] {
        Crypto::hash160(url.as_bytes())
    }

    fn serialize_id_list(&self, list: &[u64]) -> Result<Vec<u8>> {
        let items = list
            .iter()
            .map(|id| {
                let id_i64 = i64::try_from(*id).expect("Oracle request id exceeds i64::MAX");
                StackItem::from_int(id_i64)
            })
            .collect::<Vec<_>>();
        BinarySerializer::serialize(
            &StackItem::from_array(items),
            &ExecutionEngineLimits::default(),
        )
        .map_err(|err| Error::serialization(err.to_string()))
    }

    fn deserialize_id_list(&self, bytes: &[u8]) -> Result<Vec<u64>> {
        let item = BinarySerializer::deserialize(bytes, &ExecutionEngineLimits::default(), None)
            .map_err(|err| Error::serialization(err.to_string()))?;

        let StackItem::Array(array) = item else {
            return Err(Error::invalid_data(
                "Corrupted oracle URL id list entry".to_string(),
            ));
        };

        let mut ids = Vec::with_capacity(array.len());
        for element in &array {
            let value = element
                .as_int()
                .map_err(|_| Error::invalid_data("Invalid URL id entry"))?;
            let id = value
                .to_u64()
                .ok_or_else(|| Error::invalid_data("URL id entry overflow"))?;
            ids.push(id);
        }

        Ok(ids)
    }
}
