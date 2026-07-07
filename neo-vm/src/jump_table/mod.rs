//! # neo-vm::jump_table
//!
//! Opcode dispatch tables and instruction implementations.
//!
//! ## Boundary
//!
//! This module belongs to `neo-vm`. This VM crate owns deterministic script
//! execution and must not own ledger persistence, network transport, or node
//! composition.
//!
//! ## Contents
//!
//! - `bitwisee`: bitwise opcode handlers.
//! - `compound`: compound opcode handlers.
//! - `control`: control-flow opcode handlers.
//! - `numeric`: Fixed-size numeric wrappers and byte-order conversion helpers.
//! - `push`: push opcode handlers.
//! - `slot`: VM slot records and helpers.
//! - `splice`: splice opcode handlers.
//! - `stack`: VM stack opcode handlers.
//! - `types`: Storage-domain types shared by store implementations.
//! - `tests`: Module-local tests and regression coverage.

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
use crate::execution_context::ExecutionContext;
use crate::execution_engine::ExecutionEngine;
use crate::stack_item::StackItem;
use neo_vm_rs::Instruction;
use neo_vm_rs::OpCode;
use neo_vm_rs::StackValue;
use num_bigint::BigInt;

/// A handler for a VM instruction.
pub type InstructionHandler = fn(&mut ExecutionEngine, &Instruction) -> VmResult<()>;

/// C# `StackItem.GetInteger()` semantics for an integer operand read off the
/// evaluation stack (a count, index, size or shift a script controls).
///
/// In the reference VM a `Buffer` is NOT a `PrimitiveType` and has no
/// `GetInteger` override, so `GetInteger()` hits the base
/// `=> throw new InvalidCastException()` and FAULTS — even for a short buffer.
/// `Null` and compound items (`Array`/`Struct`/`Map`/pointer/interop) fault too;
/// only the `Integer`/`Boolean`/`ByteString` primitives yield a value.
///
/// This deliberately differs from [`StackItem::into_int`], which coerces a
/// `Buffer` of up to `VM_INTEGER_MAX_SIZE` bytes to its little-endian integer
/// value. That coercion is the `ConvertTo(Integer)` path (the CONVERT opcode);
/// the GetInteger path used by count/index/shift operands faults on a `Buffer`.
///
/// Callers still narrow the returned `BigInt` (e.g. `to_i32`/`to_i64`/`to_usize`)
/// and a value outside the target range faults — matching C#'s `(int)BigInteger`
/// cast, which throws `OverflowException` (it does NOT truncate) before the
/// per-opcode sign/bounds checks run.
pub(crate) fn get_integer(item: StackItem) -> VmResult<BigInt> {
    if matches!(item, StackItem::Buffer(_)) {
        return Err(VmError::invalid_type_simple(
            "operand is not an integer (C# GetInteger faults on Buffer)",
        ));
    }
    item.into_int()
}

/// C# `StackItem.GetInteger()` for an arithmetic/bitwise VALUE operand, returning
/// the typed [`StackValue`] the semantics layer expects.
///
/// Like [`get_integer`], a `Buffer` (not a `PrimitiveType`) and `Null` fault —
/// the numeric/comparison/bitwise opcodes (ADD/SUB/.../AND/OR/XOR/INVERT) read
/// their operands via `GetInteger()`, which throws on a non-integer. Only the
/// CONVERT opcode coerces a `Buffer`, via a separate `ConvertTo` path.
pub(crate) fn numeric_operand(item: StackItem) -> VmResult<StackValue> {
    match item {
        StackItem::Buffer(_) | StackItem::Null => Err(VmError::invalid_type_simple(
            "operand is not a numeric value (C# GetInteger faults on Buffer/Null)",
        )),
        item => StackValue::try_from(item),
    }
}

/// The engine's current execution context, or a fault if there is none.
///
/// Shared by every opcode-family handler module so the "No current context"
/// guard reads identically across the jump table.
#[inline]
pub(crate) fn require_context(engine: &mut ExecutionEngine) -> VmResult<&mut ExecutionContext> {
    engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))
}

/// Maps an error string raised by the `neo_vm_rs` semantics layer into a VM
/// fault, matching how the reference VM surfaces an arithmetic/type failure.
#[inline]
pub(crate) fn semantics_error(error: String) -> VmError {
    VmError::invalid_operation_msg(error)
}

/// Pushes a typed [`StackValue`] result back onto the evaluation stack,
/// converting it into the engine's [`StackItem`] representation.
#[inline]
pub(crate) fn push_stack_value(ctx: &mut ExecutionContext, value: StackValue) -> VmResult<()> {
    ctx.push(StackItem::try_from(value)?)
}

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

neo_io::impl_default_via_new!(JumpTable);

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
    // Rationale: this inherent method preserves the historical VM API; the
    // actual `Default` impl can delegate without changing call sites.
    #[allow(clippy::should_implement_trait)]
    pub fn default() -> Self {
        // Use OnceLock for safe one-time initialization
        DEFAULT.get_or_init(Self::new).clone()
    }

    /// The pre-`HF_Gorgon` compound-opcode table. C#
    /// `ApplicationEngine.ComposeNotGorgonJumpTable` = the default table with
    /// `HASKEY`/`PICKITEM`/`SETITEM`/`REMOVE` reverted to their pre-neo-vm#543
    /// handlers. `ApplicationEngine.Create` selects this table when `HF_Echidna`
    /// is active but `HF_Gorgon` is not — which is the v3.10.0 mainnet/testnet
    /// case, since `HF_Gorgon` is unscheduled there.
    ///
    /// SHL/SHR are NOT overridden here: they carry no `HF_Gorgon` split, so the
    /// default handler applies. (Their behavior IS a flat Neo.VM 3.9.0→3.10.0
    /// change — 3.10.0 always pops + integer-coerces the value even on a zero
    /// shift — but that is a VM-version change, not a hardfork gate; see the
    /// `shift` handler in `numeric.rs`.)
    pub fn not_gorgon() -> Self {
        NOT_GORGON
            .get_or_init(|| {
                let mut table = Self::new();
                table.set(OpCode::HASKEY, compound::has_key_before543);
                table.set(OpCode::PICKITEM, compound::pick_item_before543);
                table.set(OpCode::SETITEM, compound::set_item_before543);
                table.set(OpCode::REMOVE, compound::remove_before543);
                table
            })
            .clone()
    }

    /// The pre-`HF_Echidna` jump table. C# v3.10.0 overrides only SUBSTR with
    /// `ApplicationEngine.VulnerableSubStr`; the memory-unsafe distinction is
    /// not reproducible here and is consensus-equivalent for valid results and
    /// faulting cases, so this table intentionally keeps every handler at the
    /// default implementation.
    pub fn not_echidna() -> Self {
        Self::default()
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

#[cfg(test)]
#[path = "../tests/jump_table/mod.rs"]
mod tests;
