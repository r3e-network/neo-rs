//! Ledger event handler types.
//!
//! # Superseded handlers
//!
//! The legacy `CommittingHandler` and `CommittedHandler` traits from
//! `neo-core::events::handlers` are **superseded** by the canonical
//! [`CommitParticipant`](crate::persistence::participant::CommitParticipant)
//! trait in this crate. `CommitParticipant` unifies both handler roles:
//!
//! | participant phase | legacy handler |
//! | --- | --- |
//! | `precommit` (step 12) | `CommittingHandler::blockchain_committing_handler` |
//! | `committed` (step 15a) | `CommittedHandler::blockchain_committed_handler` |
//!
//! No separate `CommittingHandler` / `CommittedHandler` definitions are needed
//! in this crate — consumers should implement `CommitParticipant` instead.

pub use crate::persistence::participant::CommitParticipant;
