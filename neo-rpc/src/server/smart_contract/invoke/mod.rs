//! # neo-rpc::server::smart_contract::invoke
//!
//! Smart-contract invocation execution and response projection.
//!
//! ## Boundary
//!
//! This module translates typed RPC requests into scripts and VM execution. It
//! does not define VM semantics, native-contract behavior, or transport policy.
//!
//! ## Contents
//!
//! - `diagnostics`: invocation-tree and storage-change JSON.
//! - `invocation`: invoke handlers and execution orchestration.
//! - `invocation_wallet`: wallet transaction materialization after execution.
//! - `script`: dynamic-call script and stack-argument construction.

use super::{helpers, native_provider, request, response};

mod diagnostics;
mod invocation;
mod invocation_wallet;
mod script;

pub(super) use invocation::{invoke_function, invoke_script};
pub(super) use script::emit_contract_parameter;
