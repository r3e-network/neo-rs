//! Restricted-probe controller re-exports (assembly-layer shim).
//!
//! Phase 3 S6d2: the controller and its containment policy moved into
//! `neo-probe`, beside the detached worker wire protocol. These re-exports
//! freeze the paths consumed by the node diagnostics runtime.

#[cfg(test)]
pub use neo_probe::controller::ProbeUncertainReason;
pub use neo_probe::controller::{
    ProbeContextRechecker, ProbeController, ProbeControllerConfig, ProbeControllerResult,
    ProbeVerdict, probe_context_matches, validate_probe_startup,
};
