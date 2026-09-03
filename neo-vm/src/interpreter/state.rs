mod call_frame;
mod call_stack;
mod try_frame;
mod try_stack;

use super::runtime_types::{compound_id, propagate_aliases_from_sources, StackValue};
pub(super) use call_stack::CallStack;
pub(super) use try_frame::TryFrame;
pub(super) use try_stack::TryStack;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

pub(super) type PendingException = crate::runtime::PendingException<StackValue>;

pub(super) static LAST_INTERPRETER_IP: AtomicU32 = AtomicU32::new(u32::MAX);
pub(super) static LAST_RESULT_STAGE: AtomicU32 = AtomicU32::new(0);
pub(super) static LAST_RESULT_STACK_LEN: AtomicU32 = AtomicU32::new(0);
pub(super) static LAST_RESULT_LIMIT: AtomicU32 = AtomicU32::new(u32::MAX);

#[inline]
pub(super) fn record_interpreter_ip(ip: usize) {
    let value = if ip <= u32::MAX as usize {
        ip as u32
    } else {
        u32::MAX
    };
    LAST_INTERPRETER_IP.store(value, Ordering::Relaxed);
}

pub fn last_interpreter_ip() -> u32 {
    LAST_INTERPRETER_IP.load(Ordering::Relaxed)
}

pub fn last_result_stage() -> u32 {
    LAST_RESULT_STAGE.load(Ordering::Relaxed)
}

pub fn last_result_stack_len() -> u32 {
    LAST_RESULT_STACK_LEN.load(Ordering::Relaxed)
}

pub fn last_result_limit() -> u32 {
    LAST_RESULT_LIMIT.load(Ordering::Relaxed)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn propagate_active_aliases_into_saved_frame(
    saved_locals: &mut [StackValue],
    saved_args: &mut [StackValue],
    stack: &[StackValue],
    locals: &[StackValue],
    args: &[StackValue],
    static_fields: &[StackValue],
    consumed_mutations: &[StackValue],
) {
    for targets in [saved_locals, saved_args] {
        propagate_aliases_from_sources(targets, stack);
        propagate_aliases_from_sources(targets, locals);
        propagate_aliases_from_sources(targets, args);
        propagate_aliases_from_sources(targets, static_fields);
        propagate_aliases_from_sources(targets, consumed_mutations);
    }
}

pub(super) fn remember_consumed_mutation(
    consumed_mutations: &mut Vec<StackValue>,
    updated: &StackValue,
) {
    let Some(id) = compound_id(updated) else {
        return;
    };

    if let Some(existing) = consumed_mutations
        .iter_mut()
        .find(|value| compound_id(value) == Some(id))
    {
        *existing = updated.clone();
    } else {
        consumed_mutations.push(updated.clone());
    }
}

#[inline]
pub(super) fn reset_consumed_mutations(consumed_mutations: &mut Vec<StackValue>) {
    if cfg!(target_arch = "riscv32") {
        // Avoid walking and dropping potentially large alias-tracking values on
        // the PolkaVM/riscv32 path. The guest allocator is reset per execution.
        unsafe {
            core::ptr::write(consumed_mutations, Vec::new());
        }
    } else {
        consumed_mutations.clear();
    }
}

/// Fixed-capacity stack for TryFrames — avoids heap allocation to prevent
/// PolkaVM bump allocator corruption during host_call round-trips.
pub(super) const MAX_STACK_SIZE: usize = 2048;
pub(super) const MAX_TRY_NESTING: usize = 16;
pub(super) const MAX_CALL_DEPTH: usize = 64;
