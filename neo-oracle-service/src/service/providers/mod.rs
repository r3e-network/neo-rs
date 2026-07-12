//! # neo-oracle-service::service::providers
//!
//! Narrow ledger and native-contract reads used by oracle workflows.
//!
//! ## Boundary
//!
//! These adapters project the node-composed providers into oracle-specific
//! capabilities. They do not own storage, contract dispatch, or transaction
//! admission.
//!
//! ## Contents
//!
//! - `ledger_provider`: persisted transaction and conflict reads.
//! - `native_provider`: Oracle, Policy, RoleManagement, and contract reads.

use super::{OracleRuntimeProvider, OracleService};

mod ledger_provider;
mod native_provider;

pub(super) use ledger_provider::{NativeOracleLedgerProvider, OracleLedgerProvider};
pub use native_provider::OracleContractReadProvider;
pub(super) use native_provider::OracleServiceNativeProvider;
