//! # Static-file errors
//!
//! ## Boundary
//!
//! Errors distinguish archive I/O and validation from derived MDBX index
//! failures so callers can diagnose the failed durability surface.
//!
//! ## Contents
//!
//! - `kind`: Public result alias and typed error variants.

mod kind;

pub use kind::{StaticFileError, StaticFileResult};
