//! Probe containment re-export (assembly-layer shim).
//!
//! Phase 3 S6d2: the fail-closed platform capability detector moved into
//! `neo-probe::containment` beside the controller it admits.

pub use neo_probe::containment::NativeProbeContainment;
