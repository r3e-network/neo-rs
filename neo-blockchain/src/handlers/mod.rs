//! # Blockchain Command Handlers
//!
//! These modules add focused `BlockchainService` methods for each command
//! family. The public `pipeline::handlers` facade owns the command-handler
//! boundary; this module only keeps the implementation files mounted from a
//! single folder root.
//!
//! ## Boundary
//!
//! Handlers orchestrate canonical blockchain capabilities. Protocol codecs,
//! storage engines, and application composition remain in their owning crates.
//!
//! ## Contents
//!
//! Modules are grouped by inventory, import, initialization, transaction,
//! header, verification, and maintenance command families.

mod block_inventory;
mod empty_fast_forward;
mod extensible;
mod extensible_provider;
mod headers;
mod import;
mod initialize;
mod reverify;
mod transaction_provider;
mod transactions;
mod verification;
