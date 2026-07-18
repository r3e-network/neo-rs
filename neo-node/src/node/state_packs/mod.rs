//! # Authoritative StateService node packs
//!
//! Opt-in composition for the physically separated StateService MPT node
//! namespace.
//!
//! ## Boundary
//!
//! This module binds the generic pack engine to node startup, coordinated MDBX
//! publication, and pinned StateService reads. The on-disk format remains
//! owned by `neo-state-packs`.
//!
//! ## Contents
//!
//! - `authority`: startup reconciliation, cold-first commit, and publication.

mod authority;

pub(in crate::node) use authority::AuthoritativeNodePack;
