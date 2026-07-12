//! # Node Capabilities
//!
//! This module defines [`NodeTypes`] (protocol primitives) and the active
//! provider-trait decoupling layer ([`StoreProvider`], [`ConfigProvider`],
//! [`TxAdmission`]) that lets L6 crates depend on L3 traits instead of the
//! concrete `neo_system::Node`.
//!
//! ## Boundary
//!
//! These are dependency-inversion contracts only. Concrete node composition
//! and application lifecycle remain in `neo-system` and `neo-node`.
//!
//! ## Contents
//!
//! - `types`: node type bundle and provider capability traits.

mod types;

pub use types::{ConfigProvider, NeoNodeTypes, NodeTypes, StoreProvider, TxAdmission};
