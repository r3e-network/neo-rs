//! Durable action-store re-export (assembly-layer shim).
//!
//! Phase 3 S6d1: the durable append-only action store sank into `neo-ai-agent`
//! (it only depends on `OperationId`/`OperationalAction`, both already in that
//! crate — zero new dependencies, 0-neo red line preserved). This shim freezes
//! the historical `crate::action_store::*` paths used by `ai_agent` and
//! `probe_controller`.

pub use neo_ai_agent::{ActionBinding, ActionState, AdmitOutcome, DurableActionStore};
