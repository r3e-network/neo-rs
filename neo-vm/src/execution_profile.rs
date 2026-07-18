//! Opt-in execution counters for targeted VM performance diagnostics.
//!
//! The collector is deliberately detached from consensus state. Engines do not
//! allocate it unless a caller explicitly enables profiling, and snapshots are
//! read-only observations of opcode dispatch and evaluation-stack operations.

use crate::OpCode;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

const OPCODE_COUNT: usize = 256;
const MAX_PROFILED_SCRIPTS: usize = 64;
const MAX_PROFILED_ENTRY_POINTS: usize = 32;

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

/// One entry offset observed while loading a profiled script context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScriptEntryProfile {
    entry_offset: usize,
    context_loads: u64,
}

impl ScriptEntryProfile {
    /// Byte offset at which the context started execution.
    #[must_use]
    pub const fn entry_offset(&self) -> usize {
        self.entry_offset
    }

    /// Number of contexts loaded at this offset.
    #[must_use]
    pub const fn context_loads(&self) -> u64 {
        self.context_loads
    }
}

/// Bounded profile for one immutable NeoVM bytecode script.
///
/// `script_hash` is Neo's protocol `Hash160` of the bytecode. It is an
/// observational fingerprint, not authority for memoizing execution results;
/// any future decoded-script cache must also compare the exact bytes and bind
/// its validation/protocol context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScriptExecutionProfile {
    script_hash: [u8; 20],
    script_len: usize,
    instructions: u64,
    context_loads: u64,
    entry_points: Vec<ScriptEntryProfile>,
    other_entry_context_loads: u64,
}

impl ScriptExecutionProfile {
    /// Neo protocol `Hash160` of the immutable bytecode.
    #[must_use]
    pub const fn script_hash(&self) -> &[u8; 20] {
        &self.script_hash
    }

    /// Byte length of the immutable script.
    #[must_use]
    pub const fn script_len(&self) -> usize {
        self.script_len
    }

    /// Instructions dispatched from this script, including faulting handlers.
    #[must_use]
    pub const fn instructions(&self) -> u64 {
        self.instructions
    }

    /// Total invocation contexts loaded for this script.
    #[must_use]
    pub const fn context_loads(&self) -> u64 {
        self.context_loads
    }

    /// Entry offsets retained by the bounded collector, ordered by descending
    /// context count and then ascending byte offset.
    #[must_use]
    pub fn entry_points(&self) -> &[ScriptEntryProfile] {
        &self.entry_points
    }

    /// Context loads whose distinct entry offset exceeded the per-script bound.
    #[must_use]
    pub const fn other_entry_context_loads(&self) -> u64 {
        self.other_entry_context_loads
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct ScriptProfileKey {
    script_hash: [u8; 20],
    script_len: usize,
}

#[derive(Debug)]
struct ScriptProfileCounters {
    instructions: u64,
    context_loads: u64,
    entry_points: Vec<ScriptEntryProfile>,
    other_entry_context_loads: u64,
}

impl ScriptProfileCounters {
    fn new(entry_offset: usize) -> Self {
        Self {
            instructions: 0,
            context_loads: 1,
            entry_points: vec![ScriptEntryProfile {
                entry_offset,
                context_loads: 1,
            }],
            other_entry_context_loads: 0,
        }
    }

    fn record_context_load(&mut self, entry_offset: usize) {
        self.context_loads = self.context_loads.saturating_add(1);
        if let Some(entry) = self
            .entry_points
            .iter_mut()
            .find(|entry| entry.entry_offset == entry_offset)
        {
            entry.context_loads = entry.context_loads.saturating_add(1);
        } else if self.entry_points.len() < MAX_PROFILED_ENTRY_POINTS {
            self.entry_points.push(ScriptEntryProfile {
                entry_offset,
                context_loads: 1,
            });
        } else {
            self.other_entry_context_loads = self.other_entry_context_loads.saturating_add(1);
        }
    }
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
    max_reference_count: u64,
    scripts: Vec<ScriptExecutionProfile>,
    other_script_instructions: u64,
    other_script_context_loads: u64,
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

    /// Largest recursive StackItem reference count observed after an
    /// instruction in this execution.
    #[must_use]
    pub const fn max_reference_count(&self) -> u64 {
        self.max_reference_count
    }

    /// Bounded script profiles, ordered by descending instruction count and
    /// then protocol script hash and byte length.
    #[must_use]
    pub fn scripts(&self) -> &[ScriptExecutionProfile] {
        &self.scripts
    }

    /// Instructions from scripts beyond the collector's distinct-script bound.
    #[must_use]
    pub const fn other_script_instructions(&self) -> u64 {
        self.other_script_instructions
    }

    /// Context loads from scripts beyond the collector's distinct-script bound.
    #[must_use]
    pub const fn other_script_context_loads(&self) -> u64 {
        self.other_script_context_loads
    }
}

/// Mutable engine-local collector. Opcode counters need no atomics because an
/// engine dispatches instructions serially; only its shared stack handle does.
pub(crate) struct ExecutionProfileCollector {
    opcode_counts: [u64; OPCODE_COUNT],
    stack: StackProfileHandle,
    max_reference_count: u64,
    scripts: HashMap<ScriptProfileKey, ScriptProfileCounters>,
    other_script_instructions: u64,
    other_script_context_loads: u64,
}

impl ExecutionProfileCollector {
    pub(crate) fn new() -> Self {
        Self {
            opcode_counts: [0; OPCODE_COUNT],
            stack: StackProfileHandle::default(),
            max_reference_count: 0,
            scripts: HashMap::with_capacity(MAX_PROFILED_SCRIPTS),
            other_script_instructions: 0,
            other_script_context_loads: 0,
        }
    }

    #[inline(always)]
    pub(crate) fn record_opcode(
        &mut self,
        script_hash: [u8; 20],
        script_len: usize,
        opcode: OpCode,
    ) {
        self.opcode_counts[opcode.byte() as usize] += 1;
        let key = ScriptProfileKey {
            script_hash,
            script_len,
        };
        if let Some(script) = self.scripts.get_mut(&key) {
            script.instructions = script.instructions.saturating_add(1);
        } else {
            self.other_script_instructions = self.other_script_instructions.saturating_add(1);
        }
    }

    pub(crate) fn record_context_load(
        &mut self,
        script_hash: [u8; 20],
        script_len: usize,
        entry_offset: usize,
    ) {
        let key = ScriptProfileKey {
            script_hash,
            script_len,
        };
        if let Some(script) = self.scripts.get_mut(&key) {
            script.record_context_load(entry_offset);
        } else if self.scripts.len() < MAX_PROFILED_SCRIPTS {
            self.scripts
                .insert(key, ScriptProfileCounters::new(entry_offset));
        } else {
            self.other_script_context_loads = self.other_script_context_loads.saturating_add(1);
        }
    }

    pub(crate) fn stack_handle(&self) -> StackProfileHandle {
        self.stack.clone()
    }

    #[inline(always)]
    pub(crate) fn observe_reference_count(&mut self, count: usize) {
        self.max_reference_count = self.max_reference_count.max(count as u64);
    }

    pub(crate) fn snapshot(&self) -> VmExecutionProfile {
        let mut scripts = self
            .scripts
            .iter()
            .map(|(key, counters)| {
                let mut entry_points = counters.entry_points.clone();
                entry_points.sort_unstable_by(|left, right| {
                    right
                        .context_loads
                        .cmp(&left.context_loads)
                        .then_with(|| left.entry_offset.cmp(&right.entry_offset))
                });
                ScriptExecutionProfile {
                    script_hash: key.script_hash,
                    script_len: key.script_len,
                    instructions: counters.instructions,
                    context_loads: counters.context_loads,
                    entry_points,
                    other_entry_context_loads: counters.other_entry_context_loads,
                }
            })
            .collect::<Vec<_>>();
        scripts.sort_unstable_by(|left, right| {
            right
                .instructions
                .cmp(&left.instructions)
                .then_with(|| left.script_hash.cmp(&right.script_hash))
                .then_with(|| left.script_len.cmp(&right.script_len))
        });

        VmExecutionProfile {
            opcode_counts: self.opcode_counts,
            stack_operations: self.stack.snapshot(),
            max_reference_count: self.max_reference_count,
            scripts,
            other_script_instructions: self.other_script_instructions,
            other_script_context_loads: self.other_script_context_loads,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn populated_collector(script_order: &[u8], entry_order: &[usize]) -> VmExecutionProfile {
        let mut collector = ExecutionProfileCollector::new();
        for &discriminator in script_order {
            let script_hash = [discriminator; 20];
            collector.record_context_load(script_hash, 3, 0);
            collector.record_opcode(script_hash, 3, OpCode::NOP);
        }

        let repeated_script = [0xF0; 20];
        for &entry_offset in entry_order {
            collector.record_context_load(repeated_script, 64, entry_offset);
        }
        collector.snapshot()
    }

    #[test]
    fn snapshot_order_is_deterministic_across_observation_order() {
        let forward = populated_collector(&[3, 1, 2], &[7, 1, 4]);
        let reverse = populated_collector(&[2, 1, 3], &[4, 1, 7]);

        assert_eq!(forward, reverse);
        assert_eq!(
            forward
                .scripts()
                .iter()
                .map(|script| *script.script_hash())
                .collect::<Vec<_>>(),
            vec![[1; 20], [2; 20], [3; 20], [0xF0; 20]]
        );
        assert_eq!(
            forward.scripts()[3]
                .entry_points()
                .iter()
                .map(ScriptEntryProfile::entry_offset)
                .collect::<Vec<_>>(),
            vec![1, 4, 7]
        );
    }

    #[test]
    fn collector_enforces_script_and_entry_capacity() {
        let mut scripts = ExecutionProfileCollector::new();
        for discriminator in 0..=MAX_PROFILED_SCRIPTS {
            let mut script_hash = [0; 20];
            script_hash[..std::mem::size_of::<usize>()]
                .copy_from_slice(&discriminator.to_le_bytes());
            scripts.record_context_load(script_hash, discriminator + 1, 0);
            scripts.record_opcode(script_hash, discriminator + 1, OpCode::NOP);
        }

        let snapshot = scripts.snapshot();
        assert_eq!(snapshot.scripts().len(), MAX_PROFILED_SCRIPTS);
        assert_eq!(snapshot.other_script_context_loads(), 1);
        assert_eq!(snapshot.other_script_instructions(), 1);
        assert_eq!(
            snapshot
                .scripts()
                .iter()
                .map(ScriptExecutionProfile::instructions)
                .sum::<u64>(),
            MAX_PROFILED_SCRIPTS as u64
        );

        let mut entries = ExecutionProfileCollector::new();
        let script_hash = [0xA5; 20];
        for entry_offset in 0..=MAX_PROFILED_ENTRY_POINTS {
            entries.record_context_load(script_hash, 64, entry_offset);
        }
        let snapshot = entries.snapshot();
        let script = &snapshot.scripts()[0];
        assert_eq!(script.entry_points().len(), MAX_PROFILED_ENTRY_POINTS);
        assert_eq!(script.other_entry_context_loads(), 1);
        assert_eq!(
            script.context_loads(),
            (MAX_PROFILED_ENTRY_POINTS + 1) as u64
        );
    }

    #[test]
    fn shared_stack_counters_are_exact_under_concurrency() {
        const WORKERS: usize = 8;
        const OPERATIONS_PER_WORKER: usize = 2_000;

        let collector = ExecutionProfileCollector::new();
        let handle = collector.stack_handle();
        std::thread::scope(|scope| {
            for worker in 0..WORKERS {
                let handle = handle.clone();
                scope.spawn(move || {
                    for operation in 0..OPERATIONS_PER_WORKER {
                        handle.record_push(worker * OPERATIONS_PER_WORKER + operation + 1);
                        handle.record_pop();
                        handle.record_peek();
                        handle.record_mutable_peek();
                        handle.record_insert(1);
                        handle.record_remove();
                        handle.record_swap();
                        handle.record_reverse();
                        handle.record_clear(2);
                        handle.record_copy(3);
                        handle.record_move(4);
                    }
                });
            }
        });

        let stack = collector.snapshot().stack_operations();
        let operations = (WORKERS * OPERATIONS_PER_WORKER) as u64;
        assert_eq!(stack.pushes, operations);
        assert_eq!(stack.pops, operations);
        assert_eq!(stack.peeks, operations);
        assert_eq!(stack.mutable_peeks, operations);
        assert_eq!(stack.inserts, operations);
        assert_eq!(stack.removes, operations);
        assert_eq!(stack.swaps, operations);
        assert_eq!(stack.reverses, operations);
        assert_eq!(stack.clears, operations);
        assert_eq!(stack.cleared_items, operations * 2);
        assert_eq!(stack.copies, operations);
        assert_eq!(stack.copied_items, operations * 3);
        assert_eq!(stack.moves, operations);
        assert_eq!(stack.moved_items, operations * 4);
        assert_eq!(stack.max_depth, operations);
    }
}
