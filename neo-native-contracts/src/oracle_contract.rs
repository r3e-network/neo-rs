//! OracleContract native contract stub + supporting `OracleRequest` type.
//!
//! The stub provides the public surface used by the OracleService
//! plugin (`new`, `hash`, `ID`, `get_request`, `get_requests`,
//! `get_requests_by_url`). All storage queries return empty
//! results; a real executor should wire this up to a populated
//! native-contract cache.

use crate::hashes::ORACLE_CONTRACT_HASH;
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, NativeContract, NativeMethod};
use neo_primitives::{CallFlags, ContractParameterType, UInt160, UInt256};
use neo_storage::persistence::DataCache;
use neo_storage::{StorageItem, StorageKey};
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use std::any::Any;
use std::sync::LazyLock;

/// Storage prefix for the oracle request price (C# `OracleContract.Prefix_Price`).
const PREFIX_PRICE: u8 = 5;
/// C# default oracle price: 0.5 GAS, in datoshi.
const DEFAULT_ORACLE_PRICE: i64 = 50000000;

/// C# `SetPrice` storage effect: overwrite `Prefix_Price` as a `BigInteger`
/// (`GetAndChange(...).Set(price)`).
fn put_price(snapshot: &DataCache, price: i64) {
    snapshot.update(
        StorageKey::new(OracleContract::ID, vec![PREFIX_PRICE]),
        StorageItem::from_bytes(BigInt::from(price).to_signed_bytes_le()),
    );
}

/// Lazily-initialised script-hash handle for the OracleContract.
pub static ORACLE_HASH: LazyLock<UInt160> = LazyLock::new(|| *ORACLE_CONTRACT_HASH);

/// Static accessor for the OracleContract native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct OracleContract;

impl OracleContract {
    /// Stable native contract id (-9 in C# Oracle contract).
    pub const ID: i32 = -9;

    /// Construct a new `OracleContract` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the script hash of the Oracle native contract.
    pub fn hash(&self) -> UInt160 {
        *ORACLE_HASH
    }

    /// Returns the script hash of the Oracle native contract (static).
    pub fn script_hash() -> UInt160 {
        *ORACLE_HASH
    }

    /// Look up a single oracle request by its id.
    pub fn get_request(
        &self,
        _snapshot: &neo_storage::persistence::DataCache,
        _id: u64,
    ) -> neo_error::CoreResult<Option<OracleRequest>> {
        Ok(None)
    }

    /// Enumerate all pending oracle requests.
    pub fn get_requests(
        &self,
        _snapshot: &neo_storage::persistence::DataCache,
    ) -> Vec<(u64, OracleRequest)> {
        Vec::new()
    }

    /// Enumerate all pending oracle requests matching a URL.
    pub fn get_requests_by_url(
        &self,
        _snapshot: &neo_storage::persistence::DataCache,
        _url: &str,
    ) -> neo_error::CoreResult<Vec<(u64, OracleRequest)>> {
        Ok(Vec::new())
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
    /// User data (opaque payload).
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
}

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
        ),
    ]
});

impl NativeContract for OracleContract {
    fn id(&self) -> i32 {
        Self::ID
    }

    fn hash(&self) -> UInt160 {
        *ORACLE_HASH
    }

    fn name(&self) -> &str {
        "OracleContract"
    }

    fn methods(&self) -> &[NativeMethod] {
        &ORACLE_METHODS
    }

    fn as_any(&self) -> &dyn Any {
        self
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
                let price =
                    crate::read_storage_int(&snapshot, Self::ID, PREFIX_PRICE, DEFAULT_ORACLE_PRICE)?;
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
                let authorized = engine.check_committee_witness().map_err(|e| {
                    CoreError::invalid_operation(format!("setPrice committee check: {e}"))
                })?;
                if !authorized {
                    return Err(CoreError::invalid_operation(
                        "setPrice requires committee authorization",
                    ));
                }
                put_price(&engine.snapshot_cache(), price);
                Ok(Vec::new())
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
        assert_eq!(NativeContract::id(&c), -9);
        assert_eq!(NativeContract::name(&c), "OracleContract");
        assert_eq!(NativeContract::hash(&c), *ORACLE_CONTRACT_HASH);
        let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["getPrice", "setPrice"]);
        let setter = c.methods().iter().find(|m| m.name == "setPrice").unwrap();
        assert!(!setter.safe);
        assert_eq!(setter.required_call_flags, CallFlags::STATES.bits());
        assert_eq!(setter.parameters, vec![ContractParameterType::Integer]);
        assert_eq!(setter.return_type, ContractParameterType::Void);
    }

    #[test]
    fn set_price_write_round_trips() {
        let cache = DataCache::new(false);
        // The setter's storage effect (overwrite Prefix_Price) is observed by
        // the getter's reader.
        put_price(&cache, 7_5000000); // 0.75 GAS
        assert_eq!(
            crate::read_storage_int(&cache, OracleContract::ID, PREFIX_PRICE, DEFAULT_ORACLE_PRICE)
                .unwrap(),
            7_5000000
        );
    }

    #[test]
    fn price_reads_storage_with_default() {
        let cache = DataCache::new(false);
        assert_eq!(
            crate::read_storage_int(&cache, OracleContract::ID, PREFIX_PRICE, DEFAULT_ORACLE_PRICE)
                .unwrap(),
            DEFAULT_ORACLE_PRICE
        );
        let key = StorageKey::new(OracleContract::ID, vec![PREFIX_PRICE]);
        cache.add(key, StorageItem::from_bytes(BigInt::from(12345678).to_signed_bytes_le()));
        assert_eq!(
            crate::read_storage_int(&cache, OracleContract::ID, PREFIX_PRICE, DEFAULT_ORACLE_PRICE)
                .unwrap(),
            12345678
        );
    }
}
