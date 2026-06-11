//! # neo-state-types
//!
//! Pure data types and key constants for the Neo State Service plugin.
//!
//! Mirrors `Neo.Plugins.StateService` for the wire-protocol surface
//! (storage keys, message types, vote payloads, ingestion metrics).
//! The stateful state root / state store / verification / commit
//! logic stays in `neo-core::state_service` because it needs the
//! runtime (`DataCache`, `StoreCache`, `ApplicationEngine`, native
//! contracts, actor handles).
//!
//! ## Layering
//!
//! Sits in **Layer 1 (protocol)**. Depends only on:
//! - `neo-primitives` (Layer 0) — for `protocol_enum!` and `Serializable` helpers.
//! - `neo-io` (Layer 0) — for `impl_serializable!` and the `Serializable` trait.
//!
//! Must **not** depend on `neo-core` (Layer 2 runtime), `neo-storage`
//! (state caches), `neo-smart-contract-types` / `neo-execution` (Layer 1
//! native contracts), or any Layer 2+ crate. This matches the rule
//! polkadot-sdk and reth apply to their `*-types` crates: keep the
//! wire-protocol surface independent of the runtime that consumes it.
//!
//! ## What belongs here
//!
//! - [`Keys`] — storage key prefixes (state root, current local root, current validated root).
//! - [`MessageType`] — extensible-payload message type marker (Vote / StateRoot).
//! - [`Vote`] — validator signature over a state root hash.
//! - [`StateRootIngestStats`] + [`record_ingest_result`] /
//!   [`state_root_ingest_stats`] — lightweight ingestion counters.
//!
//! ## What does **not** belong here
//!
//! - State root / state store types (live in `neo-core::state_service` because they
//!   need the storage layer and the smart-contract engine).
//! - Verification / commit / actor logic (lives in `neo-core::state_service`).

#![doc(html_root_url = "https://docs.rs/neo-state-types/0.7.2")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod keys;
pub mod message_type;
pub mod metrics;
pub mod vote;

/// Extensible payload category for state service messages
/// (matches C# `StateService.StatePayloadCategory`).
pub const STATE_SERVICE_CATEGORY: &str = "StateService";

pub use keys::Keys;
pub use message_type::MessageType;
pub use metrics::{record_ingest_result, state_root_ingest_stats, StateRootIngestStats};
pub use vote::Vote;
