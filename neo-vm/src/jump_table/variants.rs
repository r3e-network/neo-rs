//! Default and hardfork-specific jump-table construction.
//!
//! The fixed dispatch table lives in `table`; this module wires opcode-family
//! registration and hardfork-specific table variants selected by the execution engine.

use neo_vm_rs::OpCode;

use super::{JumpTable, bitwisee, compound, control, numeric, push, slot, splice, stack, types};

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
    /// handlers. `ApplicationEngine.Create` selects this table when `HF_Echidna`
    /// is active but `HF_Gorgon` is not — which is the v3.10.1 mainnet/testnet
    /// case, since `HF_Gorgon` is unscheduled there.
    ///
    /// SHL/SHR are NOT overridden here: they carry no `HF_Gorgon` split, so the
    /// default handler applies. (Their behavior IS a flat Neo.VM 3.9.0→3.10.0
    /// change — v3.10.1 always pops + integer-coerces the value even on a zero
    /// shift — but that is a VM-version change, not a hardfork gate; see the
    /// `shift` handler in `numeric.rs`.)
    pub fn not_gorgon() -> Self {
        let mut table = Self::new();
        table.set(OpCode::HASKEY, compound::has_key_before543::<S>);
        table.set(OpCode::PICKITEM, compound::pick_item_before543::<S>);
        table.set(OpCode::SETITEM, compound::set_item_before543::<S>);
        table.set(OpCode::REMOVE, compound::remove_before543::<S>);
        table
    }

    /// The pre-`HF_Echidna` jump table. C# v3.10.1 overrides only SUBSTR with
    /// `ApplicationEngine.VulnerableSubStr`; the memory-unsafe distinction is
    /// not reproducible here and is consensus-equivalent for valid results and
    /// faulting cases, so this table intentionally keeps every handler at the
    /// default implementation.
    pub fn not_echidna() -> Self {
        Self::default()
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
