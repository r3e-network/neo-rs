//! # neo-system
//!
//! Composition root for building node services, wallets, and runtime
//! dependencies.
//!
//! ## Boundary
//!
//! This composition crate wires services and must not hide protocol rules or
//! duplicate lower-layer business logic.
//!
//! ## Contents
//!
//! - `composition`: Composition-root builders, registries, and node assembly
//!   helpers.
//! - `errors`: Typed errors and result aliases for this crate boundary.

#![doc(html_root_url = "https://docs.rs/neo-system/0.10.0")]

mod composition;
mod errors;

// Public re-exports for the crate's public surface.
pub use composition::{Node, NodeBuilder, ServiceRegistry, WalletProvider};
pub use composition::{builder, node, wallet_provider};
pub use errors::{NodeError, NodeResult, error};
