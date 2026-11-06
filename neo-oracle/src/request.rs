//! Oracle request data type.
//!
//! This is a local definition matching the on-chain `OracleRequest` structure
//! stored by the Oracle native contract. It avoids depending on neo-core's
//! native contract module.

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
