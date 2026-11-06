//! Native-side re-export of engine-owned metadata activation.
//!
//! Migrated from `neo-core/src/smart_contract/native/hardfork_activable.rs`.
//! The trait itself is owned by the engine crate; native contracts implement it
//! to declare hardfork activation windows for their metadata.

pub use neo_smart_contract::application_engine::HardforkActivable;
