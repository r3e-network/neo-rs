//! # neo-consensus::context::lifecycle
//!
//! dBFT context construction, round transitions, liveness, and timing.
//!
//! ## Boundary
//!
//! This module mutates only the in-memory consensus context. Message handling,
//! persistence, and service scheduling remain outside this lifecycle group.
//!
//! ## Contents
//!
//! - `construction`: fresh context defaults.
//! - `liveness`: failed-validator and view-change guards.
//! - `round`: view and block reset transitions.
//! - `timer`: round timeout and primary-delay calculations.

use super::{
    ConsensusContext, ConsensusState, DEFAULT_BLOCK_TIME_MS, DEFAULT_MAX_BLOCK_SIZE,
    DEFAULT_MAX_BLOCK_SYSTEM_FEE, ValidatorInfo,
};

mod construction;
mod liveness;
mod round;
mod timer;
