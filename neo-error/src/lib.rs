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

#![doc(html_root_url = "https://docs.rs/neo-error/0.11.1")]

#[path = "errors/error.rs"]
pub mod error;

pub use error::{CoreError, CoreResult, Result};

// Re-export the `impl_error_from!` and `impl_error_from_struct!` macros from
// `neo-io` so consumers of `CoreError` (and `IoError`-adjacent error types) can
// use the same boilerplate-reducing helpers without depending on `neo-io`
// macros directly. The macros themselves still live in `neo-io` (they are
// generic helpers, not specific to this crate).
#[doc(inline)]
pub use neo_io::impl_error_from;
#[doc(inline)]
pub use neo_io::impl_error_from_struct;
