//! Service-command handler implementations.
//!
//! These modules add focused `BlockchainService` methods for each command
//! family. The public `pipeline::handlers` facade owns the command-handler
//! boundary; this module only keeps the implementation files mounted from a
//! single folder root.

mod block_inventory;
mod empty_fast_forward;
mod extensible;
mod headers;
mod import;
mod initialize;
mod persist_completed;
mod reverify;
mod transactions;
mod verification;
