//! # neo-error
//!
//! Authoritative error type for the Neo workspace.
//!
//! This is the **single** crate that owns `CoreError` and `CoreResult`. Every
//! other `neo-*` crate (Layer ≥ 1) returns or accepts `CoreError` at its
//! public API boundary, so the workspace has exactly one error vocabulary.
//!
//! ## Layering
//!
//! Sits in the infrastructure layer, directly above `neo-primitives` and
//! `neo-io`. It depends on `neo-primitives` for `PrimitiveError` conversion and
//! on `neo-io` for `IoError` conversion, but must not depend on storage,
//! execution, networking, RPC, or node-composition crates.
//!
//! ## Why a low-level error crate
//!
//! A low infrastructure crate is the right home for the error type because
//! every higher layer needs to talk about errors, and pulling an error enum from
//! a service-layer crate (for example the blockchain service) into
//! infrastructure crates would invert the dependency order. Putting
//! `CoreError` in its own `neo-error` crate keeps the layering clean and
//! matches the polkadot-sdk and reth convention of a top-level `errors` /
//! `error` crate.
//!
//! ## Cross-crate `From` impls
//!
//! This crate is the *only* place that may implement `From<X> for CoreError`
//! for a lower-layer or external type `X`. Add new impls here when you need to
//! lift a lower-layer error into `CoreError`; higher-layer crates must implement
//! their local conversions in their own crate so `neo-error` does not grow
//! upward dependencies.

#![doc(html_root_url = "https://docs.rs/neo-error/0.8.0")]

pub mod error;

pub use error::{CoreError, CoreResult, Result};

// Re-export the `impl_error_from!` macro from `neo-io` so consumers of
// `CoreError` (and `IoError`-adjacent error types) can use the same
// boilerplate-reducing helper without depending on `neo-io` macros
// directly. The macro itself still lives in `neo-io` (it is a generic
// helper, not specific to this crate).
#[doc(inline)]
pub use neo_io::impl_error_from;
