use crate::core::fee;
use crate::vm::opcode;

pub struct Context {
    base_exec_fee: i64,
}

impl Context {
    // GetPrice returns a price for executing op with the provided parameter.
    pub fn get_price(&self, op: opcode::Opcode, _parameter: &[u8]) -> i64 {
        fee::opcode(self.base_exec_fee, op)
    }
}
