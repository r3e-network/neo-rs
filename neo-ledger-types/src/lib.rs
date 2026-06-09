//! # neo-ledger-types
//!
//! Pure ledger and wire data types for the Neo blockchain.
//!
//! This crate is the canonical home for the low-level [`Witness`] type —
//! the signature / verification-script pair attached to a verifiable
//! payload. The richer block-level wire types (`Block`, `Header`,
//! `Transaction`, `Signer`, `HeadersPayload`, transaction attributes and
//! the protocol enums) live in `neo-payloads`, with the P2P message
//! payloads in `neo-p2p`; both depend on this crate for `Witness`.
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
//! - [`Witness`] — the signature / verification-script pair attached to a
//!   verifiable payload, plus its serialization and script-hash helpers.
//!
//! ## What does **not** belong here
//!
//! - The block-level data types (`Block`, `Header`, `Transaction`,
//!   `Signer`, `HeadersPayload`, transaction attributes) — those live in
//!   `neo-payloads`, which depends on this crate for `Witness`.
//! - The P2P message payloads and witness-rule conditions — `neo-p2p`.
//! - Anything that needs `DataCache` / `ProtocolSettings` (stateful
//!   verification) or the application engine / native contracts — the
//!   `neo-payloads` verification helpers and `neo-execution`.

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

pub mod witness;

pub use witness::Witness;
