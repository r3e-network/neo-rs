//! Fixed opcode-handler table and hot dispatch accessors.
//!
//! This module owns the raw 256-entry handler array, unchecked hot-path table
//! access, and production dispatch error path. Hardfork/default table
//! construction stays in `variants`.

use crate::error::{VmError, VmResult};
use crate::execution_engine::ExecutionEngine;
use neo_vm_rs::{Instruction, OpCode};

/// A handler for a VM instruction.
pub type InstructionHandler = fn(&mut ExecutionEngine, &Instruction) -> VmResult<()>;

/// Represents a jump table for the VM.
#[derive(Clone)]
pub struct JumpTable {
    /// The handlers for each opcode.
    /// Uses a fixed-size array of 256 entries (one for each possible byte value)
    /// exactly matching the C# implementation which uses `DelAction`[] Table = new `DelAction`[byte.MaxValue]
    ///
    /// This field is public to allow direct access for performance-critical
    /// instruction dispatch in the execution loop.
    pub(crate) handlers: [Option<InstructionHandler>; 256],
}

impl JumpTable {
    pub(super) fn empty() -> Self {
        Self {
            handlers: [None; 256],
        }
    }

    /// Registers a handler for an opcode.
    pub fn register(&mut self, opcode: OpCode, handler: InstructionHandler) {
        self.set_handler(opcode, handler);
    }

    /// Gets the handler for an opcode.
    #[must_use]
    pub fn get(&self, opcode: OpCode) -> Option<InstructionHandler> {
        self.get_handler(opcode)
    }

    /// Gets the handler for an opcode.
    /// This matches the C# implementation's indexer get accessor.
    // Rationale: opcode handlers are stored in a fixed 256-entry table and the
    // u8 opcode value proves the unchecked index bound.
    #[allow(unsafe_code)]
    #[inline(always)]
    #[must_use]
    pub fn get_handler(&self, opcode: OpCode) -> Option<InstructionHandler> {
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
    pub fn get_handler_by_u8(&self, opcode_byte: u8) -> Option<InstructionHandler> {
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
    pub fn set_handler(&mut self, opcode: OpCode, handler: InstructionHandler) {
        let idx = usize::from(opcode.byte());
        debug_assert!(idx < self.handlers.len());
        // SAFETY: OpCode::byte() returns a u8 (0..=255) and handlers has 256 entries.
        unsafe {
            *self.handlers.get_unchecked_mut(idx) = Some(handler);
        }
    }

    /// Sets the handler for an opcode.
    /// Alias for `set_handler` for convenience.
    pub fn set(&mut self, opcode: OpCode, handler: InstructionHandler) {
        self.set_handler(opcode, handler);
    }

    /// Executes an instruction.
    pub fn execute(&self, engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
        if let Some(handler) = self.get_handler(instruction.opcode()) {
            handler(engine, instruction)
        } else {
            self.invalid_opcode(engine, instruction)
        }
    }

    /// Handles an invalid opcode.
    pub fn invalid_opcode(
        &self,
        _engine: &mut ExecutionEngine,
        instruction: &Instruction,
    ) -> VmResult<()> {
        Err(VmError::unsupported_operation(format!(
            "Unsupported opcode: {:?}",
            instruction.opcode()
        )))
    }
}

impl std::ops::Index<OpCode> for JumpTable {
    type Output = InstructionHandler;

    /// # Panics
    ///
    /// Panics if no handler is registered for `opcode`. Production code should
    /// use [`JumpTable::execute`] instead, which returns a `VmResult`.
    // Rationale: the `Index` trait cannot return `VmResult`; unsafe indexing is
    // confined to the fixed opcode table and production dispatch uses `execute`.
    #[allow(unsafe_code)]
    #[inline]
    fn index(&self, opcode: OpCode) -> &Self::Output {
        let idx = usize::from(opcode.byte());
        debug_assert!(idx < self.handlers.len());
        // SAFETY: OpCode::byte() returns a u8 (0..=255) and handlers has 256 entries.
        // The Option::expect is acceptable here because Index must return a reference
        // and cannot return Result; the execute() method is the safe alternative.
        unsafe {
            self.handlers
                .get_unchecked(idx)
                .as_ref()
                .expect("No handler registered for opcode; use JumpTable::execute() in production")
        }
    }
}

impl std::ops::IndexMut<OpCode> for JumpTable {
    // Rationale: mutable index access is retained for VM table setup; fixed
    // opcode byte bounds protect the unchecked table access.
    #[allow(unsafe_code)]
    #[inline]
    fn index_mut(&mut self, opcode: OpCode) -> &mut Self::Output {
        let idx = usize::from(opcode.byte());
        debug_assert!(idx < self.handlers.len());
        // SAFETY: OpCode::byte() returns a u8 (0..=255) and handlers has 256 entries.
        unsafe {
            if self.handlers.get_unchecked(idx).is_none() {
                *self.handlers.get_unchecked_mut(idx) = Some(
                    |_engine: &mut ExecutionEngine, instruction: &Instruction| -> VmResult<()> {
                        Err(VmError::unsupported_operation(format!(
                            "Unsupported opcode: {:?}",
                            instruction.opcode()
                        )))
                    },
                );
            }

            // SAFETY: The branch above guarantees the slot is now Some, so
            // this unwrap is infallible.
            self.handlers
                .get_unchecked_mut(idx)
                .as_mut()
                .expect("slot was just initialised to Some")
        }
    }
}
