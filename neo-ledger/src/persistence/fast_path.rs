//! Fast-path execution integration for neo-ledger persistence layer.
//!
//! This module provides hooks for fast-path transaction execution optimization.
//! When enabled via NEO_FAST_PATH_ENABLED, simple transactions can bypass full VM execution.
//!
//! ## Status
//!
//! Currently a stub - returns None to fallback to full VM execution.
//! Full implementation pending: direct storage updates for NEP-17 transfers.

use neo_protocol::payloads::transaction::Transaction;
use neo_storage::persistence::DataCache;
use neo_vm::fast_path::FastPathExecutor;
use neo_vm::VmState;
use std::sync::{Arc, Mutex, OnceLock};

/// Global fast-path executor with statistics tracking.
static FAST_PATH_EXECUTOR: OnceLock<Mutex<FastPathExecutor>> = OnceLock::new();

fn get_executor() -> &'static Mutex<FastPathExecutor> {
    FAST_PATH_EXECUTOR.get_or_init(|| Mutex::new(FastPathExecutor::new()))
}

/// Global flag: is fast-path enabled via environment variable?
pub static FAST_PATH_ENABLED: OnceLock<bool> = OnceLock::new();

fn is_fast_path_enabled() -> bool {
    *FAST_PATH_ENABLED.get_or_init(|| {
        std::env::var("NEO_FAST_PATH_ENABLED")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    })
}

/// Result of a fast-path execution attempt.
#[derive(Debug)]
pub struct FastPathResult {
    /// VM state after execution (HALT or FAULT)
    pub vm_state: VmState,

    /// Gas consumed during execution
    pub gas_consumed: i64,

    /// Whether storage was modified
    pub storage_modified: bool,
}

/// Attempts fast-path execution for a transaction.
///
/// Returns `Some(FastPathResult)` if fast-path handled the transaction,
/// or `None` if fallback to full VM is required.
///
/// ## Current Implementation
///
/// This is currently a stub that always returns None (full VM fallback).
/// Full implementation will directly update storage for simple NEP-17 transfers.
pub fn try_fast_path_execution(
    tx: &Transaction,
    _tx_overlay: &Arc<DataCache>,
    _system_fee: i64,
) -> Option<FastPathResult> {
    // Fast-path disabled globally
    if !is_fast_path_enabled() {
        return None;
    }

    let script = tx.script();

    // Classify the transaction (but don't use the result yet)
    let executor = get_executor().lock().unwrap();
    let _complexity = executor.classify_transaction(script);
    drop(executor);

    // TODO: Implement direct storage updates for simple transfers
    // For now, always fallback to full VM
    None
}

/// Gets current fast-path execution statistics.
pub fn get_fast_path_stats() -> String {
    let executor = get_executor().lock().unwrap();
    format!("Fast-path execution statistics: {:?}", executor.stats())
}
