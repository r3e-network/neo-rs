//! # neo-consensus::messages
//!
//! Typed service commands, events, and payload wrappers for the crate boundary.
//!
//! ## Boundary
//!
//! This module belongs to `neo-consensus`. This protocol/service crate owns
//! dBFT state and messages and must not own ledger persistence, RPC transport,
//! or application startup.
//!
//! ## Contents
//!
//! - `change_view`: dBFT ChangeView message records.
//! - `commit`: dBFT Commit message records.
//! - `payload`: Common dBFT extensible-payload envelope helpers.
//! - `prepare_request`: dBFT PrepareRequest message records.
//! - `prepare_response`: dBFT PrepareResponse message records.
//! - `recovery`: dBFT recovery request and response messages.
//! - `tests`: Module-local tests and regression coverage.

mod change_view;
mod commit;
mod payload;
mod prepare_request;
mod prepare_response;
mod recovery;

pub use change_view::ChangeViewMessage;
pub use commit::CommitMessage;
pub use payload::ConsensusPayload;
pub use prepare_request::PrepareRequestMessage;
pub use prepare_response::PrepareResponseMessage;
pub use recovery::{
    ChangeViewPayloadCompact, CommitPayloadCompact, PreparationPayloadCompact, RecoveryMessage,
    RecoveryRequestMessage,
};

pub(crate) use payload::consensus_message_bytes;

#[cfg(test)]
#[path = "../tests/messages/mod.rs"]
mod tests;
