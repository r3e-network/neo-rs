//! Core node system orchestration (actors, services, plugins, wallets, networking).
//!
//! This module provides the central runtime for a Neo node, coordinating:
//! - Actor system lifecycle (blockchain, local node, task manager)
//! - Service registration and discovery
//! - Plugin management
//! - Wallet integration
//! - Network operations
//!
//! # Module Structure
//!
//! The codebase has been refactored into focused modules:
//! - `core` - NeoSystem and NeoSystemContext implementations
//! - `registry` - `ServiceRegistry` for typed service discovery
//! - `actors` - Internal actor implementations
//! - `helpers` - Utility functions and internal types
//! - `converters` - Type conversion functions
//! - `relay` - Relay cache types
//! - `system` - Readiness status and constants
//! - `builder` - Fluent builder for NeoSystem construction
//! - `network` - P2P actor helpers and peer management
//! - `services` - Service registration and handler wiring
//! - `mempool` - Mempool callbacks and plugin notifications
//! - `persistence` - Block execution and commit pipeline helpers

// Core module containing NeoSystem and NeoSystemContext
pub mod context;
mod core;

// Extracted submodules with clean implementations
pub(crate) mod actors;
pub mod builder;
pub(crate) mod converters;
pub(crate) mod helpers;
pub(crate) mod mempool;
pub(crate) mod network;
pub(crate) mod persistence;
pub mod registry;
pub(crate) mod relay;
pub(crate) mod services;
pub(crate) mod storage;
pub mod system;

// Re-export everything from core for backward compatibility
pub use context::NeoSystemContext;
pub use core::*;

// Re-export from extracted modules (these override legacy exports)
pub use actors::TransactionRouterMessage;
pub use builder::NeoSystemBuilder;
pub use registry::ServiceRegistry;
pub use system::{ReadinessStatus, STATE_STORE_SERVICE};

// Re-export ProtocolSettings for convenience
pub use crate::protocol_settings::ProtocolSettings;
