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
pub(crate) mod helpers;
pub(crate) mod mempool;
pub(crate) mod network;
pub(crate) mod parallel_execution;
pub(crate) mod persistence;
pub mod registry;
pub(crate) mod relay;
pub(crate) mod services;
pub(crate) mod storage;
pub mod system;
pub(crate) mod system_context;

// Re-export everything from core for backward compatibility
pub use context::{
    NeoSystemContext, TRANSACTION_EXECUTION_OBSERVATION_RULE_VERSION,
    TRANSACTION_EXECUTION_OBSERVATION_SCHEMA_VERSION, TransactionExecutionExceptionCategory,
    TransactionExecutionObservation, TransactionExecutionSink, TransactionExecutionStage,
};
pub use core::*;

// Re-export from extracted modules (these override legacy exports)
pub use actors::TransactionRouterHandle;
pub use builder::NeoSystemBuilder;
// Execution strategy (`ExecutionMode`, fast-path eligibility, dual-execution
// verification) is execution semantics and lives in `neo-smart-contract`; this
// re-export keeps the historical `neo_system::ExecutionMode` path resolving.
pub use neo_smart_contract::application_engine::execution_strategy::ExecutionMode;
pub use parallel_execution::{ParallelTransactionLoop, parallel_execute_block};
pub use persistence::{
    EmptyBlockProfileSnapshot, ExecutionProfileSnapshot, ExecutionStats, FastPathStatsSnapshot,
    PERSISTENCE_STAGE_NESTING, ParallelExecutionSnapshot,
};
pub use registry::ServiceRegistry;
pub use system::{ReadinessStatus, STATE_STORE_SERVICE};

// Re-export ProtocolSettings for convenience
pub use neo_protocol::protocol_settings::ProtocolSettings;
