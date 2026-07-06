//! Empty-block fast-forward eligibility and staging.
//!
//! This module is the public facade for the state-equivalent empty-block fast
//! path. Eligibility planning stays separate from the staged writer so the
//! safety gate remains easy to audit: a run is eligible only during trusted
//! bulk sync, with replay artifacts disabled, with contiguous zero-merkle
//! blocks, outside native initialization/hardfork heights, and after every
//! active native contract explicitly opts in.

mod planner;
mod stage;
mod types;

pub use planner::{
    EmptyBlockFastForwardRejection, EmptyBlockFastForwardRequest, plan_empty_block_fast_forward,
};
pub use stage::{StagedEmptyBlockFastForward, stage_empty_block_fast_forward};
pub use types::{EmptyBlockFastForwardPlan, MAX_EMPTY_BLOCK_FAST_FORWARD_BLOCKS};

#[cfg(test)]
#[path = "../tests/pipeline/empty_block_fast_forward.rs"]
mod tests;
