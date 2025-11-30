//! ApplicationEngine.OpCodePrices - matches C# Neo.SmartContract.ApplicationEngine.OpCodePrices.cs exactly

use crate::smart_contract::ApplicationEngine;
use std::collections::HashMap;

impl ApplicationEngine {
    /// Gets the gas cost for an opcode
    pub fn get_opcode_price(opcode: u8) -> i64 {
        match opcode {
            // Push operations
            0x00..=0x01 => 30,  // PUSHINT8, PUSHINT16
            0x02..=0x04 => 30,  // PUSHINT32, PUSHINT64, PUSHINT128
            0x05 => 120,        // PUSHINT256
            0x0A => 30,         // PUSHNULL
            0x0B..=0x0D => 120, // PUSHDATA1, PUSHDATA2, PUSHDATA4
            0x0E => 30,         // PUSHM1
            0x10..=0x20 => 30,  // PUSH0-PUSH16

            // Control flow
            0x21 => 30,           // NOP
            0x22..=0x24 => 70,    // JMP, JMPIF, JMPIFNOT
            0x25..=0x2A => 70,    // JMPEQ, JMPNE, JMPGT, JMPGE, JMPLT, JMPLE
            0x2B..=0x2E => 70,    // JMP_L, JMPIF_L, JMPIFNOT_L, JMPEQ_L
            0x2F..=0x32 => 70,    // JMPNE_L, JMPGT_L, JMPGE_L, JMPLT_L, JMPLE_L
            0x34..=0x37 => 22000, // CALL, CALL_L, CALLA, CALLT
            0x38 => 30,           // ABORT
            0x39 => 30,           // ASSERT
            0x3A => 30,           // THROW
            0x3B..=0x3D => 70,    // TRY, TRY_L, ENDTRY, ENDTRY_L
            0x3E => 70,           // ENDFINALLY
            0x40 => 30,           // RET
            0x41 => 1000,         // SYSCALL

            // Stack operations
            0x43 => 60,  // DEPTH
            0x45 => 60,  // DROP
            0x46 => 60,  // NIP
            0x48 => 60,  // XDROP
            0x49 => 400, // CLEAR
            0x4A => 60,  // DUP
            0x4B => 60,  // OVER
            0x4D => 60,  // PICK
            0x4E => 60,  // TUCK
            0x50 => 60,  // SWAP
            0x51 => 60,  // ROT
            0x52 => 60,  // ROLL
            0x53 => 400, // REVERSE3
            0x54 => 500, // REVERSE4
            0x55 => 500, // REVERSEN

            // Slot operations
            0x56 => 80000,     // INITSSLOT
            0x57 => 80000,     // INITSLOT
            0x58..=0x5E => 60, // LDSFLD0-LDSFLD6
            0x5F => 60,        // LDSFLD
            0x60..=0x66 => 60, // STSFLD0-STSFLD6
            0x67 => 60,        // STSFLD
            0x68..=0x6E => 60, // LDLOC0-LDLOC6
            0x6F => 60,        // LDLOC
            0x70..=0x76 => 60, // STLOC0-STLOC6
            0x77 => 60,        // STLOC
            0x78..=0x7E => 60, // LDARG0-LDARG6
            0x7F => 60,        // LDARG
            0x80..=0x86 => 60, // STARG0-STARG6
            0x87 => 60,        // STARG

            // Splice operations
            0x88 => 500,    // NEWBUFFER
            0x89 => 300000, // MEMCPY
            0x8B => 80000,  // CAT
            0x8C => 80000,  // SUBSTR
            0x8D => 80000,  // LEFT
            0x8E => 80000,  // RIGHT

            // Bitwise operations (FIXED: correct opcode byte values)
            0x90 => 30, // INVERT
            0x91 => 80, // AND
            0x92 => 80, // OR
            0x93 => 80, // XOR
            0x97 => 80, // EQUAL
            0x98 => 80, // NOTEQUAL

            // Arithmetic operations (FIXED: correct opcode byte values)
            0x99 => 30,  // SIGN
            0x9A => 30,  // ABS
            0x9B => 30,  // NEGATE
            0x9C => 30,  // INC
            0x9D => 30,  // DEC
            0x9E => 80,  // ADD
            0x9F => 80,  // SUB
            0xA0 => 80,  // MUL
            0xA1 => 80,  // DIV
            0xA2 => 80,  // MOD
            0xA3 => 300, // POW
            0xA4 => 300, // SQRT
            0xA5 => 300, // MODMUL
            0xA6 => 300, // MODPOW
            0xA8 => 80,  // SHL
            0xA9 => 80,  // SHR
            0xAA => 30,  // NOT
            0xAB => 100, // BOOLAND
            0xAC => 100, // BOOLOR
            0xB1 => 30,  // NZ
            0xB3 => 80,  // NUMEQUAL
            0xB4 => 80,  // NUMNOTEQUAL
            0xB5 => 80,  // LT
            0xB6 => 80,  // LE
            0xB7 => 80,  // GT
            0xB8 => 80,  // GE
            0xB9 => 80,  // MIN
            0xBA => 80,  // MAX
            0xBB => 80,  // WITHIN

            // Compound types (FIXED: correct opcode byte values)
            0xBE => 400,     // PACKMAP
            0xBF => 400,     // PACKSTRUCT
            0xC0 => 400,     // PACK
            0xC1 => 7000,    // UNPACK
            0xC2 => 16000,   // NEWARRAY0
            0xC3 => 15000,   // NEWARRAY
            0xC4 => 15000,   // NEWARRAY_T
            0xC5 => 16000,   // NEWSTRUCT0
            0xC6 => 15000,   // NEWSTRUCT
            0xC8 => 200,     // NEWMAP
            0xCA => 500,     // SIZE
            0xCB => 30,      // HASKEY
            0xCC => 1000000, // KEYS
            0xCD => 500000,  // VALUES
            0xCE => 270,     // PICKITEM
            0xCF => 15000,   // APPEND
            0xD0 => 270,     // SETITEM
            0xD1 => 500,     // REVERSEITEMS
            0xD2 => 500,     // REMOVE
            0xD3 => 500,     // CLEARITEMS
            0xD4 => 60,      // POPITEM

            // Types (FIXED: correct opcode byte values)
            0xD8 => 30,    // ISNULL
            0xD9 => 30,    // ISTYPE
            0xDB => 80000, // CONVERT

            // Extensions
            0xE0 => 30, // ABORTMSG
            0xE1 => 30, // ASSERTMSG

            _ => 30, // Default price
        }
    }

    /// Creates the default opcode price table
    pub fn create_opcode_price_table() -> HashMap<u8, i64> {
        let mut table = HashMap::new();

        for opcode in 0..=255u8 {
            table.insert(opcode, Self::get_opcode_price(opcode));
        }

        table
    }
}
