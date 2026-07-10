//! # neo-node::node::ledger_source
//!
//! Local and remote ledger source providers used by node modes.
//!
//! ## Boundary
//!
//! This module belongs to `neo-node`. This application crate may compose lower
//! layers but must not define protocol bytes, storage formats, consensus rules,
//! or VM semantics.
//!
//! ## Contents
//!
//! - `local`: store-backed [`neo_network::BlockSource`] implementation for
//!   normal nodes with a local ledger.
//! - `remote`: JSON-RPC-backed [`neo_network::BlockSource`] implementation for
//!   remote-ledger mode.
//! - `tip`: local persisted-ledger tip helpers used by startup, validation,
//!   and import resume paths.

mod local;
mod remote;
mod tip;

pub(super) use local::LedgerBlockSource;
pub(super) use remote::RpcLedgerBlockSource;
pub(in crate::node) use tip::store_ledger_index;
pub(in crate::node) use tip::{LocalLedgerTip, local_ledger_tip};
