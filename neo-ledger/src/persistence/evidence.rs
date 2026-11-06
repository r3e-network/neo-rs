//! Bounded, schema-free fault evidence staging and post-commit export.
//!
//! The ledger stages *pure* fault projections ([`FaultObservation`]) during
//! execution and exports them only after the canonical commit succeeds (step 15b).
//! It never constructs the system/rpc observation schema: the schema/rule version
//! stamps, the wall-clock observation time and the `TransactionExecutionObservation`
//! envelope are applied by the [`EvidenceSink`] adapter owned by `neo-system`.
//!
//! Staging is strictly bounded: persistence must never allocate in proportion to the
//! number of FAULT transactions in a block. Overflow is counted as `dropped` and
//! reported through [`EvidenceSink::record_dropped`]; it can never fail the pipeline.

use neo_primitives::UInt256;
use neo_vm::VmState;

/// Hard ceiling for fault evidence retained until the canonical commit succeeds.
///
/// The configured evidence queue may be much larger, but the effective staging limit
/// is `min(available_capacity, MAX_PENDING_FAULT_OBSERVATIONS_PER_BLOCK)`.
pub const MAX_PENDING_FAULT_OBSERVATIONS_PER_BLOCK: usize = 64;

/// Pure fault classification the ledger can derive without any observation schema.
///
/// Mirrors the canonical rule: a captured VM exception is `CaughtVmException`,
/// otherwise the bare fault is `VmFault`. The `neo-system` evidence adapter maps this
/// onto `TransactionExecutionExceptionCategory`; a VM FAULT is never an attack verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultCategory {
    /// The engine reported a catchable VM exception (`fault_exception().is_some()`).
    CaughtVmException,
    /// A bare VM fault without a captured exception.
    VmFault,
}

/// Pure, schema-free projection of a faulted transaction execution (step 10b).
#[derive(Clone, Debug)]
pub struct FaultObservation {
    /// Hash of the faulted transaction.
    pub transaction_hash: UInt256,
    /// Height of the block containing the faulted transaction.
    pub block_height: u32,
    /// Hash of the block containing the faulted transaction.
    pub block_hash: UInt256,
    /// Terminal VM state; always [`VmState::FAULT`] for staged observations.
    pub vm_state: VmState,
    /// Pure fault classification derived by the ledger.
    pub category: FaultCategory,
    /// GAS consumed by the faulted execution.
    pub gas_consumed: i64,
}

/// Post-commit fault-evidence export sink (step 15b).
///
/// Every method is infallible with respect to the persistence pipeline: an
/// unavailable, full, or closed sink must never change canonical fault results or
/// abort a successful commit.
pub trait EvidenceSink: Send + Sync {
    /// Currently available staging capacity, used to bound the pre-commit buffer.
    fn available_capacity(&self) -> usize;

    /// Attempts to export one observation after the commit succeeded.
    ///
    /// Returns whether the observation was accepted; `false` counts as dropped.
    fn try_emit(&self, observation: &FaultObservation) -> bool;

    /// Records the total number of observations dropped for this block.
    fn record_dropped(&self, dropped: u64);
}

/// Bounded staging buffer that never grows with the FAULT count (steps 2/10b/15b).
pub(crate) struct FaultObservationBuffer {
    pub(crate) observations: Vec<FaultObservation>,
    limit: usize,
    pub(crate) dropped: u64,
}

impl FaultObservationBuffer {
    /// Creates a buffer whose limit is bounded by both the available sink capacity
    /// and [`MAX_PENDING_FAULT_OBSERVATIONS_PER_BLOCK`].
    pub(crate) fn for_available_capacity(available_capacity: usize) -> Self {
        let limit = available_capacity.min(MAX_PENDING_FAULT_OBSERVATIONS_PER_BLOCK);
        Self {
            observations: Vec::with_capacity(limit),
            limit,
            dropped: 0,
        }
    }

    /// Stages an observation, counting overflow as dropped instead of allocating.
    pub(crate) fn push(&mut self, observation: FaultObservation) {
        if self.observations.len() < self.limit {
            self.observations.push(observation);
        } else {
            self.dropped = self.dropped.saturating_add(1);
        }
    }

    /// Number of observations actually staged (bounded).
    pub(crate) fn staged_len(&self) -> usize {
        self.observations.len()
    }
}
