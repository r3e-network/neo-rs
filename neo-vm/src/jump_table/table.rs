//! Fixed opcode-handler table and hot dispatch accessors.
//!
//! This module owns the raw 256-entry handler array, unchecked hot-path table
//! access, and production dispatch error path. Hardfork/default table
//! construction stays in `variants`.

use crate::error::{VmError, VmResult};
use crate::execution_engine::ExecutionEngine;
use crate::{Instruction, OpCode};

/// A handler for a VM instruction.
pub type InstructionHandler<S = ()> = fn(&mut ExecutionEngine<S>, &Instruction) -> VmResult<()>;

fn unsupported_opcode_handler<S>(
    _engine: &mut ExecutionEngine<S>,
    instruction: &Instruction,
) -> VmResult<()> {
    Err(VmError::unsupported_operation(format!(
        "Unsupported opcode: {:?}",
        instruction.opcode()
    )))
}

/// Represents a jump table for the VM.
#[derive(Clone)]
pub struct JumpTable<S = ()> {
    /// The handlers for each opcode.
    /// Uses a fixed-size array of 256 entries (one for each possible byte value)
    /// exactly matching the C# implementation which uses `DelAction`[] Table = new `DelAction`[byte.MaxValue]
    ///
    /// This field is public to allow direct access for performance-critical
    /// instruction dispatch in the execution loop.
    pub(crate) handlers: [Option<InstructionHandler<S>>; 256],
}

impl<S> JumpTable<S> {
    pub(super) fn empty() -> Self {
        Self {
            handlers: [None; 256],
        }
    }

    /// Registers a handler for an opcode.
    pub fn register(&mut self, opcode: OpCode, handler: InstructionHandler<S>) {
        self.set_handler(opcode, handler);
    }

    /// Gets the handler for an opcode.
    #[must_use]
    pub fn get(&self, opcode: OpCode) -> Option<InstructionHandler<S>> {
        self.get_handler(opcode)
    }

    /// Gets the handler for an opcode.
    /// This matches the C# implementation's indexer get accessor.
    // Rationale: opcode handlers are stored in a fixed 256-entry table and the
    // u8 opcode value proves the unchecked index bound.
    #[allow(unsafe_code)]
    #[inline(always)]
    #[must_use]
    pub fn get_handler(&self, opcode: OpCode) -> Option<InstructionHandler<S>> {
        let idx = usize::from(opcode.byte());
        debug_assert!(idx < self.handlers.len());
        // SAFETY: OpCode::byte() returns a u8 (0..=255) and handlers has 256 entries.
        unsafe { *self.handlers.get_unchecked(idx) }
    }

    /// Gets the handler for a raw `u8` opcode value.
    ///
    /// This is used in the hot execution loop where the opcode is already a `u8`.
    /// The `debug_assert` catches out-of-bounds access in debug builds while
    /// maintaining zero overhead in release builds.
    // Rationale: raw opcode dispatch is the hottest VM path; u8 input proves
    // the fixed-table bound without an extra release check.
    #[allow(unsafe_code)]
    #[inline(always)]
    #[must_use]
    pub fn get_handler_by_u8(&self, opcode_byte: u8) -> Option<InstructionHandler<S>> {
        let idx = usize::from(opcode_byte);
        debug_assert!(idx < self.handlers.len());
        // SAFETY: opcode_byte is u8 (0..=255) and handlers has exactly 256 entries,
        // so the index is always in bounds.
        unsafe { *self.handlers.get_unchecked(idx) }
    }

    /// Sets the handler for an opcode.
    /// This matches the C# implementation's indexer set accessor.
    // Rationale: opcode handlers are stored in a fixed 256-entry table and the
    // u8 opcode value proves the unchecked index bound.
    #[allow(unsafe_code)]
    #[inline]
    pub fn set_handler(&mut self, opcode: OpCode, handler: InstructionHandler<S>) {
        let idx = usize::from(opcode.byte());
        debug_assert!(idx < self.handlers.len());
        // SAFETY: OpCode::byte() returns a u8 (0..=255) and handlers has 256 entries.
        unsafe {
            *self.handlers.get_unchecked_mut(idx) = Some(handler);
        }
    }

    /// Sets the handler for an opcode.
    /// Alias for `set_handler` for convenience.
    pub fn set(&mut self, opcode: OpCode, handler: InstructionHandler<S>) {
        self.set_handler(opcode, handler);
    }

    /// Executes an instruction.
    pub fn execute(
        &self,
        engine: &mut ExecutionEngine<S>,
        instruction: &Instruction,
    ) -> VmResult<()> {
        let handler = self
            .get_handler(instruction.opcode())
            .unwrap_or(unsupported_opcode_handler::<S>);
        handler(engine, instruction)
    }

    /// Handles an invalid opcode.
    pub fn invalid_opcode(
        &self,
        _engine: &mut ExecutionEngine<S>,
        instruction: &Instruction,
    ) -> VmResult<()> {
        Err(VmError::unsupported_operation(format!(
            "Unsupported opcode: {:?}",
            instruction.opcode()
        )))
    }
}
