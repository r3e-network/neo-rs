//! OracleContract protocol constants and event names.
//!
//! Centralizes request limits, storage prefixes, default pricing, and event
//! names so the contract root stays focused on the native-contract surface.

/// C# `OracleContract.MaxUrlLength` (strict-UTF8 bytes).
pub(in crate::oracle_contract) const MAX_URL_LENGTH: usize = 256;
/// C# `OracleContract.MaxFilterLength` (strict-UTF8 bytes).
pub(in crate::oracle_contract) const MAX_FILTER_LENGTH: usize = 128;
/// C# `OracleContract.MaxCallbackLength` (strict-UTF8 bytes).
pub(in crate::oracle_contract) const MAX_CALLBACK_LENGTH: usize = 32;
/// C# `OracleContract.MaxUserDataLength` (serialized bytes).
pub(in crate::oracle_contract) const MAX_USER_DATA_LENGTH: usize = 512;

/// Storage prefix for the oracle request price (C# `OracleContract.Prefix_Price`).
pub(in crate::oracle_contract) const PREFIX_PRICE: u8 = 5;
/// Storage prefix for the per-url request-id list (C# `Prefix_IdList`).
pub(in crate::oracle_contract) const PREFIX_ID_LIST: u8 = 6;
/// Storage prefix for the pending request records (C# `Prefix_Request`).
pub(in crate::oracle_contract) const PREFIX_REQUEST: u8 = 7;
/// Storage prefix for the next-request-id counter (C# `Prefix_RequestId`).
pub(in crate::oracle_contract) const PREFIX_REQUEST_ID: u8 = 9;

/// C# default oracle price: 0.5 GAS, in datoshi (genesis `InitializeAsync` value).
pub(in crate::oracle_contract) const DEFAULT_ORACLE_PRICE: i64 = 50000000;
/// C# `Request`: `gasForResponse` must be at least 0.1 GAS (`0_10000000` datoshi).
pub(in crate::oracle_contract) const MIN_GAS_FOR_RESPONSE: i64 = 10000000;
/// C# `Request`: at most 256 pending responses per url.
pub(in crate::oracle_contract) const MAX_PENDING_IDS_PER_URL: usize = 256;

pub(crate) const ORACLE_REQUEST_EVENT: &str = "OracleRequest";
pub(crate) const ORACLE_RESPONSE_EVENT: &str = "OracleResponse";
