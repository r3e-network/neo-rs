use alloc::vec::Vec;

use crate::script::{opcode::OpCode, Script};

/// Helper type for constructing Neo VM scripts programmatically.
pub struct ScriptBuilder {
    pub(super) script: Vec<u8>,
}

impl ScriptBuilder {
    pub fn new() -> Self {
        Self { script: Vec::new() }
    }

    pub fn push_opcode(&mut self, opcode: OpCode) -> &mut Self {
        self.script.push(opcode as u8);
        self
    }

    pub fn push_raw(&mut self, bytes: &[u8]) -> &mut Self {
        self.script.extend_from_slice(bytes);
        self
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.script
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.script
    }

    pub fn into_script(self) -> Script {
        Script::new(self.script)
    }
}
