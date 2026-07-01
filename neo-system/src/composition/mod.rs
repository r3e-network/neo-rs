//! # neo-system::composition
//!
//! Composition-root builders, registries, and node assembly helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-system`. This composition crate wires services
//! and must not hide protocol rules or duplicate lower-layer business logic.
//!
//! ## Contents
//!
//! - `builder`: RPC client builder.
//! - `node`: Daemon composition, CLI modes, and long-running node startup.
//! - `service_registry`: Service registry and lookup helpers.
//! - `wallet_provider`: wallet provider adapter.

pub mod builder;
pub mod node;
pub mod service_registry;
pub mod wallet_provider;

pub use builder::NodeBuilder;
pub use node::Node;
pub use service_registry::ServiceRegistry;
pub use wallet_provider::WalletProvider;
