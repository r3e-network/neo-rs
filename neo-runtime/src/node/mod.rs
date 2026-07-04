//! Node type hierarchy and component traits.
//!
//! This module defines [`NodeTypes`], [`NodeComponents`], and
//! [`FullNode`] — the compile-time type hierarchy that replaces
//! runtime-checked service wiring.

mod types;

pub use types::{
    BlockchainProvider, ConfigProvider, FullNode, FullNodeTypes, NeoNodeTypes,
    NodeComponents, NodeTypes, StoreProvider, TxAdmission,
};
