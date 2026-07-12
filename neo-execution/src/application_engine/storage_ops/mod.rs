//! # neo-execution::application_engine::storage_ops
//!
//! ApplicationEngine script loading and storage syscall mechanics.
//!
//! ## Boundary
//!
//! This module mutates only the engine and its supplied cache overlay. Storage
//! backend durability and contract-specific policy remain outside execution.
//!
//! ## Contents
//!
//! - `load_execute_storage`: script loading, execution, and storage context APIs.
//! - `storage_low_level`: low-level storage get/find/put/delete operations.

use super::*;

mod load_execute_storage;
mod storage_low_level;
