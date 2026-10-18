//! ApplicationEngine.OpCodePrices - matches C# Neo.SmartContract.ApplicationEngine.OpCodePrices.cs exactly.
//!
//! The 256-entry array maps OpCode byte values (index) to execution-unit costs
//! before the ExecFeeFactor multiplier is applied. It is checked exhaustively
//! against a table transcribed from the v3.10.1 C# reference by
//! `tests::matches_csharp_reference_table`, so an accidental edit cannot drift
//! into a consensus divergence.
//!
//! Note that opcode numbering is easy to get wrong: `NEWARRAY` is 0xc3,
//! `NEWARRAY_T` is 0xc4, `NEWSTRUCT0` is 0xc5 (price 16, not the NEWARRAY
//! price), `NEWSTRUCT` is 0xc6, and `NEWMAP` is 0xc8 (price 8).
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

#[cfg(test)]
mod tests {
    use super::ApplicationEngine;

    /// Transcribed from Neo v3.10.1
    /// `src/Neo/SmartContract/ApplicationEngine.OpCodePrices.cs`, indexed by
    /// `Neo.VM.OpCode` byte values. Slots that are not valid opcodes are 0.
    ///
    /// Do not "fix" this table to match an implementation shortcut: it is the
    /// consensus reference used to keep `OPCODE_PRICE_TABLE` honest.
    const CSHARP_V3101_PRICES: [i64; 256] = [
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

    #[test]
    fn matches_csharp_reference_table() {
        assert_eq!(
            ApplicationEngine::OPCODE_PRICE_TABLE,
            CSHARP_V3101_PRICES,
            "opcode price table diverges from Neo v3.10.1; this changes consensus gas accounting"
        );
    }
}
