//! Node type hierarchy and provider traits.
//!
//! This module defines [`NodeTypes`] (protocol primitives) and the active
//! provider-trait decoupling layer ([`StoreProvider`], [`ConfigProvider`],
//! [`TxAdmission`]) that lets L6 crates depend on L3 traits instead of the
//! concrete `neo_system::Node`.

mod types;

pub use types::{ConfigProvider, NeoNodeTypes, NodeTypes, StoreProvider, TxAdmission};
