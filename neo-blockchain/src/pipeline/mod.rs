//! # neo-blockchain::pipeline
//!
//! Ordered validation, execution, native-hook, and persistence steps for block
//! import.
//!
//! ## Boundary
//!
//! This module belongs to `neo-blockchain`. This node-service crate owns the
//! concrete block-import path and must not depend upward on composition, RPC,
//! GUI, or binaries.
//!
//! ## Contents
//!
//! - `block_processing`: block execution and persistence workflow.
//! - `block_validation`: block validation workflow.
//! - `consensus_witness_stage`: consensus header witness verification stage.
//! - `empty_block_fast_forward`: guarded empty-block fast-forward eligibility.
//! - `handlers`: service message handlers.
//! - `native_persist`: native-contract persistence hooks.
//! - `stage_traits`: pipeline stage trait definitions (ADR-027, moved from neo-engine).
//! - `validate_stage`: concrete `ValidateStage` impl (ADR-010 Phase 1).
//! - `verified_import_pipeline`: high-level verified import stage chain.

pub mod block_processing;
pub mod block_validation;
pub mod consensus_witness_stage;
pub mod empty_block_fast_forward;
pub mod handlers;
pub mod native_persist;
pub mod stage_traits;
pub mod validate_stage;
pub mod verified_import_pipeline;

pub use stage_traits::{
    ConsensusWitnessStage, EngineError, EngineResult, PipelineStage, StageContext, StageId,
    StageOutput, ValidateStage,
};
