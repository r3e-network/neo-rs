//! Flow control OpCodes for the Neo Virtual Machine.
//!
//! This module contains all OpCodes related to program flow control,
//! including jumps, calls, exceptions, and function returns.

/// Flow control OpCodes.
///
/// These opcodes control the execution flow of the virtual machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FlowControlOpCode {
    /// Does nothing. It is intended to fill in space if opcodes are patched.
    ///
    /// # Stack
    /// ```text
    /// Push: 0 items
    /// Pop: 0 items
    /// ```
    NOP = 0x21,

    /// Unconditionally transfers control to a target instruction. The target instruction is represented as a 1-byte signed offset from the beginning of the current instruction.
    ///
    /// # Stack
    /// ```text
    /// Push: 0 items
    /// Pop: 0 items
    /// ```
    JMP = 0x22,

    /// Unconditionally transfers control to a target instruction. The target instruction is represented as a 4-byte signed offset from the beginning of the current instruction.
    ///
    /// # Stack
    /// ```text
    /// Push: 0 items
    /// Pop: 0 items
    /// ```
    JMP_L = 0x23,

    /// Transfers control to a target instruction if the value is true, not null, or non-zero. The target instruction is represented as a 1-byte signed offset from the beginning of the current instruction.
    ///
    /// # Stack
    /// ```text
    /// Push: 0 items
    /// Pop: 1 item
    /// ```
    JMPIF = 0x24,

    /// Transfers control to a target instruction if the value is true, not null, or non-zero. The target instruction is represented as a 4-byte signed offset from the beginning of the current instruction.
    ///
    /// # Stack
    /// ```text
    /// Push: 0 items
    /// Pop: 1 item
    /// ```
    JMPIF_L = 0x25,

    /// Transfers control to a target instruction if the value is false, a null reference, or zero. The target instruction is represented as a 1-byte signed offset from the beginning of the current instruction.
    ///
    /// # Stack
    /// ```text
    /// Push: 0 items
    /// Pop: 1 item
    /// ```
    JMPIFNOT = 0x26,

    /// Transfers control to a target instruction if the value is false, a null reference, or zero. The target instruction is represented as a 4-byte signed offset from the beginning of the current instruction.
    ///
    /// # Stack
    /// ```text
    /// Push: 0 items
    /// Pop: 1 item
    /// ```
    JMPIFNOT_L = 0x27,

    /// Transfers control to a target instruction if two values are equal. The target instruction is represented as a 1-byte signed offset from the beginning of the current instruction.
    ///
    /// # Stack
    /// ```text
    /// Push: 0 items
    /// Pop: 2 items
    /// ```
    JMPEQ = 0x28,

    /// Transfers control to a target instruction if two values are equal. The target instruction is represented as a 4-byte signed offset from the beginning of the current instruction.
    ///
    /// # Stack
    /// ```text
    /// Push: 0 items
    /// Pop: 2 items
    /// ```
    JMPEQ_L = 0x29,

    /// Transfers control to a target instruction when two values are not equal. The target instruction is represented as a 1-byte signed offset from the beginning of the current instruction.
    ///
    /// # Stack
    /// ```text
    /// Push: 0 items
    /// Pop: 2 items
    /// ```
    JMPNE = 0x2A,

    /// Transfers control to a target instruction when two values are not equal. The target instruction is represented as a 4-byte signed offset from the beginning of the current instruction.
    ///
    /// # Stack
    /// ```text
    /// Push: 0 items
    /// Pop: 2 items
    /// ```
    JMPNE_L = 0x2B,

    /// Transfers control to a target instruction if the first value is greater than the second value. The target instruction is represented as a 1-byte signed offset from the beginning of the current instruction.
    ///
    /// # Stack
    /// ```text
    /// Push: 0 items
    /// Pop: 2 items
    /// ```
    JMPGT = 0x2C,

    /// Transfers control to a target instruction if the first value is greater than the second value. The target instruction is represented as a 4-byte signed offset from the beginning of the current instruction.
    ///
    /// # Stack
    /// ```text
    /// Push: 0 items
    /// Pop: 2 items
    /// ```
    JMPGT_L = 0x2D,

    /// Transfers control to a target instruction if the first value is greater than or equal to the second value. The target instruction is represented as a 1-byte signed offset from the beginning of the current instruction.
    ///
    /// # Stack
    /// ```text
    /// Push: 0 items
    /// Pop: 2 items
    /// ```
    JMPGE = 0x2E,

    /// Transfers control to a target instruction if the first value is greater than or equal to the second value. The target instruction is represented as a 4-byte signed offset from the beginning of the current instruction.
    ///
    /// # Stack
    /// ```text
    /// Push: 0 items
    /// Pop: 2 items
    /// ```
    JMPGE_L = 0x2F,

    /// Transfers control to a target instruction if the first value is less than the second value. The target instruction is represented as a 1-byte signed offset from the beginning of the current instruction.
    ///
    /// # Stack
    /// ```text
    /// Push: 0 items
    /// Pop: 2 items
    /// ```
    JMPLT = 0x30,

    /// Transfers control to a target instruction if the first value is less than the second value. The target instruction is represented as a 4-byte signed offset from the beginning of the current instruction.
    ///
    /// # Stack
    /// ```text
    /// Push: 0 items
    /// Pop: 2 items
    /// ```
    JMPLT_L = 0x31,

    /// Transfers control to a target instruction if the first value is less than or equal to the second value. The target instruction is represented as a 1-byte signed offset from the beginning of the current instruction.
    ///
    /// # Stack
    /// ```text
    /// Push: 0 items
    /// Pop: 2 items
    /// ```
    JMPLE = 0x32,

    /// Transfers control to a target instruction if the first value is less than or equal to the second value. The target instruction is represented as a 4-byte signed offset from the beginning of the current instruction.
    ///
    /// # Stack
    /// ```text
    /// Push: 0 items
    /// Pop: 2 items
    /// ```
    JMPLE_L = 0x33,

    /// Calls the function at the target address. The target instruction is represented as a 1-byte signed offset from the beginning of the current instruction.
    ///
    /// # Stack
    /// ```text
    /// Push: 0 items
    /// Pop: 0 items
    /// ```
    CALL = 0x34,

    /// Calls the function at the target address. The target instruction is represented as a 4-byte signed offset from the beginning of the current instruction.
    ///
    /// # Stack
    /// ```text
    /// Push: 0 items
    /// Pop: 0 items
    /// ```
    CALL_L = 0x35,

    /// Calls the function at the target address. The target instruction is represented as a value on the stack.
    ///
    /// # Stack
    /// ```text
    /// Push: 0 items
    /// Pop: 1 item
    /// ```
    CALLA = 0x36,

    /// Unconditionally terminates the execution with failure.
    ///
    /// # Stack
    /// ```text
    /// Push: 0 items
    /// Pop: 0 items
    /// ```
    ABORT = 0x37,

    /// Throws an exception if the condition is not met.
    ///
    /// # Stack
    /// ```text
    /// Push: 0 items
    /// Pop: 1 item
    /// ```
    ASSERT = 0x38,

    /// Throws an exception.
    ///
    /// # Stack
    /// ```text
    /// Push: 0 items
    /// Pop: 1 item
    /// ```
    THROW = 0x39,

    /// Begins a try block.
    ///
    /// # Stack
    /// ```text
    /// Push: 0 items
    /// Pop: 0 items
    /// ```
    TRY = 0x3A,

    /// Begins a try block with long offsets.
    ///
    /// # Stack
    /// ```text
    /// Push: 0 items
    /// Pop: 0 items
    /// ```
    TRY_L = 0xF0,

    /// Ends a try block.
    ///
    /// # Stack
    /// ```text
    /// Push: 0 items
    /// Pop: 0 items
    /// ```
    ENDTRY = 0x3B,

    /// Ends a try block with long offset.
    ///
    /// # Stack
    /// ```text
    /// Push: 0 items
    /// Pop: 0 items
    /// ```
    ENDTRY_L = 0xF1,

    /// Ends a finally block.
    ///
    /// # Stack
    /// ```text
    /// Push: 0 items
    /// Pop: 0 items
    /// ```
    ENDFINALLY = 0x3C,

    /// Returns from the current function.
    ///
    /// # Stack
    /// ```text
    /// Push: 0 items
    /// Pop: 0 items
    /// ```
    RET = 0x3D,

    /// Calls a system function.
    ///
    /// # Stack
    /// ```text
    /// Push: 0 items
    /// Pop: 0 items
    /// ```
    SYSCALL = 0x3E,
}

impl FlowControlOpCode {
    /// Checks if this is a jump instruction.
    pub fn is_jump(&self) -> bool {
        matches!(
            self,
            Self::JMP
                | Self::JMP_L
                | Self::JMPIF
                | Self::JMPIF_L
                | Self::JMPIFNOT
                | Self::JMPIFNOT_L
                | Self::JMPEQ
                | Self::JMPEQ_L
                | Self::JMPNE
                | Self::JMPNE_L
                | Self::JMPGT
                | Self::JMPGT_L
                | Self::JMPGE
                | Self::JMPGE_L
                | Self::JMPLT
                | Self::JMPLT_L
                | Self::JMPLE
                | Self::JMPLE_L
        )
    }

    /// Checks if this is a call instruction.
    pub fn is_call(&self) -> bool {
        matches!(self, Self::CALL | Self::CALL_L | Self::CALLA)
    }

    /// Checks if this is an exception-related instruction.
    pub fn is_exception(&self) -> bool {
        matches!(
            self,
            Self::ABORT
                | Self::ASSERT
                | Self::THROW
                | Self::TRY
                | Self::TRY_L
                | Self::ENDTRY
                | Self::ENDTRY_L
                | Self::ENDFINALLY
        )
    }

    /// Checks if this instruction uses a long offset.
    pub fn uses_long_offset(&self) -> bool {
        matches!(
            self,
            Self::JMP_L
                | Self::JMPIF_L
                | Self::JMPIFNOT_L
                | Self::JMPEQ_L
                | Self::JMPNE_L
                | Self::JMPGT_L
                | Self::JMPGE_L
                | Self::JMPLT_L
                | Self::JMPLE_L
                | Self::CALL_L
                | Self::TRY_L
                | Self::ENDTRY_L
        )
    }
}
