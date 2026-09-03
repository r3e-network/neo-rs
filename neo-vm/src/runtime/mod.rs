//! Shared NeoVM execution context used by host-specific runtimes.

pub mod ops;
mod pending_exception;
mod try_frame;

use alloc::{format, string::String, string::ToString, vec, vec::Vec};
pub(crate) use pending_exception::{PendingException, PendingExceptionValue};
use try_frame::TryFrame;

use crate::{ExecutionResult, StackValue, VmState};

/// Minimal stack/fault interface required by shared runtime opcode adapters.
pub trait RuntimeStack {
    /// Pop one value from the evaluation stack.
    fn pop_value(&mut self) -> StackValue;

    /// Push one value to the evaluation stack.
    fn push_value(&mut self, value: StackValue);

    /// Borrow the current top value mutably for in-place collection opcodes.
    fn top_value_mut(&mut self) -> Option<&mut StackValue>;

    /// Borrow the full evaluation stack.
    fn stack_values(&self) -> &[StackValue];

    /// Borrow the full evaluation stack mutably.
    fn stack_values_mut(&mut self) -> &mut Vec<StackValue>;

    /// Put the runtime into a faulted state.
    fn fault(&mut self, message: &str);

    /// Pop an integer-compatible value that fits in `i64`.
    ///
    /// This intentionally preserves the existing compiled-runtime behavior:
    /// invalid generated stacks are programmer/runtime faults and panic in the
    /// same way the previous per-runtime wrappers did.
    fn pop_i64(&mut self) -> i64 {
        let value = self.pop_value();
        value
            .to_i128()
            .and_then(|integer| i64::try_from(integer).ok())
            .unwrap_or_else(|| {
                panic!(
                    "expected integer-compatible StackValue fitting i64, got {:?}",
                    value
                )
            })
    }

    /// Push a compact integer result.
    fn push_i64(&mut self, value: i64) {
        self.push_value(StackValue::Integer(value));
    }

    /// Push a boolean result.
    fn push_bool(&mut self, value: bool) {
        self.push_value(StackValue::Boolean(value));
    }
}

pub(crate) fn push_value_result(
    runtime: &mut (impl RuntimeStack + ?Sized),
    result: Result<StackValue, String>,
) {
    match result {
        Ok(value) => runtime.push_value(value),
        Err(message) => runtime.fault(&message),
    }
}

pub(crate) fn push_i64_result(
    runtime: &mut (impl RuntimeStack + ?Sized),
    result: Result<i64, String>,
) {
    match result {
        Ok(value) => runtime.push_i64(value),
        Err(message) => runtime.fault(&message),
    }
}

pub(crate) fn push_bool_result(
    runtime: &mut (impl RuntimeStack + ?Sized),
    result: Result<bool, String>,
) {
    match result {
        Ok(value) => runtime.push_bool(value),
        Err(message) => runtime.fault(&message),
    }
}

pub(crate) fn push_values_result(
    runtime: &mut (impl RuntimeStack + ?Sized),
    result: Result<Vec<StackValue>, String>,
) {
    match result {
        Ok(values) => {
            for value in values {
                runtime.push_value(value);
            }
        }
        Err(message) => runtime.fault(&message),
    }
}

/// Common NeoVM execution state shared by native, RISC-V, and proving runtimes.
pub struct VmContext {
    /// Evaluation stack.
    pub stack: Vec<StackValue>,
    /// Local variable slots.
    pub locals: Vec<StackValue>,
    /// Argument slots.
    pub args: Vec<StackValue>,
    /// Contract-level static fields.
    pub static_fields: Vec<StackValue>,
    /// Fault message, set when the VM aborts.
    pub fault_message: Option<String>,
    /// Current execution state.
    pub state: VmState,
    try_stack: Vec<TryFrame>,
    pending_error: Option<PendingException<StackValue>>,
    call_stack: Vec<i32>,
}

impl VmContext {
    /// Creates a context from an initial evaluation stack.
    #[must_use]
    pub fn from_stack(stack: Vec<StackValue>) -> Self {
        Self {
            stack,
            locals: Vec::new(),
            args: Vec::new(),
            static_fields: Vec::new(),
            fault_message: None,
            state: VmState::Halt,
            try_stack: Vec::new(),
            pending_error: None,
            call_stack: Vec::new(),
        }
    }

    /// Initializes local and argument slots.
    pub fn init_slot(&mut self, local_count: usize, arg_count: usize) {
        self.locals = vec![StackValue::Null; local_count];
        self.args = Vec::with_capacity(arg_count);
        for _ in 0..arg_count {
            let value = self.pop_value();
            self.args.push(value);
        }
        self.args.reverse();
    }

    /// Initializes static fields for NeoVM INITSSLOT.
    pub fn init_sslot(&mut self, count: usize) {
        if self.static_fields.len() < count {
            self.static_fields.resize(count, StackValue::Null);
        }
    }

    /// Sets fault state and records the user-facing message.
    pub fn fault(&mut self, message: &str) {
        self.state = VmState::Fault;
        self.fault_message = Some(message.to_string());
    }

    /// Returns true when the context is faulted.
    #[must_use]
    pub fn is_faulted(&self) -> bool {
        self.state == VmState::Fault
    }

    /// Converts this context into an execution result.
    #[must_use]
    pub fn into_execution_result(self, fee_consumed_pico: i64) -> ExecutionResult {
        ExecutionResult {
            fee_consumed_pico,
            state: self.state,
            stack: self.stack,
            fault_message: self.fault_message,
            fault_ip: None,
            fault_locals: None,
        }
    }

    /// Pushes a value onto the evaluation stack.
    pub fn push(&mut self, value: StackValue) {
        self.push_value(value);
    }

    /// Pushes an integer onto the evaluation stack.
    pub fn push_int(&mut self, value: i64) {
        self.push_value(StackValue::Integer(value));
    }

    /// Pushes a boolean onto the evaluation stack.
    pub fn push_bool(&mut self, value: bool) {
        self.push_value(StackValue::Boolean(value));
    }

    /// Pushes a byte string onto the evaluation stack.
    pub fn push_bytes(&mut self, value: &[u8]) {
        self.push_value(StackValue::ByteString(value.to_vec()));
    }

    /// Pushes null onto the evaluation stack.
    pub fn push_null(&mut self) {
        self.push_value(StackValue::Null);
    }

    /// Pops the top value from the evaluation stack.
    #[must_use]
    pub fn pop(&mut self) -> StackValue {
        self.pop_value()
    }

    /// Pushes the value of argument slot `index`.
    pub fn load_arg(&mut self, index: usize) {
        self.push_value(self.args[index].clone());
    }

    /// Stores the top value into argument slot `index`.
    pub fn store_arg(&mut self, index: usize) {
        let value = self.pop_value();
        self.args[index] = value;
    }

    /// Pushes the value of local slot `index`.
    pub fn load_local(&mut self, index: usize) {
        self.push_value(self.locals[index].clone());
    }

    /// Stores the top value into local slot `index`.
    pub fn store_local(&mut self, index: usize) {
        let value = self.pop_value();
        self.locals[index] = value;
    }

    /// Pushes the value of static field `index`.
    pub fn load_static(&mut self, index: usize) {
        if index >= self.static_fields.len() {
            self.fault("invalid static field index");
            return;
        }
        self.push_value(self.static_fields[index].clone());
    }

    /// Stores the top value into static field `index`.
    pub fn store_static(&mut self, index: usize) {
        if index >= self.static_fields.len() {
            self.fault("invalid static field index");
            return;
        }
        let value = self.pop_value();
        self.static_fields[index] = value;
    }

    /// Throws an exception with the top stack value as payload.
    pub fn throw_ex(&mut self) {
        let value = self.pop_value();
        if self.try_stack.iter().any(|frame| !frame.caught) {
            self.pending_error = Some(PendingException::thrown_value(value));
        } else {
            let message = match &value {
                StackValue::ByteString(bytes) => String::from_utf8_lossy(bytes).to_string(),
                StackValue::Integer(value) => format!("exception: {value}"),
                _ => format!("exception: {:?}", value),
            };
            self.fault(&message);
        }
    }

    /// Immediately faults the VM, or records pending error inside a try frame.
    pub fn abort(&mut self) {
        if self.try_stack.iter().any(|frame| !frame.caught) {
            self.pending_error = Some(PendingException::message("ABORT".to_string()));
        } else {
            self.fault("ABORT");
        }
    }

    /// Faults the VM with a message from the top stack value.
    pub fn abort_msg(&mut self) {
        let value = self.pop_value();
        let message = match &value {
            StackValue::ByteString(bytes) => {
                format!("ABORTMSG: {}", String::from_utf8_lossy(bytes))
            }
            _ => format!("ABORTMSG: {:?}", value),
        };
        if self.try_stack.iter().any(|frame| !frame.caught) {
            self.pending_error = Some(PendingException::message(message));
        } else {
            self.fault(&message);
        }
    }

    /// Pops a boolean-like value and faults when it is false.
    pub fn assert_top(&mut self) {
        let is_true = self.pop_value().to_bool();
        if !is_true {
            if self.try_stack.iter().any(|frame| !frame.caught) {
                self.pending_error = Some(PendingException::message(
                    "ASSERT: assertion failed".to_string(),
                ));
            } else {
                self.fault("ASSERT: assertion failed");
            }
        }
    }

    /// Pops a message then a boolean-like value and faults when it is false.
    pub fn assert_msg(&mut self) {
        let message_value = self.pop_value();
        let condition_value = self.pop_value();
        if condition_value.to_bool() {
            return;
        }

        let message = match &message_value {
            StackValue::ByteString(bytes) => {
                format!("ASSERTMSG: {}", String::from_utf8_lossy(bytes))
            }
            _ => format!("ASSERTMSG: {:?}", message_value),
        };
        if self.try_stack.iter().any(|frame| !frame.caught) {
            self.pending_error = Some(PendingException::message(message));
        } else {
            self.fault(&message);
        }
    }

    /// Enters a try block with catch/finally offsets.
    pub fn try_enter(&mut self, catch_pc: i32, finally_pc: i32) {
        self.try_stack.push(TryFrame {
            catch_pc,
            finally_pc,
            caught: false,
            in_finally: false,
            end_pc: 0,
        });
    }

    /// Ends a try/catch block and returns the next program counter.
    #[must_use]
    pub fn end_try(&mut self, target_pc: i32) -> i32 {
        if let Some(frame) = self.try_stack.last_mut() {
            if frame.finally_pc != 0 && !frame.in_finally {
                frame.end_pc = target_pc;
                frame.in_finally = true;
                return frame.finally_pc;
            }
        }
        self.try_stack.pop();
        target_pc
    }

    /// Ends a finally block and returns the continuation program counter.
    #[must_use]
    pub fn end_finally(&mut self) -> i32 {
        if let Some(frame) = self.try_stack.pop() {
            if frame.in_finally {
                if self.pending_error.is_some() {
                    return -1;
                }
                if frame.end_pc != 0 {
                    return frame.end_pc;
                }
            }
        }
        self.fault("ENDFINALLY without matching finally context");
        -1
    }

    /// Handles a pending exception through the try stack.
    #[must_use]
    pub fn check_exception(&mut self) -> Option<i32> {
        let error = self.pending_error.take()?;
        let frame_index = self.try_stack.iter().rposition(|frame| !frame.caught)?;
        let frame = &mut self.try_stack[frame_index];
        frame.caught = true;
        let catch_pc = frame.catch_pc;
        let finally_pc = frame.finally_pc;
        if catch_pc != 0 {
            self.push_value(error.into_catch_item());
            Some(catch_pc)
        } else if finally_pc != 0 {
            self.try_stack[frame_index].in_finally = true;
            self.pending_error = Some(error);
            Some(finally_pc)
        } else {
            self.fault(&error.fault_message());
            None
        }
    }

    /// Pushes a return address onto the call stack.
    pub fn call_push(&mut self, return_pc: i32) {
        self.call_stack.push(return_pc);
    }

    /// Pops a return address from the call stack.
    #[must_use]
    pub fn call_pop(&mut self) -> Option<i32> {
        self.call_stack.pop()
    }

    /// Alias for generated RET handling.
    #[must_use]
    pub fn ret(&mut self) -> Option<i32> {
        self.call_pop()
    }
}

impl RuntimeStack for VmContext {
    fn pop_value(&mut self) -> StackValue {
        self.stack
            .pop()
            .expect("stack underflow: pop on empty stack")
    }

    fn push_value(&mut self, value: StackValue) {
        self.stack.push(value);
    }

    fn top_value_mut(&mut self) -> Option<&mut StackValue> {
        self.stack.last_mut()
    }

    fn stack_values(&self) -> &[StackValue] {
        &self.stack
    }

    fn stack_values_mut(&mut self) -> &mut Vec<StackValue> {
        &mut self.stack
    }

    fn fault(&mut self, message: &str) {
        VmContext::fault(self, message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_slot_preserves_argument_order() {
        let mut ctx = VmContext::from_stack(vec![
            StackValue::Integer(100),
            StackValue::Integer(200),
            StackValue::Integer(300),
        ]);

        ctx.init_slot(2, 2);

        assert_eq!(
            ctx.args,
            vec![StackValue::Integer(200), StackValue::Integer(300)]
        );
        assert_eq!(ctx.locals, vec![StackValue::Null, StackValue::Null]);
        assert_eq!(ctx.stack, vec![StackValue::Integer(100)]);
    }

    #[test]
    fn static_access_faults_out_of_range() {
        let mut ctx = VmContext::from_stack(vec![StackValue::Integer(1)]);

        ctx.store_static(0);

        assert_eq!(ctx.state, VmState::Fault);
        assert_eq!(
            ctx.fault_message.as_deref(),
            Some("invalid static field index")
        );
    }
}
