//! OracleContract native contract (id -9) + supporting `OracleRequest` type.
//!
//! Ports C# `Neo.SmartContract.Native.OracleContract`: the price
//! reads/writes (`getPrice`/`setPrice`), the request pipeline
//! (`request` validates, charges the price + response GAS, mints the
//! response GAS to the oracle account, allocates the request id and
//! stores the `OracleRequest` record plus the per-url id-list), the
//! response pipeline (`finish` re-enters the requesting contract's
//! callback, `verify` accepts oracle-response transactions), and the
//! `PostPersist` cleanup that removes answered requests and mints the
//! oracle fee to the designated oracle nodes.

use crate::hashes::ORACLE_CONTRACT_HASH;
use neo_config::{Hardfork, ProtocolSettings};
use neo_crypto::Crypto;
use neo_error::{CoreError, CoreResult};
use neo_execution::application_engine_contract::NativeArgNullMask;
use neo_execution::native_contract::OracleRequestDetails;
use neo_execution::{ApplicationEngine, Contract, NativeContract, NativeEvent, NativeMethod};
use neo_payloads::{OracleResponse, Transaction, TransactionAttribute};
use neo_primitives::{CallFlags, ContractParameterType, UInt160, UInt256};
use neo_serialization::BinarySerializer;
use neo_storage::persistence::{DataCache, SeekDirection};
use neo_storage::{StorageItem, StorageKey};
use neo_vm::{Interoperable, StackItem};
use neo_vm_rs::{ExecutionEngineLimits, StackValue};
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use std::any::Any;
use std::sync::LazyLock;

/// C# `OracleContract.MaxUrlLength` (strict-UTF8 bytes).
const MAX_URL_LENGTH: usize = 256;
/// C# `OracleContract.MaxFilterLength` (strict-UTF8 bytes).
const MAX_FILTER_LENGTH: usize = 128;
/// C# `OracleContract.MaxCallbackLength` (strict-UTF8 bytes).
const MAX_CALLBACK_LENGTH: usize = 32;
/// C# `OracleContract.MaxUserDataLength` (serialized bytes).
const MAX_USER_DATA_LENGTH: usize = 512;

/// Storage prefix for the oracle request price (C# `OracleContract.Prefix_Price`).
const PREFIX_PRICE: u8 = 5;
/// Storage prefix for the per-url request-id list (C# `Prefix_IdList`).
const PREFIX_ID_LIST: u8 = 6;
/// Storage prefix for the pending request records (C# `Prefix_Request`).
const PREFIX_REQUEST: u8 = 7;
/// Storage prefix for the next-request-id counter (C# `Prefix_RequestId`).
const PREFIX_REQUEST_ID: u8 = 9;

/// C# default oracle price: 0.5 GAS, in datoshi (genesis `InitializeAsync` value).
const DEFAULT_ORACLE_PRICE: i64 = 50000000;
/// C# `Request`: `gasForResponse` must be at least 0.1 GAS (`0_10000000` datoshi).
const MIN_GAS_FOR_RESPONSE: i64 = 10000000;
/// C# `Request`: at most 256 pending responses per url.
const MAX_PENDING_IDS_PER_URL: usize = 256;

/// Static accessor for the OracleContract native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct OracleContract;

impl OracleContract {
    /// Stable native contract id (-9 in C# Oracle contract).
    pub const ID: i32 = -9;
    /// Stable native contract name (matches C# `OracleContract.Name`).
    pub const NAME: &'static str = "OracleContract";

    /// Construct a new `OracleContract` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the script hash of the Oracle native contract.
    pub fn hash(&self) -> UInt160 {
        Self::script_hash()
    }

    /// Returns the script hash of the Oracle native contract (static).
    pub fn script_hash() -> UInt160 {
        *ORACLE_CONTRACT_HASH
    }

    /// Look up a single oracle request by its id (C# `GetRequest`).
    pub fn get_request(&self, snapshot: &DataCache, id: u64) -> CoreResult<Option<OracleRequest>> {
        self.read_request(snapshot, id)
    }

    /// Enumerate all pending oracle requests (C# `GetRequests`): a forward
    /// scan over `Prefix_Request`, the id read big-endian from the key
    /// suffix. Records that fail to decode are skipped (the signature
    /// predates fallibility and only this contract writes the records).
    pub fn get_requests(&self, snapshot: &DataCache) -> Vec<(u64, OracleRequest)> {
        let prefix = StorageKey::new(Self::ID, vec![PREFIX_REQUEST]);
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
    pub fn get_requests_by_url(
        &self,
        snapshot: &DataCache,
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
    fn put_price(&self, snapshot: &DataCache, price: i64) -> CoreResult<()> {
        let key = StorageKey::new(OracleContract::ID, vec![PREFIX_PRICE]);
        if snapshot.get(&key).is_none() {
            return Err(CoreError::invalid_data("OracleContract price is missing"));
        }
        snapshot.update(
            key,
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(price))),
        );
        Ok(())
    }

    /// Reads `Prefix_Price`. C# genesis initialization seeds this record and
    /// later code reads it with direct storage access, so absence is a fault.
    fn read_price(&self, snapshot: &DataCache) -> CoreResult<i64> {
        let item = snapshot
            .get(&StorageKey::new(OracleContract::ID, vec![PREFIX_PRICE]))
            .ok_or_else(|| CoreError::invalid_data("OracleContract price is missing"))?;
        BigInt::from_signed_bytes_le(&item.value_bytes())
            .to_i64()
            .ok_or_else(|| CoreError::invalid_data("OracleContract price out of range"))
    }

    /// The request-id counter key `(Oracle.ID, [Prefix_RequestId])`.
    fn request_id_key() -> StorageKey {
        StorageKey::new(OracleContract::ID, vec![PREFIX_REQUEST_ID])
    }

    /// The request record key `(Oracle.ID, [Prefix_Request, id_be8])` — C#
    /// `CreateStorageKey(Prefix_Request, ulong)` appends the id big-endian.
    fn request_key(id: u64) -> StorageKey {
        StorageKey::new(
            OracleContract::ID,
            crate::keys::prefixed_with_u64_be(PREFIX_REQUEST, id),
        )
    }

    /// The per-url id-list key `(Oracle.ID, [Prefix_IdList] ++ Hash160(url))` —
    /// C# `GetUrlHash` is `Crypto.Hash160(url.ToStrictUtf8Bytes())`.
    fn id_list_key(url: &str) -> StorageKey {
        StorageKey::new(
            OracleContract::ID,
            crate::keys::prefixed(PREFIX_ID_LIST, &Crypto::hash160(url.as_bytes())),
        )
    }

    /// Reads the request-id counter (`Prefix_RequestId`). The C# genesis
    /// `InitializeAsync` seeds it with `BigInteger.Zero`; later `Request`
    /// uses `GetAndChange(... )!`, so absence is a fault.
    fn read_request_id(&self, snapshot: &DataCache) -> CoreResult<u64> {
        let item = snapshot.get(&Self::request_id_key()).ok_or_else(|| {
            CoreError::invalid_data("OracleContract request-id counter is missing")
        })?;
        BigInt::from_signed_bytes_le(&item.value_bytes())
            .to_u64()
            .ok_or_else(|| CoreError::invalid_data("Oracle request-id counter out of range"))
    }

    /// Writes the request-id counter (C# `itemId.Add(1)` after taking the id).
    fn write_request_id(&self, snapshot: &DataCache, value: &BigInt) {
        snapshot.update(
            Self::request_id_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(value)),
        );
    }

    /// Encodes an `OracleRequest` as the C# `IInteroperable` layout: the
    /// BinarySerialized `Array[OriginalTxid, GasForResponse, Url, Filter|Null,
    /// CallbackContract, CallbackMethod, UserData]` (`OracleRequest.ToStackItem`).
    fn encode_oracle_request(request: &OracleRequest) -> CoreResult<Vec<u8>> {
        let item = request.to_stack_value();
        BinarySerializer::serialize_stack_value_default(&item)
            .map_err(|e| CoreError::serialization(format!("OracleRequest serialize: {e}")))
    }

    /// Decodes a stored `OracleRequest` record (C# `OracleRequest.FromStackItem`).
    fn decode_oracle_request(bytes: &[u8]) -> CoreResult<OracleRequest> {
        let limits = ExecutionEngineLimits::default();
        let item = BinarySerializer::deserialize_stack_value_with_limits(
            bytes,
            limits.max_item_size as usize,
            limits.max_stack_size as usize,
        )
        .map_err(|e| CoreError::deserialization(format!("OracleRequest: {e}")))?;
        OracleRequest::from_stack_value(item)
    }

    /// Encodes the per-url id-list (C# `IdList : InteroperableList<ulong>`): the
    /// BinarySerialized `Array` of `Integer` ids.
    fn encode_id_list(ids: &[u64]) -> CoreResult<Vec<u8>> {
        let item = OracleIdList::new(ids.to_vec()).to_stack_value();
        BinarySerializer::serialize_stack_value_default(&item)
            .map_err(|e| CoreError::serialization(format!("Oracle IdList serialize: {e}")))
    }

    /// Decodes the per-url id-list (C# `IdList.FromStackItem`, `(ulong)item.GetInteger()`).
    fn decode_id_list(bytes: &[u8]) -> CoreResult<Vec<u64>> {
        let limits = ExecutionEngineLimits::default();
        let item = BinarySerializer::deserialize_stack_value_with_limits(
            bytes,
            limits.max_item_size as usize,
            limits.max_stack_size as usize,
        )
        .map_err(|e| CoreError::deserialization(format!("Oracle IdList: {e}")))?;
        Ok(OracleIdList::from_stack_value(item)?.into_ids())
    }

    /// Reads a pending request record (C# `GetRequest`): `None` when no request
    /// with the given id exists.
    fn read_request(&self, snapshot: &DataCache, id: u64) -> CoreResult<Option<OracleRequest>> {
        match snapshot.get(&Self::request_key(id)) {
            None => Ok(None),
            Some(item) => Self::decode_oracle_request(&item.value_bytes()).map(Some),
        }
    }

    /// Adds a pending request record with C# `SnapshotCache.Add` semantics: the
    /// request id must not already exist.
    fn add_request_record(
        &self,
        snapshot: &DataCache,
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
    fn oracle_response_attribute(tx: &Transaction) -> Option<&OracleResponse> {
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
    fn get_original_txid(
        &self,
        engine: &ApplicationEngine,
        snapshot: &DataCache,
    ) -> CoreResult<UInt256> {
        let container = engine.script_container().ok_or_else(|| {
            CoreError::invalid_operation("OracleContract: request requires a transaction container")
        })?;
        let tx = container
            .as_any()
            .downcast_ref::<Transaction>()
            .ok_or_else(|| {
                CoreError::invalid_operation(
                    "OracleContract: script container is not a transaction",
                )
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

/// A pending oracle request (mirrors C# `OracleRequest`).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OracleRequest {
    /// The original transaction hash that created the request.
    pub original_tx_id: UInt256,
    /// GAS allocated for the response.
    pub gas_for_response: i64,
    /// The URL to fetch.
    pub url: String,
    /// Optional JSONPath filter.
    pub filter: Option<String>,
    /// Callback contract hash.
    pub callback_contract: UInt160,
    /// Callback method name.
    pub callback_method: String,
    /// User data (opaque payload, BinarySerializer-encoded).
    pub user_data: Vec<u8>,
}

impl OracleRequest {
    /// Construct a new oracle request (used by tests and by the
    /// service when emitting transactions).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        original_tx_id: UInt256,
        gas_for_response: i64,
        url: impl Into<String>,
        filter: Option<String>,
        callback_contract: UInt160,
        callback_method: impl Into<String>,
        user_data: Vec<u8>,
    ) -> Self {
        Self {
            original_tx_id,
            gas_for_response,
            url: url.into(),
            filter,
            callback_contract,
            callback_method: callback_method.into(),
            user_data,
        }
    }

    /// Converts to the C# `OracleRequest.ToStackItem` layout.
    pub fn to_stack_value(&self) -> StackValue {
        let filter_item = match &self.filter {
            Some(filter) => StackValue::ByteString(filter.as_bytes().to_vec()),
            None => StackValue::Null,
        };
        StackValue::Array(
            0,
            vec![
                StackValue::ByteString(self.original_tx_id.to_bytes()),
                StackValue::Integer(self.gas_for_response),
                StackValue::ByteString(self.url.as_bytes().to_vec()),
                filter_item,
                StackValue::ByteString(self.callback_contract.to_bytes()),
                StackValue::ByteString(self.callback_method.as_bytes().to_vec()),
                StackValue::ByteString(self.user_data.clone()),
            ],
        )
    }

    /// Parses the C# `OracleRequest.FromStackItem` layout from a StackValue.
    pub fn from_stack_value(stack_value: StackValue) -> CoreResult<Self> {
        let StackValue::Array(0, items) = stack_value else {
            return Err(CoreError::invalid_data("OracleRequest is not an array"));
        };
        if items.len() != 7 {
            return Err(CoreError::invalid_data("OracleRequest must have 7 fields"));
        }
        let field_bytes = |index: usize, name: &str| -> CoreResult<Vec<u8>> {
            items[index]
                .to_byte_string_bytes()
                .ok_or_else(|| CoreError::invalid_data(format!("OracleRequest {name}: not bytes")))
        };
        let field_string = |index: usize, name: &str| -> CoreResult<String> {
            String::from_utf8(field_bytes(index, name)?)
                .map_err(|e| CoreError::invalid_data(format!("OracleRequest {name}: {e}")))
        };
        let original_tx_id = UInt256::from_bytes(&field_bytes(0, "OriginalTxid")?)
            .map_err(|e| CoreError::invalid_data(format!("OracleRequest OriginalTxid: {e}")))?;
        let gas_for_response = neo_vm_rs::stack_value_as_bigint(&items[1])
            .map_err(|e| CoreError::invalid_data(format!("OracleRequest GasForResponse: {e}")))?
            .to_i64()
            .ok_or_else(|| CoreError::invalid_data("OracleRequest GasForResponse out of range"))?;
        let url = field_string(2, "Url")?;
        let filter = if matches!(items[3], StackValue::Null) {
            None
        } else {
            Some(field_string(3, "Filter")?)
        };
        let callback_contract = crate::args::bytes_to_hash160(
            &field_bytes(4, "CallbackContract")?,
            "OracleRequest CallbackContract",
        )?;
        let callback_method = field_string(5, "CallbackMethod")?;
        let user_data = field_bytes(6, "UserData")?;
        Ok(Self {
            original_tx_id,
            gas_for_response,
            url,
            filter,
            callback_contract,
            callback_method,
            user_data,
        })
    }
}

neo_vm::impl_interoperable_via_stack_value!(OracleRequest);

#[derive(Debug, Clone, PartialEq, Eq)]
struct OracleIdList {
    ids: Vec<u64>,
}

impl OracleIdList {
    fn new(ids: Vec<u64>) -> Self {
        Self { ids }
    }

    fn into_ids(self) -> Vec<u64> {
        self.ids
    }

    fn to_stack_value(&self) -> StackValue {
        StackValue::Array(
            0,
            self.ids
                .iter()
                .map(|id| StackValue::BigInteger(BigInt::from(*id).to_signed_bytes_le()))
                .collect(),
        )
    }

    fn from_stack_value(stack_value: StackValue) -> CoreResult<Self> {
        let StackValue::Array(0, items) = stack_value else {
            return Err(CoreError::invalid_data("Oracle IdList is not an array"));
        };
        let mut ids = Vec::with_capacity(items.len());
        for entry in &items {
            let id = neo_vm_rs::stack_value_as_bigint(entry)
                .map_err(|e| CoreError::invalid_data(format!("Oracle IdList entry: {e}")))?
                .to_u64()
                .ok_or_else(|| CoreError::invalid_data("Oracle IdList entry out of range"))?;
            ids.push(id);
        }
        Ok(Self { ids })
    }
}

neo_vm::impl_interoperable_via_stack_value!(OracleIdList);

static ORACLE_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    vec![
        NativeMethod::new(
            "getPrice".to_string(),
            1 << 15,
            true,
            CallFlags::READ_STATES.bits(),
            vec![],
            ContractParameterType::Integer,
        ),
        // Committee-gated price setter (not safe, States, Void).
        NativeMethod::new(
            "setPrice".to_string(),
            1 << 15,
            false,
            CallFlags::STATES.bits(),
            vec![ContractParameterType::Integer],
            ContractParameterType::Void,
        )
        .with_parameter_names(["price"]),
        // C# Request: CpuFee 0, States | AllowNotify, Void.
        NativeMethod::new(
            "request".to_string(),
            0,
            false,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
            vec![
                ContractParameterType::String,
                ContractParameterType::String,
                ContractParameterType::String,
                ContractParameterType::Any,
                ContractParameterType::Integer,
            ],
            ContractParameterType::Void,
        )
        .with_parameter_names(["url", "filter", "callback", "userData", "gasForResponse"]),
        // C# Finish: CpuFee 0, States | AllowCall | AllowNotify, Void.
        NativeMethod::new(
            "finish".to_string(),
            0,
            false,
            (CallFlags::STATES | CallFlags::ALLOW_CALL | CallFlags::ALLOW_NOTIFY).bits(),
            vec![],
            ContractParameterType::Void,
        ),
        // C# Verify: CpuFee 1 << 15, CallFlags.None — and the C# manifest
        // marks it Safe ((None & ~ReadOnly) == 0).
        NativeMethod::new(
            "verify".to_string(),
            1 << 15,
            true,
            CallFlags::NONE.bits(),
            vec![],
            ContractParameterType::Boolean,
        ),
    ]
});

/// Oracle's `[ContractEvent]` declarations (OracleContract.cs:46-53), both
/// ungated: `OracleRequest` at order 0, `OracleResponse` at order 1.
static ORACLE_EVENTS: LazyLock<Vec<NativeEvent>> = LazyLock::new(|| {
    vec![
        NativeEvent::new(
            0,
            "OracleRequest",
            &[
                ("Id", ContractParameterType::Integer),
                ("RequestContract", ContractParameterType::Hash160),
                ("Url", ContractParameterType::String),
                ("Filter", ContractParameterType::String),
            ],
        ),
        NativeEvent::new(
            1,
            "OracleResponse",
            &[
                ("Id", ContractParameterType::Integer),
                ("OriginalTx", ContractParameterType::Hash256),
            ],
        ),
    ]
});

impl NativeContract for OracleContract {
    fn id(&self) -> i32 {
        Self::ID
    }

    fn hash(&self) -> UInt160 {
        Self::script_hash()
    }

    fn name(&self) -> &str {
        Self::NAME
    }

    fn methods(&self) -> &[NativeMethod] {
        &ORACLE_METHODS
    }

    fn event_descriptors(&self) -> &[NativeEvent] {
        &ORACLE_EVENTS
    }

    /// C# `OracleContract.Activations => [null, HF_Faun]` (OracleContract.cs:56):
    /// active from genesis, but the manifest's supported standards update at
    /// Faun, so the Faun boundary must refresh the stored contract state.
    fn activations(&self) -> Vec<Hardfork> {
        vec![Hardfork::HfFaun]
    }

    /// C# `OracleContract.OnManifestCompose` (OracleContract.cs:58-64): NEP-30
    /// once HF_Faun is enabled at the height; no standards before it.
    fn supported_standards(&self, settings: &ProtocolSettings, block_height: u32) -> Vec<String> {
        if settings.is_hardfork_enabled(Hardfork::HfFaun, block_height) {
            vec!["NEP-30".to_string()]
        } else {
            Vec::new()
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    /// Url + original txid pair consumed by the engine's oracle-response
    /// witness path (`CheckWitness` signer inheritance).
    ///
    /// C# `GetRequest(...)` exposed through the native-contract seam so the
    /// engine can resolve oracle-response witnesses without depending on
    /// `neo-native-contracts`.
    fn oracle_request_url_full(
        &self,
        snapshot: &DataCache,
        id: u64,
    ) -> CoreResult<Option<OracleRequestDetails>> {
        Ok(self
            .read_request(snapshot, id)?
            .map(|request| OracleRequestDetails::new(request.url, request.original_tx_id)))
    }

    /// C# `OracleContract.InitializeAsync(engine, hardfork)` for
    /// `hardfork == ActiveIn` (the Oracle contract is genesis-active): seed the
    /// request-id counter with `BigInteger.Zero` (stored as empty bytes) and the
    /// request price with 0.5 GAS (`0_50000000` datoshi).
    fn initialize(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        let snapshot = engine.snapshot_cache();
        snapshot.add(
            Self::request_id_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(0))),
        );
        snapshot.add(
            StorageKey::new(Self::ID, vec![PREFIX_PRICE]),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_ORACLE_PRICE,
            ))),
        );
        Ok(())
    }

    /// C# `OracleContract.PostPersistAsync`: for every oracle-response
    /// transaction in the persisting block, remove the answered request
    /// record and its id from the per-url id-list, then mint the oracle
    /// price to the designated oracle node selected by `id % nodes.len()`.
    fn post_persist(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        let (block_index, response_ids): (u32, Vec<u64>) = {
            let block = engine.persisting_block().ok_or_else(|| {
                CoreError::invalid_operation(
                    "OracleContract::post_persist requires a persisting block",
                )
            })?;
            let ids = block
                .transactions
                .iter()
                .filter_map(|tx| Self::oracle_response_attribute(tx).map(|response| response.id))
                .collect();
            (block.index(), ids)
        };

        let snapshot = engine.snapshot_cache();
        let mut nodes: Option<Vec<(UInt160, BigInt)>> = None;
        for id in response_ids {
            // Remove the request from storage (skip responses without one).
            let key = Self::request_key(id);
            let Some(item) = snapshot.get(&key) else {
                continue;
            };
            let request = Self::decode_oracle_request(&item.value_bytes())?;
            snapshot.delete(&key);

            // Remove the id from the url id-list; C# throws when the id is
            // not listed, and deletes the entry once the list is empty.
            let list_key = Self::id_list_key(&request.url);
            let mut list = match snapshot.get(&list_key) {
                Some(list_item) => Self::decode_id_list(&list_item.value_bytes())?,
                None => Vec::new(),
            };
            let Some(position) = list.iter().position(|listed| *listed == id) else {
                return Err(CoreError::invalid_operation(
                    "OracleContract::post_persist: request id missing from the url id-list",
                ));
            };
            list.remove(position);
            if list.is_empty() {
                snapshot.delete(&list_key);
            } else {
                snapshot.update(
                    list_key,
                    StorageItem::from_bytes(Self::encode_id_list(&list)?),
                );
            }

            // Accumulate the oracle fee for the node selected by the id.
            if nodes.is_none() {
                let points = crate::RoleManagement::new().get_designated_by_role_at(
                    &snapshot,
                    crate::Role::Oracle,
                    block_index,
                )?;
                nodes = Some(
                    points
                        .into_iter()
                        .map(|point| {
                            (
                                UInt160::from_script(&Contract::create_signature_redeem_script(
                                    point,
                                )),
                                BigInt::from(0),
                            )
                        })
                        .collect(),
                );
            }
            if let Some(nodes) = nodes.as_mut() {
                if !nodes.is_empty() {
                    let index = usize::try_from(id % nodes.len() as u64).unwrap_or(0);
                    let price = self.read_price(&snapshot)?;
                    nodes[index].1 += BigInt::from(price);
                }
            }
        }

        if let Some(nodes) = nodes {
            for (account, gas) in nodes {
                if gas > BigInt::from(0) {
                    crate::GasToken::new().gas_mint(engine, &account, &gas, false)?;
                }
            }
        }
        Ok(())
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        match method {
            "getPrice" => {
                let price = self.read_price(&snapshot)?;
                Ok(BigInt::from(price).to_signed_bytes_le())
            }
            "setPrice" => {
                // C#: validate price > 0 -> AssertCommittee -> overwrite Prefix_Price.
                let price = args
                    .first()
                    .map(|b| BigInt::from_signed_bytes_le(b))
                    .and_then(|b| b.to_i64())
                    .ok_or_else(|| {
                        CoreError::invalid_operation("OracleContract::setPrice requires a price")
                    })?;
                if price <= 0 {
                    return Err(CoreError::invalid_operation(format!(
                        "Oracle price must be positive, got {price}"
                    )));
                }
                crate::committee::assert_committee(engine, "setPrice")?;
                self.put_price(&engine.snapshot_cache(), price)?;
                Ok(Vec::new())
            }
            "request" => {
                // C# Request(url, filter?, callback, userData, gasForResponse):
                // size/shape validations, fees + response-GAS mint, id
                // allocation, request record + per-url id-list, notification.
                let url = String::from_utf8(
                    args.first()
                        .ok_or_else(|| {
                            CoreError::invalid_operation("OracleContract::request requires a url")
                        })?
                        .clone(),
                )
                .map_err(|e| {
                    CoreError::invalid_operation(format!("OracleContract::request url: {e}"))
                })?;
                if url.len() > MAX_URL_LENGTH {
                    return Err(CoreError::invalid_operation(format!(
                        "URL size {} bytes exceeds maximum allowed size of {MAX_URL_LENGTH} bytes.",
                        url.len()
                    )));
                }

                // `filter` is a nullable String: a Null arg (bit 1 of the
                // native arg null-mask) means "no filter".
                let filter_is_null = engine
                    .get_state::<NativeArgNullMask>()
                    .is_some_and(|mask| mask.0 & (1 << 1) != 0);
                let filter = if filter_is_null {
                    None
                } else {
                    let bytes = args.get(1).ok_or_else(|| {
                        CoreError::invalid_operation("OracleContract::request requires a filter")
                    })?;
                    Some(String::from_utf8(bytes.clone()).map_err(|e| {
                        CoreError::invalid_operation(format!("OracleContract::request filter: {e}"))
                    })?)
                };
                let filter_size = filter.as_ref().map_or(0, String::len);
                if filter_size > MAX_FILTER_LENGTH {
                    return Err(CoreError::invalid_operation(format!(
                        "Filter size {filter_size} bytes exceeds maximum allowed size of {MAX_FILTER_LENGTH} bytes.",
                    )));
                }

                let callback = String::from_utf8(
                    args.get(2)
                        .ok_or_else(|| {
                            CoreError::invalid_operation(
                                "OracleContract::request requires a callback",
                            )
                        })?
                        .clone(),
                )
                .map_err(|e| {
                    CoreError::invalid_operation(format!("OracleContract::request callback: {e}"))
                })?;
                if callback.len() > MAX_CALLBACK_LENGTH {
                    return Err(CoreError::invalid_operation(format!(
                        "Callback size {} bytes exceeds maximum allowed size of {MAX_CALLBACK_LENGTH} bytes.",
                        callback.len()
                    )));
                }
                if callback.starts_with('_') {
                    return Err(CoreError::invalid_operation(
                        "Callback cannot start with underscore.",
                    ));
                }

                let user_data_bytes = args.get(3).cloned().unwrap_or_default();

                let gas_for_response = args
                    .get(4)
                    .map(|b| BigInt::from_signed_bytes_le(b))
                    .and_then(|b| b.to_i64())
                    .ok_or_else(|| {
                        CoreError::invalid_operation(
                            "OracleContract::request requires a gasForResponse",
                        )
                    })?;
                if gas_for_response < MIN_GAS_FOR_RESPONSE {
                    return Err(CoreError::invalid_operation(format!(
                        "gasForResponse {gas_for_response} must be at least 0.1 datoshi.",
                    )));
                }

                // engine.AddFee(GetPrice * FeeFactor) — the request price, in
                // datoshi — then AddFee(gasForResponse * FeeFactor) and the
                // response-GAS mint to the oracle account.
                let price = self.read_price(&snapshot)?;
                engine
                    .charge_execution_fee(u64::try_from(price).unwrap_or(0))
                    .map_err(|e| {
                        CoreError::invalid_operation(format!(
                            "OracleContract::request: price fee: {e}"
                        ))
                    })?;
                engine
                    .charge_execution_fee(u64::try_from(gas_for_response).unwrap_or(0))
                    .map_err(|e| {
                        CoreError::invalid_operation(format!(
                            "OracleContract::request: response fee: {e}"
                        ))
                    })?;
                crate::GasToken::new().gas_mint(
                    engine,
                    &Self::script_hash(),
                    &BigInt::from(gas_for_response),
                    false,
                )?;

                // Increase the request id (the request takes the pre-increment value).
                let id = self.read_request_id(&snapshot)?;
                self.write_request_id(&snapshot, &(BigInt::from(id) + 1));

                // The request must come from a deployed contract
                // (C# ContractManagement.IsContract(CallingScriptHash)).
                let calling = engine.get_calling_script_hash().ok_or_else(|| {
                    CoreError::invalid_operation(
                        "OracleContract::request requires a calling contract",
                    )
                })?;
                if !crate::ContractManagement::is_contract(&snapshot, &calling) {
                    return Err(CoreError::invalid_operation(
                        "OracleContract::request: caller is not a deployed contract",
                    ));
                }

                // C#: UserData = BinarySerializer.Serialize(userData,
                // MaxUserDataLength, engine.Limits.MaxStackSize) — re-encode
                // the marshaled item under the 512-byte cap.
                let limits = ExecutionEngineLimits::default();
                let user_data_item = BinarySerializer::deserialize_stack_value_with_limits(
                    &user_data_bytes,
                    limits.max_item_size as usize,
                    limits.max_stack_size as usize,
                )
                .map_err(|e| {
                    CoreError::invalid_operation(format!("OracleContract::request userData: {e}"))
                })?;
                let user_data = BinarySerializer::serialize_stack_value_with_limits(
                    &user_data_item,
                    MAX_USER_DATA_LENGTH,
                    limits.max_stack_size as usize,
                )
                .map_err(|e| {
                    CoreError::invalid_operation(format!("OracleContract::request userData: {e}"))
                })?;

                let request = OracleRequest {
                    original_tx_id: self.get_original_txid(engine, &snapshot)?,
                    gas_for_response,
                    url: url.clone(),
                    filter: filter.clone(),
                    callback_contract: calling,
                    callback_method: callback,
                    user_data,
                };
                self.add_request_record(&snapshot, id, &request)?;

                // Add the id to the per-url IdList (capped at 256 pending).
                let list_key = Self::id_list_key(&url);
                let mut list = match snapshot.get(&list_key) {
                    Some(item) => Self::decode_id_list(&item.value_bytes())?,
                    None => Vec::new(),
                };
                if list.len() >= MAX_PENDING_IDS_PER_URL {
                    return Err(CoreError::invalid_operation(
                        "There are too many pending responses for this url",
                    ));
                }
                list.push(id);
                snapshot.update(
                    list_key,
                    StorageItem::from_bytes(Self::encode_id_list(&list)?),
                );

                let filter_item = match &filter {
                    Some(f) => StackItem::from_byte_string(f.as_bytes().to_vec()),
                    None => StackItem::null(),
                };
                engine
                    .send_notification(
                        Self::script_hash(),
                        "OracleRequest".to_string(),
                        vec![
                            StackItem::from_int(BigInt::from(id)),
                            StackItem::from_byte_string(calling.to_bytes()),
                            StackItem::from_byte_string(url.as_bytes().to_vec()),
                            filter_item,
                        ],
                    )
                    .map_err(|e| {
                        CoreError::invalid_operation(format!("OracleContract::request notify: {e}"))
                    })?;
                Ok(Vec::new())
            }
            "finish" => {
                // C# Finish: only valid as the single direct call of an
                // oracle-response transaction's fixed script.
                if engine.invocation_stack().len() != 2 {
                    return Err(CoreError::invalid_operation(
                        "OracleContract::finish: invalid invocation stack depth",
                    ));
                }
                let current = engine.current_script_hash().ok_or_else(|| {
                    CoreError::invalid_operation("OracleContract::finish: no current script")
                })?;
                if engine.get_invocation_counter(&current) != 1 {
                    return Err(CoreError::invalid_operation(
                        "OracleContract::finish: invalid invocation counter",
                    ));
                }
                let (id, code_byte, result) = {
                    let container = engine.script_container().ok_or_else(|| {
                        CoreError::invalid_operation(
                            "OracleContract::finish requires a transaction container",
                        )
                    })?;
                    let tx = container
                        .as_any()
                        .downcast_ref::<Transaction>()
                        .ok_or_else(|| {
                            CoreError::invalid_operation(
                                "OracleContract::finish: script container is not a transaction",
                            )
                        })?;
                    let response = Self::oracle_response_attribute(tx)
                        .ok_or_else(|| CoreError::invalid_operation("Oracle response not found"))?;
                    (response.id, response.code as u8, response.result.clone())
                };
                let request = self
                    .read_request(&snapshot, id)?
                    .ok_or_else(|| CoreError::invalid_operation("Oracle request not found"))?;
                engine
                    .send_notification(
                        Self::script_hash(),
                        "OracleResponse".to_string(),
                        vec![
                            StackItem::from_int(BigInt::from(id)),
                            StackItem::from_byte_string(request.original_tx_id.to_bytes()),
                        ],
                    )
                    .map_err(|e| {
                        CoreError::invalid_operation(format!("OracleContract::finish notify: {e}"))
                    })?;
                let user_data = BinarySerializer::deserialize(
                    &request.user_data,
                    &ExecutionEngineLimits::default(),
                    None,
                )
                .map_err(|e| {
                    CoreError::deserialization(format!("OracleContract::finish userData: {e}"))
                })?;
                // C# CallFromNativeContractAsync(Hash, CallbackContract,
                // CallbackMethod, Url, userData, (int)Code, Result): the
                // callback runs after this native call returns.
                engine.queue_contract_call_from_native(
                    Self::script_hash(),
                    request.callback_contract,
                    request.callback_method.clone(),
                    vec![
                        StackItem::from_byte_string(request.url.as_bytes().to_vec()),
                        user_data,
                        StackItem::from_int(BigInt::from(i64::from(code_byte))),
                        StackItem::from_byte_string(result),
                    ],
                );
                Ok(Vec::new())
            }
            "verify" => {
                // C#: `(Transaction?)engine.ScriptContainer` — a null
                // container yields false, a non-transaction container is an
                // invalid cast (fault), otherwise true iff the transaction
                // carries an OracleResponse attribute.
                let Some(container) = engine.script_container() else {
                    return Ok(vec![0]);
                };
                let tx = container
                    .as_any()
                    .downcast_ref::<Transaction>()
                    .ok_or_else(|| {
                        CoreError::invalid_operation(
                            "OracleContract::verify: script container is not a transaction",
                        )
                    })?;
                Ok(vec![u8::from(
                    Self::oracle_response_attribute(tx).is_some(),
                )])
            }
            other => Err(CoreError::invalid_operation(format!(
                "OracleContract method '{other}' is not implemented"
            ))),
        }
    }
}

#[cfg(test)]
mod oracle_native_tests {
    use super::*;
    use neo_storage::persistence::DataCache;
    use neo_storage::{StorageItem, StorageKey};

    #[test]
    fn native_contract_surface() {
        let c = OracleContract::new();
        let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            ["getPrice", "setPrice", "request", "finish", "verify"]
        );

        let setter = c.methods().iter().find(|m| m.name == "setPrice").unwrap();
        assert!(!setter.safe);
        assert_eq!(setter.required_call_flags, CallFlags::STATES.bits());
        assert_eq!(setter.parameters, vec![ContractParameterType::Integer]);
        assert_eq!(setter.return_type, ContractParameterType::Void);

        let request = c.methods().iter().find(|m| m.name == "request").unwrap();
        assert!(!request.safe);
        assert_eq!(request.cpu_fee, 0);
        assert_eq!(
            request.required_call_flags,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits()
        );
        assert_eq!(
            request.parameters,
            vec![
                ContractParameterType::String,
                ContractParameterType::String,
                ContractParameterType::String,
                ContractParameterType::Any,
                ContractParameterType::Integer,
            ]
        );
        assert_eq!(request.return_type, ContractParameterType::Void);

        let finish = c.methods().iter().find(|m| m.name == "finish").unwrap();
        assert!(!finish.safe);
        assert_eq!(finish.cpu_fee, 0);
        assert_eq!(
            finish.required_call_flags,
            (CallFlags::STATES | CallFlags::ALLOW_CALL | CallFlags::ALLOW_NOTIFY).bits()
        );
        assert!(finish.parameters.is_empty());
        assert_eq!(finish.return_type, ContractParameterType::Void);

        let verify = c.methods().iter().find(|m| m.name == "verify").unwrap();
        // C# ContractMethodMetadata: Safe = (RequiredCallFlags & ~ReadOnly) == 0,
        // and Verify declares CallFlags.None.
        assert!(verify.safe);
        assert_eq!(verify.cpu_fee, 1 << 15);
        assert_eq!(verify.required_call_flags, CallFlags::NONE.bits());
        assert!(verify.parameters.is_empty());
        assert_eq!(verify.return_type, ContractParameterType::Boolean);
    }

    #[test]
    fn set_price_write_round_trips() {
        let cache = DataCache::new(false);
        cache.add(
            StorageKey::new(OracleContract::ID, vec![PREFIX_PRICE]),
            StorageItem::from_bytes(BigInt::from(DEFAULT_ORACLE_PRICE).to_signed_bytes_le()),
        );
        // The setter's storage effect (overwrite Prefix_Price) is observed by
        // the getter's reader.
        OracleContract::new()
            .put_price(&cache, 7_5000000)
            .expect("initialized price can be overwritten"); // 0.75 GAS
        assert_eq!(OracleContract::new().read_price(&cache).unwrap(), 7_5000000);
    }

    #[test]
    fn price_requires_initialized_storage() {
        let cache = DataCache::new(false);
        assert!(OracleContract::new().read_price(&cache).is_err());
        assert!(OracleContract::new().put_price(&cache, 12345678).is_err());

        let key = StorageKey::new(OracleContract::ID, vec![PREFIX_PRICE]);
        cache.add(
            key,
            StorageItem::from_bytes(BigInt::from(12345678).to_signed_bytes_le()),
        );
        assert_eq!(OracleContract::new().read_price(&cache).unwrap(), 12345678);
    }

    fn sample_request(filter: Option<String>) -> OracleRequest {
        OracleRequest::new(
            UInt256::from_bytes(&[0xAA; 32]).unwrap(),
            1_0000000,
            "https://example.org/data",
            filter,
            UInt160::from_bytes(&[0xCB; 20]).unwrap(),
            "oracleCallback",
            BinarySerializer::serialize(
                &StackItem::from_int(BigInt::from(42)),
                &ExecutionEngineLimits::default(),
            )
            .unwrap(),
        )
    }

    #[test]
    fn request_record_round_trips() {
        for filter in [Some("$.value".to_string()), None] {
            let request = sample_request(filter);
            let bytes = OracleContract::encode_oracle_request(&request).unwrap();
            let decoded = OracleContract::decode_oracle_request(&bytes).unwrap();
            assert_eq!(decoded, request);
        }
    }

    #[test]
    fn request_record_add_rejects_duplicate_id_like_csharp() {
        // C# Request writes the record via SnapshotCache.Add, so a reused
        // request id faults and must not overwrite the existing request.
        let cache = DataCache::new(false);
        let contract = OracleContract::new();
        let original = sample_request(None);
        contract
            .add_request_record(&cache, 5, &original)
            .expect("first add succeeds");
        let replacement = sample_request(Some("$.replacement".to_string()));
        let err = contract
            .add_request_record(&cache, 5, &replacement)
            .expect_err("duplicate request id must fault");
        assert!(err.to_string().contains("duplicate request id 5"), "{err}");
        assert_eq!(contract.read_request(&cache, 5).unwrap(), Some(original));
    }

    #[test]
    fn request_record_layout_matches_csharp_to_stack_item() {
        // C# OracleRequest.ToStackItem: an Array of 7 items —
        // [txid bytes, Integer, url, filter|Null, contract bytes, method, userdata].
        let request = sample_request(Some("$.x".to_string()));
        let bytes = OracleContract::encode_oracle_request(&request).unwrap();
        let expected_item = StackItem::from_array(vec![
            StackItem::from_byte_string(request.original_tx_id.to_bytes()),
            StackItem::from_int(BigInt::from(request.gas_for_response)),
            StackItem::from_byte_string(request.url.as_bytes().to_vec()),
            StackItem::from_byte_string(b"$.x".to_vec()),
            StackItem::from_byte_string(request.callback_contract.to_bytes()),
            StackItem::from_byte_string(request.callback_method.as_bytes().to_vec()),
            StackItem::from_byte_string(request.user_data.clone()),
        ]);
        let expected =
            BinarySerializer::serialize(&expected_item, &ExecutionEngineLimits::default()).unwrap();
        assert_eq!(bytes, expected);
        assert_eq!(
            Interoperable::to_stack_value(&request).unwrap(),
            StackValue::try_from(expected_item.clone()).unwrap()
        );

        let mut trait_decoded = OracleRequest::default();
        Interoperable::from_stack_value(
            &mut trait_decoded,
            StackValue::try_from(expected_item).unwrap(),
        )
        .unwrap();
        assert_eq!(trait_decoded, request);

        let item =
            BinarySerializer::deserialize(&bytes, &ExecutionEngineLimits::default(), None).unwrap();
        let StackItem::Array(array) = item else {
            panic!("OracleRequest must serialize as an Array (not Struct)");
        };
        let items = array.items();
        assert_eq!(items.len(), 7);
        assert_eq!(items[0].as_bytes().unwrap(), vec![0xAA; 32]);
        assert_eq!(items[1].as_int().unwrap(), BigInt::from(1_0000000));
        assert_eq!(
            items[2].as_bytes().unwrap(),
            b"https://example.org/data".to_vec()
        );
        assert_eq!(items[3].as_bytes().unwrap(), b"$.x".to_vec());
        assert_eq!(items[4].as_bytes().unwrap(), vec![0xCB; 20]);
        assert_eq!(items[5].as_bytes().unwrap(), b"oracleCallback".to_vec());
        assert_eq!(
            items[6].as_bytes().unwrap(),
            BinarySerializer::serialize(
                &StackItem::from_int(BigInt::from(42)),
                &ExecutionEngineLimits::default()
            )
            .unwrap()
        );

        // A null filter serializes as StackItem::Null in slot 3.
        let no_filter = OracleContract::encode_oracle_request(&sample_request(None)).unwrap();
        let StackItem::Array(array) =
            BinarySerializer::deserialize(&no_filter, &ExecutionEngineLimits::default(), None)
                .unwrap()
        else {
            panic!("array expected");
        };
        assert!(matches!(array.items()[3], StackItem::Null));
    }

    #[test]
    fn oracle_storage_codecs_use_stack_value_projection() {
        fn slice_between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
            let start_index = source.find(start).expect("start marker exists");
            let end_index = source[start_index..]
                .find(end)
                .map(|offset| start_index + offset)
                .expect("end marker exists");
            &source[start_index..end_index]
        }

        let source = include_str!("oracle_contract.rs");
        let request_encoder = slice_between(
            source,
            "fn encode_oracle_request",
            "fn decode_oracle_request",
        );
        assert!(request_encoder.contains("to_stack_value"));
        assert!(request_encoder.contains("serialize_stack_value_default"));
        assert!(!request_encoder.contains("StackItem::from_array"));
        assert!(!request_encoder.contains("BinarySerializer::serialize("));

        let request_decoder =
            slice_between(source, "fn decode_oracle_request", "fn encode_id_list");
        assert!(request_decoder.contains("deserialize_stack_value_with_limits"));
        assert!(request_decoder.contains("OracleRequest::from_stack_value"));
        assert!(!request_decoder.contains("BinarySerializer::deserialize("));

        let id_list_encoder = slice_between(source, "fn encode_id_list", "fn decode_id_list");
        assert!(id_list_encoder.contains("OracleIdList::new"));
        assert!(id_list_encoder.contains("to_stack_value"));
        assert!(!id_list_encoder.contains("StackValue::Array"));
        assert!(id_list_encoder.contains("serialize_stack_value_default"));
        assert!(!id_list_encoder.contains("StackItem::from_array"));
        assert!(!id_list_encoder.contains("BinarySerializer::serialize("));

        let id_list_decoder = slice_between(source, "fn decode_id_list", "fn read_request");
        assert!(id_list_decoder.contains("deserialize_stack_value_with_limits"));
        assert!(id_list_decoder.contains("OracleIdList::from_stack_value"));
        assert!(!id_list_decoder.contains("StackValue::Array"));
        assert!(!id_list_decoder.contains("stack_value_as_bigint"));
        assert!(!id_list_decoder.contains("BinarySerializer::deserialize("));

        // C# OracleContract.Request stores `BinarySerializer.Serialize(userData,
        // MaxUserDataLength, engine.Limits.MaxStackSize)` (OracleContract.cs:265).
        // The Rust request path only needs a value projection before reserializing
        // under that byte cap; the later `finish` callback still materializes a
        // StackItem because it queues a VM call.
        let request_user_data = slice_between(
            source,
            "// C#: UserData = BinarySerializer.Serialize(userData,",
            "let request = OracleRequest",
        );
        assert!(request_user_data.contains("deserialize_stack_value_with_limits"));
        assert!(request_user_data.contains("serialize_stack_value_with_limits"));
        assert!(!request_user_data.contains("BinarySerializer::deserialize("));
        assert!(!request_user_data.contains("BinarySerializer::serialize_with_limits("));
    }

    #[test]
    fn id_list_round_trips_and_key_uses_url_hash160() {
        let ids = vec![0u64, 1, 7, u64::from(u32::MAX) + 5, u64::MAX];
        let bytes = OracleContract::encode_id_list(&ids).unwrap();
        let expected = BinarySerializer::serialize(
            &StackItem::from_array(
                ids.iter()
                    .map(|id| StackItem::from_int(BigInt::from(*id)))
                    .collect::<Vec<_>>(),
            ),
            &ExecutionEngineLimits::default(),
        )
        .unwrap();
        assert_eq!(bytes, expected);
        assert_eq!(OracleContract::decode_id_list(&bytes).unwrap(), ids);

        // C# GetUrlHash = Crypto.Hash160(strict utf8 url) appended to Prefix_IdList.
        let url = "https://example.org/data";
        let key = OracleContract::id_list_key(url);
        let mut expected = vec![PREFIX_ID_LIST];
        expected.extend_from_slice(&Crypto::hash160(url.as_bytes()));
        assert_eq!(key.key(), expected.as_slice());

        // Request key is Prefix_Request ++ big-endian id.
        let rkey = OracleContract::request_key(0x0102030405060708);
        assert_eq!(
            rkey.key(),
            [
                PREFIX_REQUEST,
                0x01,
                0x02,
                0x03,
                0x04,
                0x05,
                0x06,
                0x07,
                0x08
            ]
        );
    }

    #[test]
    fn oracle_id_list_interoperable_projection_matches_csharp_shape() {
        let ids = vec![0u64, 7, u64::MAX];
        let state = OracleIdList::new(ids.clone());
        let expected_value = StackValue::Array(
            ids.iter()
                .map(|id| StackValue::BigInteger(BigInt::from(*id).to_signed_bytes_le()))
                .collect::<Vec<_>>(),
        );

        let trait_value = Interoperable::to_stack_value(&state).unwrap();
        assert_eq!(trait_value, expected_value);

        let mut parsed = OracleIdList::new(Vec::new());
        Interoperable::from_stack_value(&mut parsed, trait_value).unwrap();
        assert_eq!(parsed.into_ids(), ids);
    }

    #[test]
    fn request_id_counter_round_trips() {
        let cache = DataCache::new(false);
        // C# genesis initialization seeds Prefix_RequestId. Later reads use
        // GetAndChange(...), so a missing counter faults instead of inventing 0.
        assert!(OracleContract::new().read_request_id(&cache).is_err());
        OracleContract::new().write_request_id(&cache, &BigInt::from(1));
        assert_eq!(OracleContract::new().read_request_id(&cache).unwrap(), 1);
        OracleContract::new().write_request_id(&cache, &BigInt::from(u64::MAX));
        assert_eq!(
            OracleContract::new().read_request_id(&cache).unwrap(),
            u64::MAX
        );
    }

    #[test]
    fn request_queries_resolve_storage() {
        let cache = DataCache::new(false);
        let contract = OracleContract::new();
        assert!(contract.get_request(&cache, 1).unwrap().is_none());
        assert!(contract.get_requests(&cache).is_empty());
        assert!(
            contract
                .get_requests_by_url(&cache, "https://example.org/data")
                .unwrap()
                .is_empty()
        );

        let request = sample_request(None);
        cache.add(
            OracleContract::request_key(3),
            StorageItem::from_bytes(OracleContract::encode_oracle_request(&request).unwrap()),
        );
        cache.add(
            OracleContract::id_list_key(&request.url),
            StorageItem::from_bytes(OracleContract::encode_id_list(&[3]).unwrap()),
        );

        assert_eq!(
            contract.get_request(&cache, 3).unwrap(),
            Some(request.clone())
        );
        assert_eq!(contract.get_requests(&cache), vec![(3, request.clone())]);
        assert_eq!(
            contract.get_requests_by_url(&cache, &request.url).unwrap(),
            vec![(3, request.clone())]
        );

        // The native-contract seam exposes the same record to the engine.
        let details = NativeContract::oracle_request_url_full(&contract, &cache, 3)
            .unwrap()
            .expect("details");
        assert_eq!(details.url, request.url);
        assert_eq!(details.original_tx_id, request.original_tx_id);
    }

    /// C# `OracleContract.OnManifestCompose` (OracleContract.cs:58-64): no
    /// standards before HF_Faun, NEP-30 from the Faun height — and the Faun
    /// boundary is a manifest-refresh activation (`Activations => [null,
    /// HF_Faun]`, OracleContract.cs:56).
    #[test]
    fn manifest_standards_gain_nep30_at_faun() {
        use neo_config::{Hardfork, ProtocolSettings};
        use neo_execution::native_contract::build_native_contract_state;

        let unscheduled =
            build_native_contract_state(&OracleContract, &ProtocolSettings::default(), 0);
        assert!(unscheduled.manifest.supported_standards.is_empty());

        let mut settings = ProtocolSettings::default();
        settings.hardforks.insert(Hardfork::HfFaun, 10);
        let before = build_native_contract_state(&OracleContract, &settings, 9);
        assert!(before.manifest.supported_standards.is_empty());
        let after = build_native_contract_state(&OracleContract, &settings, 10);
        assert_eq!(after.manifest.supported_standards, ["NEP-30"]);

        assert_eq!(
            NativeContract::activations(&OracleContract),
            [Hardfork::HfFaun]
        );
    }
}

#[cfg(test)]
mod oracle_request_finish_tests {
    use super::*;
    use crate::test_support::deploy_native as deploy_contract;
    use neo_config::ProtocolSettings;
    use neo_execution::contract_state::ContractState;
    use neo_execution::native_contract::build_native_contract_state;
    use neo_io::{BinaryWriter, Serializable};
    use neo_manifest::{
        ContractAbi, ContractManifest, ContractMethodDescriptor, ContractParameterDefinition,
        ContractPermission, NefFile, WildCardContainer,
    };
    use neo_payloads::signer::Signer;
    use neo_payloads::witness::Witness;
    use neo_payloads::{Block, BlockHeader, OracleResponse};
    use neo_primitives::{OracleResponseCode, TriggerType, Verifiable, WitnessScope};
    use neo_vm::script_builder::ScriptBuilder;
    use neo_vm_rs::{OpCode, VmState};
    use std::sync::Arc;

    /// Builds a tiny deployed contract with one `method(params)` descriptor,
    /// so `ContractManagement.IsContract` passes and the queued `finish`
    /// callback can resolve a real method. Methods with parameters open with
    /// `INITSLOT` (as compiled contracts do) to consume the pushed arguments.
    fn mock_contract_state(hash: UInt160, method: &str, params: usize) -> ContractState {
        let script = if params > 0 {
            vec![
                OpCode::INITSLOT.byte(),
                0,
                u8::try_from(params).expect("param count"),
                OpCode::RET.byte(),
            ]
        } else {
            vec![OpCode::RET.byte()]
        };
        let nef = NefFile::new("test".to_string(), script);
        let parameters = (0..params)
            .map(|i| {
                ContractParameterDefinition::new(format!("arg{i}"), ContractParameterType::Any)
                    .expect("parameter")
            })
            .collect();
        let descriptor = ContractMethodDescriptor::new(
            method.to_string(),
            parameters,
            ContractParameterType::Void,
            0,
            false,
        )
        .expect("descriptor");
        let manifest = ContractManifest {
            name: "MockOracleClient".to_string(),
            groups: Vec::new(),
            features: std::collections::HashMap::new(),
            supported_standards: Vec::new(),
            abi: ContractAbi::new(vec![descriptor], Vec::new()),
            permissions: vec![ContractPermission::default_wildcard()],
            trusts: WildCardContainer::default(),
            extra: None,
        };
        ContractState::new(1, hash, nef, manifest)
    }

    /// Entry script: `OracleContract.request(url, filter, callback, userData, gas)`
    /// via System.Contract.Call (args pushed in reverse so arg0 is on top).
    fn request_script(
        url: &[u8],
        filter: Option<&[u8]>,
        callback: &[u8],
        user_data: i64,
        gas_for_response: i64,
    ) -> Vec<u8> {
        let mut builder = ScriptBuilder::new();
        builder.emit_push_int(gas_for_response); // arg4
        builder.emit_push_int(user_data); // arg3 (Any)
        builder.emit_push(callback); // arg2
        match filter {
            Some(f) => {
                builder.emit_push(f); // arg1
            }
            None => {
                builder.emit_opcode(OpCode::PUSHNULL); // arg1 = null
            }
        }
        builder.emit_push(url); // arg0
        builder.emit_push_int(5);
        builder.emit_pack();
        builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
        builder.emit_push(b"request");
        builder.emit_push(&OracleContract::script_hash().to_array());
        builder
            .emit_syscall("System.Contract.Call")
            .expect("System.Contract.Call");
        builder.to_array()
    }

    /// Entry script: `OracleContract.finish()` (zero args).
    fn finish_script() -> Vec<u8> {
        let mut builder = ScriptBuilder::new();
        builder.emit_push_int(0);
        builder.emit_pack();
        builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
        builder.emit_push(b"finish");
        builder.emit_push(&OracleContract::script_hash().to_array());
        builder
            .emit_syscall("System.Contract.Call")
            .expect("System.Contract.Call");
        builder.to_array()
    }

    fn signed_tx(signer: UInt160) -> Transaction {
        let mut tx = Transaction::new();
        tx.set_signers(vec![Signer::new(signer, WitnessScope::GLOBAL)]);
        tx.set_witnesses(vec![Witness::empty()]);
        tx
    }

    fn run(script: Vec<u8>, tx: Transaction, snapshot: Arc<DataCache>) -> (VmState, Vec<u8>) {
        crate::install();
        let container: Arc<dyn Verifiable> = Arc::new(tx);
        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            Some(container),
            snapshot,
            None,
            ProtocolSettings::default(),
            2000_00000000,
            None,
        )
        .expect("engine builds");
        engine
            .load_script(script, CallFlags::ALL, None)
            .expect("script loads");
        let state = engine.execute_allow_fault();
        let names: Vec<u8> = Vec::new();
        let _ = names;
        (state, Vec::new())
    }

    fn seed_initialized_oracle_storage(cache: &DataCache) {
        OracleContract::new().write_request_id(cache, &BigInt::from(0));
        cache.add(
            StorageKey::new(OracleContract::ID, vec![PREFIX_PRICE]),
            StorageItem::from_bytes(BigInt::from(DEFAULT_ORACLE_PRICE).to_signed_bytes_le()),
        );
    }

    /// Seeds a snapshot with the Oracle native contract record installed.
    fn oracle_snapshot() -> Arc<DataCache> {
        crate::install();
        let cache = DataCache::new(false);
        deploy_contract(
            &cache,
            &build_native_contract_state(&OracleContract, &ProtocolSettings::default(), 0),
        );
        seed_initialized_oracle_storage(&cache);
        Arc::new(cache)
    }

    #[test]
    fn request_writes_record_id_list_counter_and_mints_response_gas() {
        let snapshot = oracle_snapshot();
        let url = b"https://example.org/data";
        let script = request_script(url, Some(b"$.value"), b"cb", 42, 1_0000000);

        // The entry script itself must be a deployed contract for
        // ContractManagement.IsContract(CallingScriptHash) to pass.
        let caller_hash = UInt160::from_script(&script);
        deploy_contract(&snapshot, &mock_contract_state(caller_hash, "dummy", 0));

        let tx = signed_tx(UInt160::from_bytes(&[0x42; 20]).unwrap());
        let expected_txid = tx.hash();

        crate::install();
        let container: Arc<dyn Verifiable> = Arc::new(tx);
        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            Some(container),
            Arc::clone(&snapshot),
            None,
            ProtocolSettings::default(),
            2000_00000000,
            None,
        )
        .expect("engine builds");
        engine
            .load_script(script, CallFlags::ALL, None)
            .expect("script loads");
        assert_eq!(
            engine.execute_allow_fault(),
            VmState::HALT,
            "request must HALT"
        );

        // Request-id counter incremented to 1.
        assert_eq!(OracleContract::new().read_request_id(&snapshot).unwrap(), 1);

        // The stored OracleRequest record (id 0) matches the C# layout.
        let request = OracleContract::new()
            .read_request(&snapshot, 0)
            .unwrap()
            .expect("request stored");
        assert_eq!(request.original_tx_id, expected_txid);
        assert_eq!(request.gas_for_response, 1_0000000);
        assert_eq!(request.url, "https://example.org/data");
        assert_eq!(request.filter, Some("$.value".to_string()));
        assert_eq!(request.callback_contract, caller_hash);
        assert_eq!(request.callback_method, "cb");
        assert_eq!(
            request.user_data,
            BinarySerializer::serialize(
                &StackItem::from_int(BigInt::from(42)),
                &ExecutionEngineLimits::default()
            )
            .unwrap()
        );

        // The per-url id-list holds the new id.
        let list_item = snapshot
            .get(&OracleContract::id_list_key("https://example.org/data"))
            .expect("id list written");
        assert_eq!(
            OracleContract::decode_id_list(&list_item.value_bytes()).unwrap(),
            vec![0]
        );

        // gasForResponse was minted to the Oracle account (GAS Struct[balance]).
        let mut gas_key_bytes = vec![crate::NEP17_PREFIX_ACCOUNT];
        gas_key_bytes.extend_from_slice(&OracleContract::script_hash().to_bytes());
        let gas_item = snapshot
            .get(&StorageKey::new(crate::GasToken::ID, gas_key_bytes))
            .expect("oracle GAS account written");
        let decoded = BinarySerializer::deserialize(
            &gas_item.value_bytes(),
            &ExecutionEngineLimits::default(),
            None,
        )
        .unwrap();
        let StackItem::Struct(fields) = decoded else {
            panic!("GAS account state must be a struct");
        };
        assert_eq!(fields.items()[0].as_int().unwrap(), BigInt::from(1_0000000));

        // The OracleRequest notification carries [id, caller, url, filter].
        let event = engine
            .notifications()
            .iter()
            .find(|n| n.event_name == "OracleRequest")
            .expect("OracleRequest notification");
        assert_eq!(event.script_hash, OracleContract::script_hash());
        assert_eq!(event.state[0].as_int().unwrap(), BigInt::from(0));
        assert_eq!(event.state[1].as_bytes().unwrap(), caller_hash.to_bytes());
        assert_eq!(event.state[2].as_bytes().unwrap(), url.to_vec());
        assert_eq!(event.state[3].as_bytes().unwrap(), b"$.value".to_vec());
    }

    #[test]
    fn second_request_takes_the_next_id() {
        let snapshot = oracle_snapshot();
        let url = b"https://example.org/data";
        let script = request_script(url, None, b"cb", 1, 1_0000000);
        let caller_hash = UInt160::from_script(&script);
        deploy_contract(&snapshot, &mock_contract_state(caller_hash, "dummy", 0));

        for expected_counter in 1..=2u64 {
            let (state, _) = run(
                script.clone(),
                signed_tx(UInt160::from_bytes(&[0x42; 20]).unwrap()),
                Arc::clone(&snapshot),
            );
            assert_eq!(state, VmState::HALT);
            assert_eq!(
                OracleContract::new().read_request_id(&snapshot).unwrap(),
                expected_counter
            );
        }
        // Both ids are pending for the url, and a null filter round-trips.
        let list_item = snapshot
            .get(&OracleContract::id_list_key("https://example.org/data"))
            .unwrap();
        assert_eq!(
            OracleContract::decode_id_list(&list_item.value_bytes()).unwrap(),
            vec![0, 1]
        );
        assert_eq!(
            OracleContract::new()
                .read_request(&snapshot, 1)
                .unwrap()
                .unwrap()
                .filter,
            None
        );
    }

    #[test]
    fn request_validation_faults() {
        let long_url = vec![b'a'; MAX_URL_LENGTH + 1];
        let long_filter = vec![b'f'; MAX_FILTER_LENGTH + 1];
        let long_callback = vec![b'c'; MAX_CALLBACK_LENGTH + 1];
        let cases: Vec<(&str, Vec<u8>)> = vec![
            (
                "url too long",
                request_script(&long_url, None, b"cb", 1, 1_0000000),
            ),
            (
                "filter too long",
                request_script(b"https://x", Some(&long_filter), b"cb", 1, 1_0000000),
            ),
            (
                "callback too long",
                request_script(b"https://x", None, &long_callback, 1, 1_0000000),
            ),
            (
                "callback starts with underscore",
                request_script(b"https://x", None, b"_cb", 1, 1_0000000),
            ),
            (
                "gasForResponse below 0.1 GAS",
                request_script(b"https://x", None, b"cb", 1, 9999999),
            ),
        ];
        for (name, script) in cases {
            let snapshot = oracle_snapshot();
            let (state, _) = run(
                script,
                signed_tx(UInt160::from_bytes(&[0x42; 20]).unwrap()),
                Arc::clone(&snapshot),
            );
            assert_eq!(state, VmState::FAULT, "{name} must FAULT");
            assert_eq!(
                OracleContract::new().read_request_id(&snapshot).unwrap(),
                0,
                "{name}: no id allocated"
            );
        }
    }

    #[test]
    fn request_from_a_non_contract_caller_faults() {
        // No ContractManagement record for the entry script hash.
        let snapshot = oracle_snapshot();
        let script = request_script(b"https://x", None, b"cb", 1, 1_0000000);
        let (state, _) = run(
            script,
            signed_tx(UInt160::from_bytes(&[0x42; 20]).unwrap()),
            Arc::clone(&snapshot),
        );
        assert_eq!(state, VmState::FAULT);
        assert!(
            OracleContract::new()
                .read_request(&snapshot, 0)
                .unwrap()
                .is_none()
        );
    }

    fn seeded_finish_snapshot(id: u64) -> (Arc<DataCache>, OracleRequest, UInt160) {
        let snapshot = oracle_snapshot();
        let callback_hash = UInt160::from_bytes(&[0xCB; 20]).unwrap();
        deploy_contract(
            &snapshot,
            &mock_contract_state(callback_hash, "oracleCallback", 4),
        );
        let request = OracleRequest::new(
            UInt256::from_bytes(&[0xAA; 32]).unwrap(),
            1_0000000,
            "https://example.org/data",
            Some("$.value".to_string()),
            callback_hash,
            "oracleCallback",
            BinarySerializer::serialize(
                &StackItem::from_int(BigInt::from(42)),
                &ExecutionEngineLimits::default(),
            )
            .unwrap(),
        );
        snapshot.add(
            OracleContract::request_key(id),
            StorageItem::from_bytes(OracleContract::encode_oracle_request(&request).unwrap()),
        );
        snapshot.add(
            OracleContract::id_list_key(&request.url),
            StorageItem::from_bytes(OracleContract::encode_id_list(&[id]).unwrap()),
        );
        (snapshot, request, callback_hash)
    }

    fn oracle_response_tx(id: u64, result: &[u8]) -> Transaction {
        let mut tx = signed_tx(UInt160::from_bytes(&[0x42; 20]).unwrap());
        tx.add_attribute(TransactionAttribute::OracleResponse(OracleResponse::new(
            id,
            OracleResponseCode::Success,
            result.to_vec(),
        )));
        tx
    }

    #[test]
    fn finish_notifies_and_queues_the_callback() {
        let (snapshot, request, _) = seeded_finish_snapshot(7);
        let tx = oracle_response_tx(7, b"\"abc\"");

        crate::install();
        let container: Arc<dyn Verifiable> = Arc::new(tx);
        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            Some(container),
            Arc::clone(&snapshot),
            None,
            ProtocolSettings::default(),
            2000_00000000,
            None,
        )
        .expect("engine builds");
        engine
            .load_script(finish_script(), CallFlags::ALL, None)
            .expect("script loads");
        assert_eq!(
            engine.execute_allow_fault(),
            VmState::HALT,
            "finish must HALT"
        );

        // C# Finish emits OracleResponse [id, originalTxid] before the callback.
        let event = engine
            .notifications()
            .iter()
            .find(|n| n.event_name == "OracleResponse")
            .expect("OracleResponse notification");
        assert_eq!(event.script_hash, OracleContract::script_hash());
        assert_eq!(event.state[0].as_int().unwrap(), BigInt::from(7));
        assert_eq!(
            event.state[1].as_bytes().unwrap(),
            request.original_tx_id.to_bytes()
        );

        // C# Finish does NOT remove the request — PostPersist does.
        assert!(
            OracleContract::new()
                .read_request(&snapshot, 7)
                .unwrap()
                .is_some()
        );
        let list_item = snapshot
            .get(&OracleContract::id_list_key(&request.url))
            .unwrap();
        assert_eq!(
            OracleContract::decode_id_list(&list_item.value_bytes()).unwrap(),
            vec![7]
        );
    }

    #[test]
    fn finish_without_oracle_response_attribute_faults() {
        let (snapshot, _, _) = seeded_finish_snapshot(7);
        let (state, _) = run(
            finish_script(),
            signed_tx(UInt160::from_bytes(&[0x42; 20]).unwrap()),
            snapshot,
        );
        assert_eq!(state, VmState::FAULT);
    }

    #[test]
    fn finish_with_unknown_request_id_faults() {
        let (snapshot, _, _) = seeded_finish_snapshot(7);
        let (state, _) = run(finish_script(), oracle_response_tx(99, b""), snapshot);
        assert_eq!(state, VmState::FAULT);
    }

    #[test]
    fn verify_accepts_only_oracle_response_transactions() {
        crate::install();
        let make_engine = |tx: Transaction| {
            let container: Arc<dyn Verifiable> = Arc::new(tx);
            ApplicationEngine::new(
                TriggerType::Application,
                Some(container),
                Arc::new(DataCache::new(false)),
                None,
                ProtocolSettings::default(),
                10_00000000,
                None,
            )
            .expect("engine builds")
        };
        let contract = OracleContract::new();

        let mut with_attr = make_engine(oracle_response_tx(1, b""));
        assert_eq!(
            contract.invoke(&mut with_attr, "verify", &[]).unwrap(),
            vec![1]
        );

        let mut without_attr = make_engine(signed_tx(UInt160::from_bytes(&[0x42; 20]).unwrap()));
        assert_eq!(
            contract.invoke(&mut without_attr, "verify", &[]).unwrap(),
            vec![0]
        );
    }

    fn post_persist_engine(
        snapshot: Arc<DataCache>,
        block_index: u32,
        txs: Vec<Transaction>,
    ) -> ApplicationEngine {
        crate::install();
        let mut header = BlockHeader::default();
        header.set_index(block_index);
        ApplicationEngine::new(
            TriggerType::PostPersist,
            None,
            snapshot,
            Some(Block::from_parts(header, txs)),
            ProtocolSettings::default(),
            2000_00000000,
            None,
        )
        .expect("engine builds")
    }

    #[test]
    fn post_persist_removes_answered_requests_and_id_list_entries() {
        let (snapshot, request, _) = seeded_finish_snapshot(7);
        // A second pending request for the same url keeps the list alive.
        snapshot.add(
            OracleContract::request_key(8),
            StorageItem::from_bytes(OracleContract::encode_oracle_request(&request).unwrap()),
        );
        snapshot.update(
            OracleContract::id_list_key(&request.url),
            StorageItem::from_bytes(OracleContract::encode_id_list(&[7, 8]).unwrap()),
        );

        let mut engine =
            post_persist_engine(Arc::clone(&snapshot), 10, vec![oracle_response_tx(7, b"")]);
        NativeContract::post_persist(&OracleContract, &mut engine).expect("post_persist");

        assert!(
            OracleContract::new()
                .read_request(&snapshot, 7)
                .unwrap()
                .is_none(),
            "request removed"
        );
        assert!(
            OracleContract::new()
                .read_request(&snapshot, 8)
                .unwrap()
                .is_some(),
            "other request kept"
        );
        let list_item = snapshot
            .get(&OracleContract::id_list_key(&request.url))
            .expect("list kept");
        assert_eq!(
            OracleContract::decode_id_list(&list_item.value_bytes()).unwrap(),
            vec![8]
        );

        // Answering the last pending id deletes the list entry entirely.
        let mut engine =
            post_persist_engine(Arc::clone(&snapshot), 11, vec![oracle_response_tx(8, b"")]);
        NativeContract::post_persist(&OracleContract, &mut engine).expect("post_persist");
        assert!(
            OracleContract::new()
                .read_request(&snapshot, 8)
                .unwrap()
                .is_none()
        );
        assert!(
            snapshot
                .get(&OracleContract::id_list_key(&request.url))
                .is_none(),
            "empty list deleted"
        );

        // A response without a stored request is skipped (no fault).
        let mut engine =
            post_persist_engine(Arc::clone(&snapshot), 12, vec![oracle_response_tx(9, b"")]);
        NativeContract::post_persist(&OracleContract, &mut engine).expect("post_persist");
    }

    #[test]
    fn post_persist_mints_the_price_to_the_designated_oracle_node() {
        use neo_crypto::ECPoint;

        let (snapshot, _, _) = seeded_finish_snapshot(7);
        // Designate one oracle node at index 0 (RoleManagement layout:
        // (id, [role_byte, index_be4]) -> BinarySerialized Array[pubkey]).
        let pubkey = ECPoint::from_bytes(
            &hex::decode("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c")
                .unwrap(),
        )
        .unwrap();
        let mut role_key = vec![crate::Role::Oracle.as_byte()];
        role_key.extend_from_slice(&0u32.to_be_bytes());
        let nodes = BinarySerializer::serialize(
            &StackItem::from_array(vec![StackItem::from_byte_string(pubkey.to_bytes())]),
            &ExecutionEngineLimits::default(),
        )
        .unwrap();
        snapshot.add(
            StorageKey::new(crate::RoleManagement::ID, role_key),
            StorageItem::from_bytes(nodes),
        );

        let mut engine =
            post_persist_engine(Arc::clone(&snapshot), 10, vec![oracle_response_tx(7, b"")]);
        NativeContract::post_persist(&OracleContract, &mut engine).expect("post_persist");

        // The node received the default 0.5 GAS oracle price.
        let node_account = UInt160::from_script(&Contract::create_signature_redeem_script(pubkey));
        let mut gas_key_bytes = vec![crate::NEP17_PREFIX_ACCOUNT];
        gas_key_bytes.extend_from_slice(&node_account.to_bytes());
        let gas_item = snapshot
            .get(&StorageKey::new(crate::GasToken::ID, gas_key_bytes))
            .expect("node GAS account written");
        let decoded = BinarySerializer::deserialize(
            &gas_item.value_bytes(),
            &ExecutionEngineLimits::default(),
            None,
        )
        .unwrap();
        let StackItem::Struct(fields) = decoded else {
            panic!("GAS account state must be a struct");
        };
        assert_eq!(
            fields.items()[0].as_int().unwrap(),
            BigInt::from(DEFAULT_ORACLE_PRICE)
        );
    }
}
