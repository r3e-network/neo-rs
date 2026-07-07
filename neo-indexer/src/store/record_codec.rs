//! JSON encoding and decoding for indexer store records.

use serde::{Serialize, de::DeserializeOwned};

use crate::error::{IndexerError, IndexerResult};

pub(super) fn encode_record<T>(key: Vec<u8>, value: &T) -> IndexerResult<(Vec<u8>, Vec<u8>)>
where
    T: Serialize,
{
    let bytes = serde_json::to_vec(value).map_err(|source| IndexerError::StoreRecordEncode {
        key: key.clone(),
        source,
    })?;
    Ok((key, bytes))
}

pub(super) fn decode_record<T>(key: Vec<u8>, value: Vec<u8>) -> IndexerResult<T>
where
    T: DeserializeOwned,
{
    serde_json::from_slice(&value).map_err(|source| IndexerError::StoreRecordDecode { key, source })
}
