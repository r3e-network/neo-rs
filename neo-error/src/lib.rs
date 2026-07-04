//! # neo-error
//!
//! Shared error categories and conversions used by workspace crates.
//!
//! ## Boundary
//!
//! This foundation crate owns shared error vocabulary and must not depend on
//! higher-level services or binaries.
//!
//! ## Contents
//!
//! - `error`: Typed error definitions and conversions.

#![doc(html_root_url = "https://docs.rs/neo-error/0.10.0")]

#[path = "errors/error.rs"]
pub mod error;

pub use error::{CoreError, CoreResult, Result};

// Re-export the `impl_error_from!` macro from `neo-io` so consumers of
// `CoreError` (and `IoError`-adjacent error types) can use the same
// boilerplate-reducing helper without depending on `neo-io` macros
// directly. The macro itself still lives in `neo-io` (it is a generic
// helper, not specific to this crate).
#[doc(inline)]
pub use neo_io::impl_error_from;
