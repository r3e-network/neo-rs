//! Blockchain command vocabulary and typed actor handle.
//!
//! The blockchain actor implementation (block import, validation, persistence
//! and relay orchestration) moved to `neo-system::blockchain` (task W3-C2)
//! together with the `NeoSystem` runtime that drives it. This module keeps the
//! wire-level vocabulary — [`BlockchainCommand`] and its payload types — plus
//! the [`BlockchainHandle`] facade, so protocol-level consumers (consensus,
//! RPC, tests) can keep issuing commands without depending on the runtime
//! crate.
//!
//! System attachment is no longer part of [`BlockchainCommand`]: it travels as
//! `neo_system::blockchain::BlockchainSystemCommand`, a distinct envelope the
//! actor downcasts after this one.

mod handle;
pub mod types;

pub use handle::BlockchainHandle;
pub use types::{
    BlockchainCommand, FillCompleted, FillMemoryPool, Import, ImportCompleted, InventoryPayload,
    PersistCompleted, PersistFailed, PreverifyCompleted, RelayResult, Reverify, ReverifyItem,
};

// Names re-exported through `super::*` for `types.rs` (kept private: they are
// implementation details of the vocabulary above).
use crate::UInt256;
use crate::ledger::VerifyResult;
use crate::network::p2p::payloads::{
    InventoryType, Transaction, block::Block, extensible_payload::ExtensiblePayload, header::Header,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
