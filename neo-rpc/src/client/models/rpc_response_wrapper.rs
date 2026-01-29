// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_response_wrapper.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

//! Generic RPC response wrapper
//!
//! This module provides a generic wrapper type for RPC responses that include
//! blockchain context (block hash, confirmations, etc.) along with the actual data.
//!
//! This replaces the need for separate `RpcTransaction`, `RpcBlock`, etc. types
//! with a single generic type.

use neo_primitives::UInt256;
use serde::{Deserialize, Serialize};

/// Generic RPC response with blockchain context.
///
/// This type wraps any blockchain data with additional context about
/// where and when it was confirmed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RpcResponse<T> {
    /// The actual data payload
    #[serde(flatten)]
    pub data: T,

    /// Block hash if the item is confirmed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_hash: Option<UInt256>,

    /// Number of confirmations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmations: Option<u32>,

    /// Block timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_time: Option<u64>,

    /// VM execution state as string (for transactions)
    /// Stored as string since `VMState` doesn't implement Serialize
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vm_state: Option<String>,
}

impl<T> RpcResponse<T> {
    /// Create a new unconfirmed response (just the data)
    pub const fn unconfirmed(data: T) -> Self {
        Self {
            data,
            block_hash: None,
            confirmations: None,
            block_time: None,
            vm_state: None,
        }
    }

    /// Create a confirmed response with full context
    pub const fn confirmed(data: T, block_hash: UInt256, confirmations: u32, block_time: u64) -> Self {
        Self {
            data,
            block_hash: Some(block_hash),
            confirmations: Some(confirmations),
            block_time: Some(block_time),
            vm_state: None,
        }
    }

    /// Set the VM state (for transactions)
    pub fn with_vm_state(mut self, state: impl Into<String>) -> Self {
        self.vm_state = Some(state.into());
        self
    }

    /// Check if the item is confirmed
    pub const fn is_confirmed(&self) -> bool {
        self.confirmations.is_some()
    }

    /// Get confirmations or 0 if unconfirmed
    pub fn confirmations(&self) -> u32 {
        self.confirmations.unwrap_or(0)
    }

    /// Map the inner data to a different type
    pub fn map<U, F>(self, f: F) -> RpcResponse<U>
    where
        F: FnOnce(T) -> U,
    {
        RpcResponse {
            data: f(self.data),
            block_hash: self.block_hash,
            confirmations: self.confirmations,
            block_time: self.block_time,
            vm_state: self.vm_state,
        }
    }

    /// Convert to the inner data, discarding context
    pub fn into_data(self) -> T {
        self.data
    }

    /// Get a reference to the inner data
    pub const fn data(&self) -> &T {
        &self.data
    }
}

impl<T> AsRef<T> for RpcResponse<T> {
    fn as_ref(&self) -> &T {
        &self.data
    }
}

impl<T> std::ops::Deref for RpcResponse<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

// Type aliases for common RPC responses can be added here as needed
// Example:
// pub type RpcTransaction = RpcResponse<neo_core::Transaction>;

#[cfg(test)]
mod tests {
    use super::*;
    use neo_primitives::UInt256;

    #[test]
    fn test_unconfirmed_response() {
        let data = "test_transaction".to_string();
        let response = RpcResponse::unconfirmed(data);

        assert!(!response.is_confirmed());
        assert_eq!(response.confirmations(), 0);
        assert!(response.block_hash.is_none());
    }

    #[test]
    fn test_confirmed_response() {
        let data = "test_transaction".to_string();
        let block_hash = UInt256::zero();
        let response = RpcResponse::confirmed(data, block_hash, 10, 1234567890);

        assert!(response.is_confirmed());
        assert_eq!(response.confirmations(), 10);
        assert_eq!(response.block_time, Some(1234567890));
    }

    #[test]
    fn test_with_vm_state() {
        let data = "test".to_string();
        let response = RpcResponse::unconfirmed(data).with_vm_state("HALT".to_string());

        assert_eq!(response.vm_state, Some("HALT".to_string()));
    }

    #[test]
    fn test_map() {
        let data = 42i32;
        let response = RpcResponse::unconfirmed(data);
        let mapped = response.map(|n| n.to_string());

        assert_eq!(mapped.data, "42");
    }

    #[test]
    fn test_deref() {
        let data = vec![1, 2, 3];
        let response = RpcResponse::unconfirmed(data);

        assert_eq!(response.len(), 3); // Uses Deref to access Vec methods
        assert_eq!(response[0], 1);
    }
}
