//! The protocol-level oracle request record.
//!
//! This is the single definition of the on-chain `OracleRequest` stored by the
//! oracle native contract (C# `Neo.SmartContract.Native.OracleRequest`). The
//! oracle native contract and the oracle service engine both depend on
//! `neo-protocol`, so the record is shared from here instead of being
//! redefined per consumer.
//!
//! Native-contract specific concerns stay at the boundary: the contract's
//! stack projection lives in `neo-native-contracts` and the storage codec
//! lives with the contract's storage code. Only the data shape is shared.

use neo_primitives::{UInt160, UInt256};

/// Oracle request state (matches C# `Neo.SmartContract.Native.OracleRequest`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OracleRequest {
    /// The original transaction hash that triggered this oracle request.
    pub original_tx_id: UInt256,
    /// The gas budget reserved for the oracle response callback.
    pub gas_for_response: i64,
    /// The URL to fetch.
    pub url: String,
    /// Optional JSONPath filter expression applied to the response.
    pub filter: Option<String>,
    /// The callback contract hash.
    pub callback_contract: UInt160,
    /// The callback method name.
    pub callback_method: String,
    /// User data to pass to the callback.
    pub user_data: Vec<u8>,
}

impl OracleRequest {
    /// Creates a new oracle request.
    #[must_use]
    pub fn new(
        original_tx_id: UInt256,
        gas_for_response: i64,
        url: String,
        filter: Option<String>,
        callback_contract: UInt160,
        callback_method: String,
        user_data: Vec<u8>,
    ) -> Self {
        Self {
            original_tx_id,
            gas_for_response,
            url,
            filter,
            callback_contract,
            callback_method,
            user_data,
        }
    }
}
