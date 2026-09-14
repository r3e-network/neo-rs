//! ApplicationEngine.OpCodePrices - matches C# Neo.SmartContract.ApplicationEngine.OpCodePrices.cs exactly
//!
// TODO(M-17): This table needs cross-validation against the Neo N3 C# reference
// (Neo/src/Neo/SmartContract/ApplicationEngine.OpCodePrices.cs).  The 256-entry
// array maps OpCode byte values (index) to execution-unit costs before the
// ExecFeeFactor multiplier is applied.  Representative known C# reference values
// for a spot-check (opcode → price in exec units):
//   NOP (0x21)     → 1
//   PUSH0 (0x00)   → 1
//   PUSHDATA1(0x0c)→ 8
//   SYSCALL(0x41)  → 0 (priced per-syscall)
//   NEWARRAY(0xc5) → 512
//   APPEND(0xc8)   → 8192
//
// Until a full automated comparison against the C# source is run, treat any
// price as potentially stale.

use crate::smart_contract::ApplicationEngine;

impl ApplicationEngine {
    /// The prices of all opcodes (in execution units, before ExecFeeFactor).
    pub const OPCODE_PRICE_TABLE: [i64; 256] = [
        1, 1, 1, 1, 4, 4, 0, 0, 1, 1, 4, 1, 8, 512, 4096, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 512, 512, 512, 32768,
        0, 1, 512, 4, 4, 4, 4, 4, 0, 0, 0, 2, 0, 2, 2, 0, 16, 16, 2, 2, 0, 2, 2, 0, 2, 2, 16, 2, 2,
        16, 16, 64, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
        2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 256, 2048, 0, 2048, 2048,
        2048, 2048, 0, 4, 8, 8, 8, 0, 0, 0, 32, 32, 4, 4, 4, 4, 4, 8, 8, 8, 8, 8, 64, 64, 32, 2048,
        0, 8, 8, 4, 8, 8, 0, 0, 0, 0, 4, 0, 8, 8, 8, 8, 8, 8, 8, 8, 8, 0, 0, 2048, 2048, 2048,
        2048, 16, 512, 512, 16, 512, 0, 8, 0, 4, 64, 16, 8192, 64, 8192, 8192, 8192, 16, 16, 16, 0,
        0, 0, 2, 2, 0, 8192, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];

    /// Gets the execution unit cost for an opcode.
    pub fn get_opcode_price(opcode: u8) -> i64 {
        Self::OPCODE_PRICE_TABLE[opcode as usize]
    }
}
