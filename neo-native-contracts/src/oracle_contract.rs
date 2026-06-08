//! OracleContract native contract stub + supporting `OracleRequest` type.
//!
//! The stub provides the public surface used by the OracleService
//! plugin (`new`, `hash`, `ID`, `get_request`, `get_requests`,
//! `get_requests_by_url`). All storage queries return empty
//! results; a real executor should wire this up to a populated
//! native-contract cache.

use crate::hashes::ORACLE_CONTRACT_HASH;
use neo_primitives::{UInt160, UInt256};
use std::sync::LazyLock;

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
