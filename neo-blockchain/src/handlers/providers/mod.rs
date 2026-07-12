//! # neo-blockchain::handlers::providers
//!
//! Narrow native-contract read capabilities used by command handlers.
//!
//! ## Boundary
//!
//! These adapters expose only the Policy, NEO, and RoleManagement reads needed
//! during admission. They do not construct native contracts or own storage.
//!
//! ## Contents
//!
//! - `extensible`: committee and designated-validator reads for payloads.
//! - `transaction`: Policy reads for transaction admission.

mod extensible;
mod transaction;

pub(super) use extensible::{ExtensibleNativeProvider, NativeExtensibleProvider};
pub(super) use transaction::{NativeTransactionProvider, TransactionNativeProvider};
