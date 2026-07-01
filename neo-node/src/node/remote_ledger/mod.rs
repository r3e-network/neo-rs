//! # neo-node::node::remote_ledger
//!
//! RPC-backed ledger source used when the node runs without a local ledger.
//!
//! ## Boundary
//!
//! This module belongs to `neo-node`. This application crate may compose lower
//! layers but must not define protocol bytes, storage formats, consensus rules,
//! or VM semantics.
//!
//! ## Contents
//!
//! - `remote_ledger`: remote-ledger status records for RPC-only mode.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RemoteLedgerStatus {
    pub(super) endpoint: String,
    pub(super) advertised_height: Option<u32>,
    pub(super) tip_error: Option<String>,
}

impl RemoteLedgerStatus {
    pub(super) fn new(endpoint: impl Into<String>, advertised_height: Option<u32>) -> Self {
        Self {
            endpoint: endpoint.into(),
            advertised_height,
            tip_error: None,
        }
    }

    pub(super) fn unavailable(endpoint: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            advertised_height: None,
            tip_error: Some(error.into()),
        }
    }
}
