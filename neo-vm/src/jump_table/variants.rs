//! Default and hardfork-specific jump-table construction.
//!
//! The fixed dispatch table lives in `table`; this module wires opcode-family
//! registration and hardfork-specific table variants selected by the execution engine.

use crate::OpCode;

use super::operations::{bitwisee, numeric, splice, types};
use super::{JumpTable, compound, control, push, slot, stack};

impl<S> Default for JumpTable<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> JumpTable<S> {
    /// Creates a new jump table.
    #[must_use]
    pub fn new() -> Self {
        let mut jump_table = Self::empty();
        jump_table.register_default_handlers();
        jump_table
    }

    /// The pre-`HF_Gorgon` compound-opcode table. C#
    /// `ApplicationEngine.ComposeNotGorgonJumpTable` = the default table with
    /// `HASKEY`/`PICKITEM`/`SETITEM`/`REMOVE` reverted to their pre-neo-vm#543
    /// handlers and `SHL`/`SHR` reverted to the zero-shift behavior from before
    /// neo-vm#567. `ApplicationEngine.Create` selects this table when
    /// `HF_Echidna` is active but `HF_Gorgon` is not.
    pub fn not_gorgon() -> Self {
        let mut table = Self::new();
        table.set(OpCode::HASKEY, compound::has_key_before543::<S>);
        table.set(OpCode::PICKITEM, compound::pick_item_before543::<S>);
        table.set(OpCode::SETITEM, compound::set_item_before543::<S>);
        table.set(OpCode::REMOVE, compound::remove_before543::<S>);
        table.set(OpCode::SHL, numeric::vulnerable_shl::<S>);
        table.set(OpCode::SHR, numeric::vulnerable_shr::<S>);
        table
    }

    /// The pre-`HF_Echidna` jump table. C# composes this from `NotGorgon` and
    /// additionally installs `ApplicationEngine.VulnerableSubStr`.
    pub fn not_echidna() -> Self {
        let mut table = Self::not_gorgon();
        table.set(OpCode::SUBSTR, splice::vulnerable_substr::<S>);
        table
    }

    /// Registers the default handlers for all opcodes.
    fn register_default_handlers(&mut self) {
        bitwisee::register_handlers(self);
        compound::register_handlers(self);
        control::register_handlers(self);
        numeric::register_handlers(self);
        push::register_handlers(self);
        slot::register_handlers(self);
        splice::register_handlers(self);
        stack::register_handlers(self);
        types::register_handlers(self);
    }
}
