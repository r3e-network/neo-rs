//! # NeoVM Collections
//!
//! Small collection and encoding primitives shared by VM metadata and runtime
//! value implementations.
//!
//! ## Boundary
//!
//! This module owns deterministic, VM-specific helpers without depending on
//! execution-engine state or mutable `StackItem` graphs.
//!
//! ## Contents
//!
//! Minimal integer encoding and an insertion-ordered dictionary.

mod integer;
mod ordered_dictionary;

pub use integer::encode_integer;
pub use ordered_dictionary::VmOrderedDictionary;
