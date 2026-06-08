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
//! Sits in Layer 0 (foundation). Depends only on `neo-primitives` (for
//! `PrimitiveError`) and `thiserror`. No other `neo-*` dependency.
//!
//! ## Why a foundation crate
//!
//! A foundation crate is the right home for the error type because every
//! layer above it needs to talk about errors, and pulling an error enum
//! from a service-layer crate (e.g. `neo-chain` or `neo-core`) into a
//! primitive crate (e.g. `neo-primitives` or `neo-io`) would invert the
//! dependency order. Putting `CoreError` in its own `neo-error` crate
//! keeps the layering clean and matches the polkadot-sdk and reth
//! convention of a top-level `errors` / `error` crate.
//!
//! ## Cross-crate `From` impls
//!
//! This crate is the *only* place that may implement `From<X> for CoreError`
//! for an external type `X`. Add new impls here when you need to lift a
//! lower-layer error into `CoreError`; lower-layer crates must not depend
//! on `neo-error` and so cannot add their own.

#![doc(html_root_url = "https://docs.rs/neo-error/0.7.2")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod error;

pub use error::{CoreError, CoreResult, Result, ToNativeError};

// Re-export the `impl_error_from!` macro from `neo-io` so consumers of
// `CoreError` (and `IoError`-adjacent error types) can use the same
// boilerplate-reducing helper without depending on `neo-io` macros
// directly. The macro itself still lives in `neo-io` (it is a generic
// helper, not specific to this crate).
#[doc(inline)]
pub use neo_io::impl_error_from;
