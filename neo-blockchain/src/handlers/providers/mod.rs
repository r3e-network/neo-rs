//! # neo-blockchain::handlers::providers
//!
//! Narrow native-contract read capabilities used by command handlers.
//!
//! ## Boundary
//!
//! These adapters expose only the NEO and RoleManagement reads needed by
//! extensible-payload validation. They do not construct native contracts or
//! own storage.
//!
//! ## Contents
//!
//! - `extensible`: committee and designated-validator reads for payloads.

mod extensible;

pub(super) use extensible::{ExtensibleNativeProvider, NativeExtensibleProvider};
