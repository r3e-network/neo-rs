//! Opt-in execution counters for targeted VM performance diagnostics.
//!
//! The collector is deliberately detached from consensus state. Engines do not
//! allocate it unless a caller explicitly enables profiling, and snapshots are
//! read-only observations of opcode dispatch and evaluation-stack operations.

use crate::OpCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

const OPCODE_COUNT: usize = 256;

/// Broad NeoVM opcode families used to identify execution workload shape.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(usize)]
pub enum OpcodeClass {
    /// Constants and byte-string pushes.
    Push,
    /// Branches, calls, exceptions, returns, and syscalls.
    ControlFlow,
    /// Evaluation-stack rearrangement operations.
    Stack,
    /// Static, local, and argument slot operations.
    Slot,
    /// Buffer and byte-string splice operations.
    Splice,
    /// Bitwise and byte-sequence equality operations.
    Bitwise,
    /// Integer arithmetic and numeric comparison operations.
    Numeric,
    /// Array, struct, and map operations.
    Compound,
    /// Runtime type inspection and conversion operations.
    Type,
}

impl OpcodeClass {
    /// All opcode classes in stable display order.
    pub const ALL: [Self; 9] = [
        Self::Push,
        Self::ControlFlow,
        Self::Stack,
        Self::Slot,
        Self::Splice,
        Self::Bitwise,
        Self::Numeric,
        Self::Compound,
        Self::Type,
    ];

    /// Returns the class containing `opcode`.
    #[must_use]
    pub const fn for_opcode(opcode: OpCode) -> Self {
        match opcode.byte() {
            0x00..=0x20 => Self::Push,
            0x21..=0x41 | 0xE0..=0xE1 => Self::ControlFlow,
            0x43..=0x55 => Self::Stack,
            0x56..=0x87 => Self::Slot,
            0x88..=0x8E => Self::Splice,
            0x90..=0x98 => Self::Bitwise,
            0x99..=0xBB => Self::Numeric,
            0xBE..=0xD4 => Self::Compound,
            0xD8..=0xDB => Self::Type,
            // Every canonical v3.10.1 opcode is covered above. Keep a stable
            // fallback for forward-compatible enum additions.
            _ => Self::ControlFlow,
        }
    }

    /// Stable lowercase name used by profiling output.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Push => "push",
            Self::ControlFlow => "control_flow",
            Self::Stack => "stack",
            Self::Slot => "slot",
            Self::Splice => "splice",
            Self::Bitwise => "bitwise",
            Self::Numeric => "numeric",
            Self::Compound => "compound",
            Self::Type => "type",
        }
    }
}

/// Successful evaluation-stack operations observed during one profiled run.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StackOperationProfile {
    /// Items pushed onto evaluation or result stacks.
    pub pushes: u64,
    /// Top items popped from evaluation stacks.
    pub pops: u64,
    /// Immutable stack peeks.
    pub peeks: u64,
    /// Mutable stack peeks.
    pub mutable_peeks: u64,
    /// Indexed insertions.
    pub inserts: u64,
    /// Indexed removals (top-level `pop` is tracked separately).
    pub removes: u64,
    /// Stack swap calls.
    pub swaps: u64,
    /// Stack reverse calls.
    pub reverses: u64,
    /// Stack clear calls.
    pub clears: u64,
    /// Items removed by clear calls.
    pub cleared_items: u64,
    /// Successful stack copy calls.
    pub copies: u64,
    /// Items copied by stack copy calls.
    pub copied_items: u64,
    /// Successful stack move calls.
    pub moves: u64,
    /// Items transferred by stack move calls.
    pub moved_items: u64,
    /// Largest individual evaluation/result stack depth observed.
    pub max_depth: u64,
}

#[derive(Debug, Default)]
struct StackProfileCounters {
    pushes: AtomicU64,
    pops: AtomicU64,
    peeks: AtomicU64,
    mutable_peeks: AtomicU64,
    inserts: AtomicU64,
    removes: AtomicU64,
    swaps: AtomicU64,
    reverses: AtomicU64,
    clears: AtomicU64,
    cleared_items: AtomicU64,
    copies: AtomicU64,
    copied_items: AtomicU64,
    moves: AtomicU64,
    moved_items: AtomicU64,
    max_depth: AtomicU64,
}

/// Shared counter handle attached only to stacks in a profiled engine.
#[derive(Clone, Debug, Default)]
pub(crate) struct StackProfileHandle(Arc<StackProfileCounters>);

impl StackProfileHandle {
    #[inline(always)]
    pub(crate) fn record_push(&self, depth: usize) {
        self.0.pushes.fetch_add(1, Ordering::Relaxed);
        self.observe_depth(depth);
    }

    #[inline(always)]
    pub(crate) fn record_pop(&self) {
        self.0.pops.fetch_add(1, Ordering::Relaxed);
    }

    #[inline(always)]
    pub(crate) fn record_peek(&self) {
        self.0.peeks.fetch_add(1, Ordering::Relaxed);
    }

    #[inline(always)]
    pub(crate) fn record_mutable_peek(&self) {
        self.0.mutable_peeks.fetch_add(1, Ordering::Relaxed);
    }

    #[inline(always)]
    pub(crate) fn record_insert(&self, depth: usize) {
        self.0.inserts.fetch_add(1, Ordering::Relaxed);
        self.observe_depth(depth);
    }

    #[inline(always)]
    pub(crate) fn record_remove(&self) {
        self.0.removes.fetch_add(1, Ordering::Relaxed);
    }

    #[inline(always)]
    pub(crate) fn record_swap(&self) {
        self.0.swaps.fetch_add(1, Ordering::Relaxed);
    }

    #[inline(always)]
    pub(crate) fn record_reverse(&self) {
        self.0.reverses.fetch_add(1, Ordering::Relaxed);
    }

    #[inline(always)]
    pub(crate) fn record_clear(&self, item_count: usize) {
        self.0.clears.fetch_add(1, Ordering::Relaxed);
        self.0
            .cleared_items
            .fetch_add(item_count as u64, Ordering::Relaxed);
    }

    #[inline(always)]
    pub(crate) fn record_copy(&self, item_count: usize) {
        self.0.copies.fetch_add(1, Ordering::Relaxed);
        self.0
            .copied_items
            .fetch_add(item_count as u64, Ordering::Relaxed);
    }

    #[inline(always)]
    pub(crate) fn record_move(&self, item_count: usize) {
        self.0.moves.fetch_add(1, Ordering::Relaxed);
        self.0
            .moved_items
            .fetch_add(item_count as u64, Ordering::Relaxed);
    }

    #[inline(always)]
    pub(crate) fn observe_depth(&self, depth: usize) {
        self.0.max_depth.fetch_max(depth as u64, Ordering::Relaxed);
    }

    fn snapshot(&self) -> StackOperationProfile {
        StackOperationProfile {
            pushes: self.0.pushes.load(Ordering::Relaxed),
            pops: self.0.pops.load(Ordering::Relaxed),
            peeks: self.0.peeks.load(Ordering::Relaxed),
            mutable_peeks: self.0.mutable_peeks.load(Ordering::Relaxed),
            inserts: self.0.inserts.load(Ordering::Relaxed),
            removes: self.0.removes.load(Ordering::Relaxed),
            swaps: self.0.swaps.load(Ordering::Relaxed),
            reverses: self.0.reverses.load(Ordering::Relaxed),
            clears: self.0.clears.load(Ordering::Relaxed),
            cleared_items: self.0.cleared_items.load(Ordering::Relaxed),
            copies: self.0.copies.load(Ordering::Relaxed),
            copied_items: self.0.copied_items.load(Ordering::Relaxed),
            moves: self.0.moves.load(Ordering::Relaxed),
            moved_items: self.0.moved_items.load(Ordering::Relaxed),
            max_depth: self.0.max_depth.load(Ordering::Relaxed),
        }
    }
}

/// Read-only snapshot of a targeted VM execution profile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VmExecutionProfile {
    opcode_counts: [u64; OPCODE_COUNT],
    stack_operations: StackOperationProfile,
}

impl VmExecutionProfile {
    /// Number of dispatched instructions represented by the profile.
    #[must_use]
    pub fn total_instructions(&self) -> u64 {
        self.opcode_counts.iter().sum()
    }

    /// Number of times `opcode` was dispatched, including faulting handlers.
    #[must_use]
    pub const fn opcode_count(&self, opcode: OpCode) -> u64 {
        self.opcode_counts[opcode.byte() as usize]
    }

    /// Number of instructions dispatched from `class`.
    #[must_use]
    pub fn opcode_class_count(&self, class: OpcodeClass) -> u64 {
        OpCode::ALL
            .iter()
            .copied()
            .filter(|opcode| OpcodeClass::for_opcode(*opcode) == class)
            .map(|opcode| self.opcode_count(opcode))
            .sum()
    }

    /// Non-zero opcode counters ordered by descending count and then opcode byte.
    #[must_use]
    pub fn hottest_opcodes(&self, limit: usize) -> Vec<(OpCode, u64)> {
        let mut counts = OpCode::ALL
            .iter()
            .copied()
            .filter_map(|opcode| {
                let count = self.opcode_count(opcode);
                (count > 0).then_some((opcode, count))
            })
            .collect::<Vec<_>>();
        counts.sort_unstable_by(|(left_opcode, left_count), (right_opcode, right_count)| {
            right_count
                .cmp(left_count)
                .then_with(|| left_opcode.byte().cmp(&right_opcode.byte()))
        });
        counts.truncate(limit);
        counts
    }

    /// Evaluation-stack activity observed during execution.
    #[must_use]
    pub const fn stack_operations(&self) -> StackOperationProfile {
        self.stack_operations
    }
}

/// Mutable engine-local collector. Opcode counters need no atomics because an
/// engine dispatches instructions serially; only its shared stack handle does.
pub(crate) struct ExecutionProfileCollector {
    opcode_counts: [u64; OPCODE_COUNT],
    stack: StackProfileHandle,
}

impl ExecutionProfileCollector {
    pub(crate) fn new() -> Self {
        Self {
            opcode_counts: [0; OPCODE_COUNT],
            stack: StackProfileHandle::default(),
        }
    }

    #[inline(always)]
    pub(crate) fn record_opcode(&mut self, opcode: OpCode) {
        self.opcode_counts[opcode.byte() as usize] += 1;
    }

    pub(crate) fn stack_handle(&self) -> StackProfileHandle {
        self.stack.clone()
    }

    pub(crate) fn snapshot(&self) -> VmExecutionProfile {
        VmExecutionProfile {
            opcode_counts: self.opcode_counts,
            stack_operations: self.stack.snapshot(),
        }
    }
}
