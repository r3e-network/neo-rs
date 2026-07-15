//! Oracle request storage keys, codecs, and query helpers.

use super::request::OracleIdList;
use super::{
    OracleContract, OracleRequest, PREFIX_ID_LIST, PREFIX_PRICE, PREFIX_REQUEST, PREFIX_REQUEST_ID,
};
use neo_crypto::Crypto;
use neo_error::{CoreError, CoreResult};
use neo_execution::ApplicationEngine;
use neo_payloads::{OracleResponse, Transaction, TransactionAttribute};
use neo_primitives::UInt256;
use neo_storage::persistence::{DataCache, SeekDirection};
use neo_storage::{StorageItem, StorageKey};
use num_bigint::BigInt;
use num_traits::ToPrimitive;

impl OracleContract {
    /// Look up a single oracle request by its id (C# `GetRequest`).
    pub fn get_request<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        id: u64,
    ) -> CoreResult<Option<OracleRequest>> {
        self.read_request(snapshot, id)
    }

    /// Enumerate all pending oracle requests (C# `GetRequests`): a forward
    /// scan over `Prefix_Request`, the id read big-endian from the key
    /// suffix. Records that fail to decode are skipped (the signature
    /// predates fallibility and only this contract writes the records).
    pub fn get_requests<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> Vec<(u64, OracleRequest)> {
        let prefix = Self::request_prefix_key();
        let mut out = Vec::new();
        for (key, item) in snapshot.find(Some(&prefix), SeekDirection::Forward) {
            let key_bytes = key.key();
            if key_bytes.len() != 9 {
                continue;
            }
            let mut id_bytes = [0u8; 8];
            id_bytes.copy_from_slice(&key_bytes[1..9]);
            let id = u64::from_be_bytes(id_bytes);
            if let Ok(request) = Self::decode_oracle_request(&item.value_bytes()) {
                out.push((id, request));
            }
        }
        out
    }

    /// Enumerate all pending oracle requests matching a URL (C#
    /// `GetRequestsByUrl`): resolve the per-url id-list, then each record.
    /// A listed id without a record is an error (C# `snapshot[key]` throws).
    pub fn get_requests_by_url<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        url: &str,
    ) -> CoreResult<Vec<(u64, OracleRequest)>> {
        let Some(item) = snapshot.get(&Self::id_list_key(url)) else {
            return Ok(Vec::new());
        };
        let ids = Self::decode_id_list(&item.value_bytes())?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let request = self.read_request(snapshot, id)?.ok_or_else(|| {
                CoreError::invalid_data(format!("Oracle request {id} missing for listed url"))
            })?;
            out.push((id, request));
        }
        Ok(out)
    }

    /// C# `SetPrice` storage effect: overwrite `Prefix_Price` as a `BigInteger`
    /// (`GetAndChange(...).Set(price)`). Genesis initialization creates this
    /// row; later writes fault if it is missing.
    pub(in crate::oracle_contract) fn put_price<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        price: i64,
    ) -> CoreResult<()> {
        crate::support::settings::put_required_i64_setting_key(
            snapshot,
            Self::price_key(),
            "OracleContract price",
            price,
        )
    }

    /// Reads `Prefix_Price`. C# genesis initialization seeds this record and
    /// later code reads it with direct storage access, so absence is a fault.
    pub(in crate::oracle_contract) fn read_price<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> CoreResult<i64> {
        crate::support::settings::read_required_i64_setting_key(
            snapshot,
            Self::price_key(),
            "OracleContract price",
        )
    }

    /// The oracle request price setting key `(Oracle.ID, [Prefix_Price])`.
    pub(in crate::oracle_contract) fn price_key() -> StorageKey {
        crate::keys::prefixed_key(Self::ID, PREFIX_PRICE, &[])
    }

    /// The request-id counter key `(Oracle.ID, [Prefix_RequestId])`.
    pub(in crate::oracle_contract) fn request_id_key() -> StorageKey {
        crate::keys::prefixed_key(Self::ID, PREFIX_REQUEST_ID, &[])
    }

    /// The request record prefix key `(Oracle.ID, [Prefix_Request])`.
    pub(in crate::oracle_contract) fn request_prefix_key() -> StorageKey {
        crate::keys::prefixed_key(Self::ID, PREFIX_REQUEST, &[])
    }

    /// The request record key `(Oracle.ID, [Prefix_Request, id_be8])` — C#
    /// `CreateStorageKey(Prefix_Request, ulong)` appends the id big-endian.
    pub(in crate::oracle_contract) fn request_key(id: u64) -> StorageKey {
        crate::keys::prefixed_u64_be_key(Self::ID, PREFIX_REQUEST, id)
    }

    /// The per-url id-list key `(Oracle.ID, [Prefix_IdList] ++ Hash160(url))` —
    /// C# `GetUrlHash` is `Crypto.Hash160(url.ToStrictUtf8Bytes())`.
    pub(in crate::oracle_contract) fn id_list_key(url: &str) -> StorageKey {
        crate::keys::prefixed_key(Self::ID, PREFIX_ID_LIST, &Crypto::hash160(url.as_bytes()))
    }

    /// Reads the request-id counter (`Prefix_RequestId`). The C# genesis
    /// `InitializeAsync` seeds it with `BigInteger.Zero`; later `Request`
    /// uses `GetAndChange(... )!`, so absence is a fault.
    pub(in crate::oracle_contract) fn read_request_id<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> CoreResult<u64> {
        let item = snapshot.get(&Self::request_id_key()).ok_or_else(|| {
            CoreError::invalid_data("OracleContract request-id counter is missing")
        })?;
        BigInt::from_signed_bytes_le(&item.value_bytes())
            .to_u64()
            .ok_or_else(|| CoreError::invalid_data("Oracle request-id counter out of range"))
    }

    /// Writes the request-id counter (C# `itemId.Add(1)` after taking the id).
    pub(in crate::oracle_contract) fn write_request_id<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        value: &BigInt,
    ) {
        snapshot.update(
            Self::request_id_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(value)),
        );
    }

    /// Encodes an `OracleRequest` as the C# `IInteroperable` layout: the
    /// BinarySerialized `Array[OriginalTxid, GasForResponse, Url, Filter|Null,
    /// CallbackContract, CallbackMethod, UserData]` (`OracleRequest.ToStackItem`).
    pub(in crate::oracle_contract) fn encode_oracle_request(
        request: &OracleRequest,
    ) -> CoreResult<Vec<u8>> {
        crate::support::codec::encode_storage_struct(request, "OracleRequest")
    }

    /// Decodes a stored `OracleRequest` record (C# `OracleRequest.FromStackItem`).
    pub(in crate::oracle_contract) fn decode_oracle_request(
        bytes: &[u8],
    ) -> CoreResult<OracleRequest> {
        let item = crate::support::codec::decode_stack_item(bytes, "OracleRequest")?;
        OracleRequest::from_stack_item(&item)
    }

    /// Encodes the per-url id-list (C# `IdList : InteroperableList<ulong>`): the
    /// BinarySerialized `Array` of `Integer` ids.
    pub(in crate::oracle_contract) fn encode_id_list(ids: &[u64]) -> CoreResult<Vec<u8>> {
        crate::support::codec::encode_storage_struct(
            &OracleIdList::new(ids.to_vec()),
            "Oracle IdList",
        )
    }

    /// Decodes the per-url id-list (C# `IdList.FromStackItem`, `(ulong)item.GetInteger()`).
    pub(in crate::oracle_contract) fn decode_id_list(bytes: &[u8]) -> CoreResult<Vec<u64>> {
        let item = crate::support::codec::decode_stack_item(bytes, "Oracle IdList")?;
        Ok(OracleIdList::from_stack_item(&item)?.into_ids())
    }

    /// Reads a pending request record (C# `GetRequest`): `None` when no request
    /// with the given id exists.
    pub(in crate::oracle_contract) fn read_request<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        id: u64,
    ) -> CoreResult<Option<OracleRequest>> {
        match snapshot.get(&Self::request_key(id)) {
            None => Ok(None),
            Some(item) => Self::decode_oracle_request(&item.value_bytes()).map(Some),
        }
    }

    /// Adds a pending request record with C# `SnapshotCache.Add` semantics: the
    /// request id must not already exist.
    pub(in crate::oracle_contract) fn add_request_record<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        id: u64,
        request: &OracleRequest,
    ) -> CoreResult<()> {
        snapshot
            .try_add(
                Self::request_key(id),
                StorageItem::from_bytes(Self::encode_oracle_request(request)?),
            )
            .map_err(|e| {
                CoreError::invalid_operation(format!(
                    "OracleContract::request duplicate request id {id}: {e}"
                ))
            })
    }

    /// Returns the first `OracleResponse` transaction attribute (C#
    /// `tx.GetAttribute<OracleResponse>()`).
    pub(in crate::oracle_contract) fn oracle_response_attribute(
        tx: &Transaction,
    ) -> Option<&OracleResponse> {
        tx.attributes()
            .iter()
            .find_map(|attribute| match attribute {
                TransactionAttribute::OracleResponse(response) => Some(response),
                _ => None,
            })
    }

    /// C# `GetOriginalTxid`: the script container must be a transaction; when it
    /// carries an `OracleResponse` attribute (a request issued from a response
    /// callback) the original txid is taken from the answered request, otherwise
    /// it is the transaction's own hash.
    pub(in crate::oracle_contract) fn get_original_txid<
        P: neo_execution::native_contract_provider::NativeContractProvider + 'static,
        D: neo_execution::Diagnostic + 'static,
        B: neo_storage::CacheRead,
    >(
        &self,
        engine: &ApplicationEngine<P, D, B>,
        snapshot: &DataCache<B>,
    ) -> CoreResult<UInt256> {
        let container = engine.script_container().ok_or_else(|| {
            CoreError::invalid_operation("OracleContract: request requires a transaction container")
        })?;
        let tx = container.as_transaction().ok_or_else(|| {
            CoreError::invalid_operation("OracleContract: script container is not a transaction")
        })?;
        match Self::oracle_response_attribute(tx) {
            None => Ok(tx.hash()),
            Some(response) => {
                // C# uses the null-forgiving `GetRequest(...)!`: a missing record
                // dereferences null and faults.
                let request = self.read_request(snapshot, response.id)?.ok_or_else(|| {
                    CoreError::invalid_operation(
                        "OracleContract: original oracle request not found",
                    )
                })?;
                Ok(request.original_tx_id)
            }
        }
    }
}
