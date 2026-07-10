//! ApplicationEngine.OpCodePrices - matches C# Neo.SmartContract.ApplicationEngine.OpCodePrices.cs exactly

use crate::ApplicationEngine;

impl<P, D, B> ApplicationEngine<P, D, B>
where
    P: crate::native_contract_provider::NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
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
#[path = "../tests/interop/application_engine_op_code_prices.rs"]
mod tests;
