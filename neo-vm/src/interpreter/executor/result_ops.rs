use super::super::runtime_types::{to_abi_stack, StackValue};
use super::super::state::{
    PendingException, LAST_RESULT_LIMIT, LAST_RESULT_STACK_LEN, LAST_RESULT_STAGE,
};
use crate::{ExecutionResult, VmState};
use alloc::{string::String, vec::Vec};
use core::sync::atomic::Ordering;

pub(super) fn fault_result(
    stack: &[StackValue],
    locals: &[StackValue],
    ip: usize,
    fault_message: String,
) -> ExecutionResult {
    ExecutionResult {
        fee_consumed_pico: 0,
        state: VmState::Fault,
        stack: to_abi_stack(stack),
        fault_message: Some(fault_message),
        fault_ip: Some(ip as u32),
        fault_locals: Some(crate::abi::fast_codec::encode_stack(&to_abi_stack(locals))),
    }
}

#[inline]
pub(super) fn trim_halt_stack_for_result_limit(
    stack: &mut Vec<StackValue>,
    result_stack_limit: Option<usize>,
) {
    let Some(keep) = result_stack_limit else {
        return;
    };

    // The guest uses a per-execution bump allocator. Forgetting discarded
    // return-stack debris avoids recursive Drop on deep historical compound
    // values; the whole arena is reset before the next execution.
    let old_stack = core::mem::take(stack);
    if keep == 0 {
        core::mem::forget(old_stack);
    } else if old_stack.len() > keep {
        let start = old_stack.len() - keep;
        let mut kept = Vec::with_capacity(keep);
        for item in old_stack.iter().skip(start) {
            kept.push(item.clone());
        }
        *stack = kept;
        core::mem::forget(old_stack);
    } else {
        *stack = old_stack;
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn finish_halt_result(
    mut stack: Vec<StackValue>,
    locals: Vec<StackValue>,
    args: Vec<StackValue>,
    static_fields: Vec<StackValue>,
    method_initial_stack: Vec<StackValue>,
    consumed_mutations: Vec<StackValue>,
    pending_error: Option<PendingException>,
    result_stack_limit: Option<usize>,
) -> ExecutionResult {
    LAST_RESULT_STAGE.store(1, Ordering::Relaxed);
    LAST_RESULT_STACK_LEN.store(stack.len().min(u32::MAX as usize) as u32, Ordering::Relaxed);
    LAST_RESULT_LIMIT.store(
        result_stack_limit
            .unwrap_or(usize::MAX)
            .min(u32::MAX as usize) as u32,
        Ordering::Relaxed,
    );
    trim_halt_stack_for_result_limit(&mut stack, result_stack_limit);
    LAST_RESULT_STAGE.store(2, Ordering::Relaxed);
    LAST_RESULT_STACK_LEN.store(stack.len().min(u32::MAX as usize) as u32, Ordering::Relaxed);
    let abi_stack = to_abi_stack(&stack);
    LAST_RESULT_STAGE.store(3, Ordering::Relaxed);

    // On the guest path these vectors live in a per-execution arena. Forgetting
    // avoids recursive drops for deep historical compound values; the arena is
    // reset before the next execution.
    core::mem::forget(stack);
    core::mem::forget(locals);
    core::mem::forget(args);
    core::mem::forget(static_fields);
    core::mem::forget(method_initial_stack);
    core::mem::forget(consumed_mutations);
    core::mem::forget(pending_error);
    LAST_RESULT_STAGE.store(4, Ordering::Relaxed);

    ExecutionResult {
        fee_consumed_pico: 0,
        state: VmState::Halt,
        stack: abi_stack,
        fault_message: None,
        fault_ip: None,
        fault_locals: None,
    }
}
