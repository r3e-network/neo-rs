//! # Finalized Block Delivery
//!
//! The canonical writer owns publication; one statically selected consumer
//! owns projection work. Delivery is acknowledged before the writer advances,
//! so queue memory is bounded and a consumer cannot silently miss a block.
//!
//! ## Boundary
//!
//! This module owns the generic channel, acknowledgement, and consumer
//! capability. It does not select node plugins or define canonical persistence.
//!
//! ## Contents
//!
//! - `consumer`: statically dispatched finalized-block capability.
//! - `stream`: bounded producer, worker, acknowledgement, and factory types.

mod consumer;
mod stream;

pub use consumer::FinalizedBlockConsumer;
pub use stream::{
    DEFAULT_FINALITY_CAPACITY, FinalizedBlockHandle, FinalizedBlockStream,
    FinalizedBlockStreamError, FinalizedBlockStreamFactory,
};
