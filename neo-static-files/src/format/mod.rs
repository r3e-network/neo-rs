//! # Static-file format
//!
//! ## Boundary
//!
//! This module defines deterministic archive headers, frame indexes,
//! compression envelopes, and checksums. Persistent lookup metadata is owned
//! by `archive::index` and is not part of the authoritative frame format.
//!
//! ## Contents
//!
//! - `codec`: Versioned encode, decode, limit, and integrity operations.

mod codec;

pub(crate) use codec::*;
