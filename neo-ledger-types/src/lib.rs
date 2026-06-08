//! # neo-ledger-types
//!
//! Pure ledger and wire data types for the Neo blockchain.
//!
//! This crate is the **single** canonical home for protocol-layer data
//! types: `Witness`, `Header`, `HeadersPayload`, and (in later slices)
//! `Block`, `Transaction`, `Signer`, `WitnessRule`, transaction
//! attributes, and the protocol enums that go with them
//! (`InventoryType`, `TransactionAttributeType`, `OracleResponseCode`,
//! `TransactionRemovalReason`, `VerifyResult`, `WitnessConditionType`,
//! `WitnessRuleAction`, etc.).
//!
//! ## Layering
//!
//! Sits in **Layer 1 (protocol)**. May depend on `neo-crypto`,
//! `neo-error`, `neo-primitives`, `neo-io`, `neo-vm-rs` (the last for
//! opcode metadata only), and `neo-redeem-script` (for verifying the
//! standard signature / multi-sig verification scripts that go into a
//! `Witness`).
//!
//! Must **not** depend on any Layer 2+ crate: no persistence, no chain
//! orchestrator, no state service, no native contracts, no smart
//! contract engine, no network. This is the same rule polkadot-sdk and
//! reth apply to their `*-primitives` / `*-types` crates — keep
//! structured wire types independent of the runtime that consumes them
//! so every other layer can depend on them without inverting the
//! dependency graph.
//!
//! ## What belongs here
//!
//! - `Witness` — signature / verification-script pair attached to a
//!   verifiable payload.
//! - `Header` / `HeadersPayload` — block header and the response
//!   payload to `GetHeaders` messages.
//! - (future) `Block`, `Transaction` — block-level data.
//! - (future) `Signer` — per-transaction signer with scope rules.
//! - (future) `WitnessRule` / `WitnessCondition` — conditional
//!   verification rules (moved here from `neo-p2p`).
//! - (future) protocol enums (`InventoryType`, `VerifyResult`, ...).
//!
//! ## What does **not** belong here
//!
//! - Anything that needs `DataCache` / `ProtocolSettings` (stateful
//!   verification) — that lives in `neo-core` as a `<Type>Ext` trait.
//! - Anything that needs the application engine or native contracts —
//!   that lives in `neo-execution`.

#![doc(html_root_url = "https://docs.rs/neo-ledger-types/0.7.2")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

// Re-export the generic macros from `neo-io` so types defined in this
// crate (e.g. `Witness`) can use the same `impl_default_via_new!` /
// `impl_error_from!` helpers without depending on `neo-io` macros
// directly. The macros themselves are generic, not specific to this
// crate.
#[doc(inline)]
pub use neo_io::{
    impl_default_via_new, impl_error_from, impl_from_bytes, impl_hash_for_fields,
    impl_ord_by_fields,
};

pub mod header;
pub mod headers_payload;
pub mod witness;

pub use header::Header;
pub use headers_payload::{HeadersPayload, MAX_HEADERS_COUNT};
pub use witness::Witness;
