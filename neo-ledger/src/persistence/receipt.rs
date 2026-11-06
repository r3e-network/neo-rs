//! Pure execution projections produced by the ledger persistence operation.
//!
//! The ledger emits [`ExecutionReceipt`] values only. `ApplicationExecuted` — the
//! full system/rpc diagnostic schema carrying the transaction payload, notification
//! list, log list and result stack — is preserved exclusively as a `neo-system`/rpc
//! adapter concern (task-102 Part B). A receipt is a lossless projection of the
//! canonical facts the ledger pipeline itself depends on; the system adapter is
//! responsible for bridging receipts (and the engine ports that produced them) back
//! into `ApplicationExecuted` without the ledger ever naming that schema.

use neo_primitives::{TriggerType, UInt256};
use neo_vm::VmState;

/// Pure projection of a single engine execution within the persist pipeline.
///
/// One receipt is produced per `OnPersist` engine (step 7), per transaction engine
/// (step 10, detailed mode only), and per `PostPersist` engine (step 11), preserving
/// the canonical emission order.
#[derive(Clone, Debug, PartialEq)]
pub struct ExecutionReceipt {
    /// Trigger that produced this execution (`OnPersist`, `Application`, `PostPersist`).
    pub trigger: TriggerType,
    /// Transaction hash for `Application` receipts; `None` for native triggers.
    pub transaction_hash: Option<UInt256>,
    /// Terminal VM state of the execution.
    pub vm_state: VmState,
    /// Total GAS consumed by the execution.
    pub gas_consumed: i64,
    /// Fault exception message, if the execution faulted with a captured exception.
    pub exception: Option<String>,
}

impl ExecutionReceipt {
    /// Builds a native-trigger receipt (no transaction hash).
    pub fn native(trigger: TriggerType, vm_state: VmState, gas_consumed: i64) -> Self {
        Self {
            trigger,
            transaction_hash: None,
            vm_state,
            gas_consumed,
            exception: None,
        }
    }

    /// Returns `true` when this receipt represents a faulted application transaction.
    pub fn is_faulted_transaction(&self) -> bool {
        self.transaction_hash.is_some() && self.vm_state == VmState::FAULT
    }
}

/// Canonical result of a successful [`super::persist_block`] operation.
#[derive(Clone, Debug)]
pub struct PersistReceipt {
    /// Hash of the persisted block.
    pub block_hash: UInt256,
    /// Height of the persisted block.
    pub block_height: u32,
    /// Whether detailed execution receipts were emitted (step 1 gating outcome).
    pub detailed: bool,
    /// Ordered execution receipts (OnPersist, per-transaction, PostPersist).
    pub receipts: Vec<ExecutionReceipt>,
    /// Number of fault observations staged into the bounded buffer (steps 2/10b).
    pub staged_fault_observations: usize,
    /// Number of fault observations dropped for want of evidence capacity (step 15b).
    pub dropped_fault_observations: u64,
}
