//! Jump table module for the Neo Virtual Machine.
//!
//! This module provides the jump table implementation used in the Neo VM.

pub mod bitwisee; // Matches JumpTable.Bitwisee.cs
pub mod compound; // Matches JumpTable.Compound.cs
pub mod control; // Matches JumpTable.Control.cs
pub mod numeric; // Matches JumpTable.Numeric.cs
pub mod push; // Matches JumpTable.Push.cs
pub mod slot; // Matches JumpTable.Slot.cs
pub mod splice; // Matches JumpTable.Splice.cs
pub mod stack; // Matches JumpTable.Stack.cs
pub mod types; // Matches JumpTable.Types.cs

use crate::error::VmError;
use crate::error::VmResult;
use crate::execution_engine::ExecutionEngine;
use neo_vm_rs::Instruction;
use neo_vm_rs::OpCode;

/// A handler for a VM instruction.
pub type InstructionHandler = fn(&mut ExecutionEngine, &Instruction) -> VmResult<()>;

macro_rules! register_jump_handlers {
    ($jump_table:expr_2021; $($opcode:expr_2021 => $handler:expr_2021),+ $(,)?) => {
        $(
            $jump_table.register($opcode, $handler);
        )+
    };
}

pub(crate) use register_jump_handlers;

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

impl Default for JumpTable {
    fn default() -> Self {
        Self::new()
    }
}

use std::sync::OnceLock;

/// The default jump table.
static DEFAULT: OnceLock<JumpTable> = OnceLock::new();

/// The pre-`HF_Gorgon` jump table (cached).
static NOT_GORGON: OnceLock<JumpTable> = OnceLock::new();

impl JumpTable {
    /// Creates a new jump table.
    #[must_use]
    pub fn new() -> Self {
        let mut jump_table = Self {
            handlers: [None; 256],
        };

        // Register default handlers
        jump_table.register_default_handlers();

        jump_table
    }

    /// Gets the default jump table.
    #[allow(clippy::should_implement_trait)]
    pub fn default() -> Self {
        // Use OnceLock for safe one-time initialization
        DEFAULT.get_or_init(Self::new).clone()
    }

    /// The pre-`HF_Gorgon` jump table (C# `ApplicationEngine.ComposeNotGorgonJumpTable`):
    /// the default table with the pre-543 compound handlers (HASKEY/PICKITEM/
    /// SETITEM/REMOVE) and the vulnerable SHL/SHR restored. Selected for blocks
    /// before HF_Gorgon when HF_Echidna is active.
    pub fn not_gorgon() -> Self {
        NOT_GORGON
            .get_or_init(|| {
                let mut table = Self::new();
                table.set(OpCode::HASKEY, compound::has_key_before543);
                table.set(OpCode::PICKITEM, compound::pick_item_before543);
                table.set(OpCode::SETITEM, compound::set_item_before543);
                table.set(OpCode::REMOVE, compound::remove_before543);
                table.set(OpCode::SHR, numeric::shr_vulnerable);
                table.set(OpCode::SHL, numeric::shl_vulnerable);
                table
            })
            .clone()
    }

    /// The pre-`HF_Echidna` jump table (C# `ApplicationEngine.ComposeNotEchidnaJumpTable`
    /// = NotGorgon + VulnerableSubStr). The pre-Echidna `VulnerableSubStr` is
    /// observably equivalent to the fixed SUBSTR for consensus — both fault
    /// uncatchably on an out-of-range or `i32`-overflowing `index + count` and
    /// produce the same slice for valid inputs (the original difference was an
    /// uninitialized-memory read a memory-safe VM cannot reproduce) — so the
    /// NotGorgon table is reused unchanged.
    pub fn not_echidna() -> Self {
        Self::not_gorgon()
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

    /// Registers the default handlers for all opcodes.
    fn register_default_handlers(&mut self) {
        // Register bitwisee handlers
        bitwisee::register_handlers(self);

        // Register compound handlers
        compound::register_handlers(self);

        // Register control handlers
        control::register_handlers(self);

        // Register numeric handlers
        numeric::register_handlers(self);

        // Register push handlers
        push::register_handlers(self);

        // Register slot handlers
        slot::register_handlers(self);

        // Register splice handlers
        splice::register_handlers(self);

        // Register stack handlers
        stack::register_handlers(self);

        // Register types handlers
        types::register_handlers(self);
    }
}

impl std::ops::Index<OpCode> for JumpTable {
    type Output = InstructionHandler;

    /// # Panics
    ///
    /// Panics if no handler is registered for `opcode`. Production code should
    /// use [`JumpTable::execute`] instead, which returns a `VmResult`.
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

#[cfg(test)]
#[allow(dead_code)]
mod tests {
    use super::*;

    #[test]
    fn test_jump_table_creation() {
        let jump_table = JumpTable::new();

        // Check that all opcodes have handlers
        for opcode in OpCode::ALL {
            assert!(
                jump_table.get(opcode).is_some(),
                "No handler for opcode: {:?}",
                opcode
            );
        }
    }

    #[test]
    fn test_jump_table_register() -> Result<(), Box<dyn std::error::Error>> {
        let mut jump_table = JumpTable::new();

        // Define a custom handler
        fn custom_handler(
            _engine: &mut ExecutionEngine,
            _instruction: &Instruction,
        ) -> VmResult<()> {
            Ok(())
        }

        // Register the custom handler
        jump_table.register(OpCode::NOP, custom_handler);

        // Check that the handler was registered
        assert_eq!(
            jump_table.get(OpCode::NOP).ok_or("Index out of bounds")? as *const () as usize,
            custom_handler as *const () as usize
        );
        Ok(())
    }

    #[test]
    fn test_jump_table_index() -> Result<(), Box<dyn std::error::Error>> {
        let mut jump_table = JumpTable::new();

        // Define a custom handler
        fn custom_handler(
            _engine: &mut ExecutionEngine,
            _instruction: &Instruction,
        ) -> VmResult<()> {
            Ok(())
        }

        // Set the handler using the index operator
        jump_table[OpCode::NOP] = custom_handler;

        // Check that the handler was set
        assert_eq!(
            jump_table.get(OpCode::NOP).ok_or("Index out of bounds")? as *const () as usize,
            custom_handler as *const () as usize
        );
        Ok(())
    }

    #[test]
    fn test_jump_table_default() {
        // Get the default jump table
        let jump_table = JumpTable::default();

        // Check that all opcodes have handlers
        for opcode in OpCode::ALL {
            assert!(
                jump_table.get(opcode).is_some(),
                "No handler for opcode: {:?}",
                opcode
            );
        }
    }

    /// The pre-HF_Gorgon table overrides SHL/SHR + HASKEY/PICKITEM/SETITEM/REMOVE
    /// with the pre-fork handlers, and leaves every other opcode as the default.
    #[test]
    fn not_gorgon_table_overrides_pre_fork_opcodes() {
        let default = JumpTable::default();
        let not_gorgon = JumpTable::not_gorgon();
        let overridden = [
            OpCode::SHL,
            OpCode::SHR,
            OpCode::HASKEY,
            OpCode::PICKITEM,
            OpCode::SETITEM,
            OpCode::REMOVE,
        ];
        for opcode in OpCode::ALL {
            assert!(not_gorgon.get(opcode).is_some(), "missing handler: {opcode:?}");
            let same = not_gorgon.get(opcode).map(|h| h as usize)
                == default.get(opcode).map(|h| h as usize);
            if overridden.contains(&opcode) {
                assert!(!same, "{opcode:?} should be overridden in not_gorgon");
            } else {
                assert!(same, "{opcode:?} should match the default table");
            }
        }
        // NotEchidna reuses NotGorgon (the VulnerableSubStr override is a
        // consensus no-op vs the fixed SUBSTR).
        let not_echidna = JumpTable::not_echidna();
        for opcode in OpCode::ALL {
            assert_eq!(
                not_echidna.get(opcode).map(|h| h as usize),
                not_gorgon.get(opcode).map(|h| h as usize),
                "not_echidna must equal not_gorgon for {opcode:?}"
            );
        }
    }

    #[test]
    fn test_jump_table_invalid_opcode() {
        let jump_table = JumpTable::new();

        // Create a mock engine and instruction
        let mut engine = ExecutionEngine::new(None);
        let instruction = Instruction::new(OpCode::NOP, &[]);

        let mut jump_table = jump_table.clone();
        jump_table.handlers[OpCode::NOP as usize] = None;

        // Execute the instruction
        let result = jump_table.execute(&mut engine, &instruction);

        assert!(result.is_err());
    }
}
