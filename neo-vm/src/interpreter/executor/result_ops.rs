use super::super::runtime_types::{StackValue, discard_values, release_values, to_abi_stack};
use super::super::state::{
    LAST_RESULT_LIMIT, LAST_RESULT_STACK_LEN, LAST_RESULT_STAGE, PendingException,
};
use crate::{ExecutionResult, VmState};
use alloc::{string::String, vec, vec::Vec};
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
fn discard_pending_error(pending_error: Option<PendingException>) {
    if cfg!(target_arch = "riscv32") {
        core::mem::forget(pending_error);
    } else if let Some(PendingException::ThrownValue(value)) = pending_error {
        release_values(vec![value]);
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

    let old_stack = core::mem::take(stack);
    if keep == 0 {
        discard_values(old_stack);
    } else if old_stack.len() > keep {
        let start = old_stack.len() - keep;
        let mut kept = Vec::with_capacity(keep);
        for item in old_stack.iter().skip(start) {
            kept.push(item.clone());
        }
        *stack = kept;
        discard_values(old_stack);
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

    discard_values(stack);
    discard_values(locals);
    discard_values(args);
    discard_values(static_fields);
    discard_values(method_initial_stack);
    discard_values(consumed_mutations);
    discard_pending_error(pending_error);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StackValue as AbiStackValue;

    fn halt(stack: Vec<StackValue>, result_stack_limit: Option<usize>) -> ExecutionResult {
        finish_halt_result(
            stack,
            vec![StackValue::ByteString(vec![1; 8])],
            vec![StackValue::Buffer(1, vec![2; 8])],
            vec![StackValue::Array(2, vec![StackValue::Integer(3)])],
            vec![StackValue::Null],
            vec![StackValue::Map(
                3,
                vec![(StackValue::Integer(4), StackValue::Boolean(true))],
            )],
            Some(PendingException::thrown_value(StackValue::Integer(5))),
            result_stack_limit,
        )
    }

    fn sample_stack() -> Vec<StackValue> {
        vec![
            StackValue::Integer(1),
            StackValue::Array(10, vec![StackValue::ByteString(vec![0xaa])]),
            StackValue::Struct(11, vec![StackValue::Boolean(true)]),
            StackValue::Map(12, vec![(StackValue::Integer(2), StackValue::Null)]),
        ]
    }

    fn sample_abi_stack() -> Vec<AbiStackValue> {
        vec![
            AbiStackValue::Integer(1),
            AbiStackValue::Array(vec![AbiStackValue::ByteString(vec![0xaa])]),
            AbiStackValue::Struct(vec![AbiStackValue::Boolean(true)]),
            AbiStackValue::Map(vec![(AbiStackValue::Integer(2), AbiStackValue::Null)]),
        ]
    }

    /// Builds `depth` levels of nesting without recursing.
    fn nested(depth: usize, wrap: fn(u64, StackValue) -> StackValue) -> StackValue {
        let mut value = StackValue::Null;
        for id in 0..depth as u64 {
            value = wrap(id, value);
        }
        value
    }

    #[test]
    fn halt_returns_the_full_stack_without_a_result_limit() {
        let result = halt(sample_stack(), None);

        assert_eq!(result.state, VmState::Halt);
        assert_eq!(result.stack, sample_abi_stack());
        assert_eq!(result.fee_consumed_pico, 0);
        assert_eq!(result.fault_message, None);
        assert_eq!(result.fault_ip, None);
        assert_eq!(result.fault_locals, None);
    }

    #[test]
    fn halt_keeps_only_the_top_items_within_the_result_limit() {
        let expected = sample_abi_stack();

        for limit in 0..=expected.len() + 1 {
            let result = halt(sample_stack(), Some(limit));
            let kept = limit.min(expected.len());

            assert_eq!(result.state, VmState::Halt, "limit {limit}");
            assert_eq!(
                result.stack,
                expected[expected.len() - kept..],
                "limit {limit}"
            );
        }
    }

    #[test]
    fn trim_keeps_the_top_of_the_stack_in_order() {
        let mut stack = sample_stack();
        trim_halt_stack_for_result_limit(&mut stack, Some(2));
        assert_eq!(stack, sample_stack()[2..]);

        let mut stack = sample_stack();
        trim_halt_stack_for_result_limit(&mut stack, Some(0));
        assert!(stack.is_empty());

        let mut stack = sample_stack();
        trim_halt_stack_for_result_limit(&mut stack, Some(4));
        assert_eq!(stack, sample_stack());

        let mut stack = sample_stack();
        trim_halt_stack_for_result_limit(&mut stack, None);
        assert_eq!(stack, sample_stack());
    }

    #[test]
    fn halt_releases_deeply_nested_state_without_recursion() {
        const DEPTH: usize = 100_000;
        // Far too small for one native frame per nesting level.
        const THREAD_STACK_BYTES: usize = 256 * 1024;

        let array = |id, inner| StackValue::Array(id, vec![inner]);
        let r#struct = |id, inner| StackValue::Struct(id, vec![inner]);
        let map_value = |id, inner| StackValue::Map(id, vec![(StackValue::Integer(0), inner)]);
        let map_key = |id, inner| StackValue::Map(id, vec![(inner, StackValue::Integer(0))]);

        let result = std::thread::Builder::new()
            .stack_size(THREAD_STACK_BYTES)
            .spawn(move || {
                finish_halt_result(
                    vec![nested(DEPTH, array), StackValue::Integer(7)],
                    vec![nested(DEPTH, r#struct)],
                    vec![nested(DEPTH, map_value)],
                    vec![nested(DEPTH, map_key)],
                    vec![nested(DEPTH, array)],
                    vec![nested(DEPTH, r#struct)],
                    Some(PendingException::thrown_value(nested(DEPTH, array))),
                    Some(1),
                )
            })
            .expect("spawn small-stack thread")
            .join()
            .expect("halt teardown completes");

        assert_eq!(result.state, VmState::Halt);
        assert_eq!(result.stack, vec![AbiStackValue::Integer(7)]);
    }
}
