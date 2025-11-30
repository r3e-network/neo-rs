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

// Core module containing NeoSystem and NeoSystemContext
mod core;

// Extracted submodules with clean implementations
pub(crate) mod actors;
pub mod builder;
pub(crate) mod converters;
pub(crate) mod genesis;
pub(crate) mod helpers;
pub mod registry;
pub(crate) mod relay;
pub mod system;

// Re-export everything from core for backward compatibility
pub use core::*;

// Re-export from extracted modules (these override legacy exports)
pub use actors::TransactionRouterMessage;
pub use builder::NeoSystemBuilder;
pub use registry::ServiceRegistry;
pub use system::{ReadinessStatus, STATE_STORE_SERVICE};

// Re-export ProtocolSettings for convenience
pub use crate::protocol_settings::ProtocolSettings;
