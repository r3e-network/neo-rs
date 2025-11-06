#![cfg_attr(not(feature = "std"), no_std)]

//! Consensus primitives backing the Neo N3 Rust node.
//!
//! The crate focuses on the dBFT message flow, validator handling, and quorum
//! tracking. Networking and persistence are implemented in higher layers.
//! `neo-base` provides deterministic encoding while `neo-crypto` supplies the
//! signing helpers used to authenticate messages.

extern crate alloc;

mod dbft;
mod error;
mod message;
#[cfg(feature = "store")]
mod persistence;
mod state;
mod validator;

pub use dbft::{DbftEngine, ReplayResult};
pub use error::ConsensusError;
pub use message::{ChangeViewReason, ConsensusMessage, MessageKind, SignedMessage, ViewNumber};
#[cfg(feature = "store")]
pub use persistence::{
    clear_snapshot, load_engine, persist_engine, ConsensusColumn, PersistenceError, SnapshotKey,
};
pub use state::{ConsensusState, QuorumDecision, SnapshotState};
pub use validator::{Validator, ValidatorId, ValidatorSet};
