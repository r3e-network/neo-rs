mod codec;
mod eval;
mod types;

pub use types::{WitnessCondition, WitnessConditionError, WitnessConditionType};

pub(super) use super::WitnessConditionContext;

#[cfg(test)]
mod tests;
