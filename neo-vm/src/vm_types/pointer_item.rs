use neo_type::{Bytes, ScriptHash};

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct Pointer {
    pub offset:      u32,
    pub script_hash: ScriptHash,
}

impl Pointer {
    #[inline]
    pub fn new(offset: u32, script_hash: ScriptHash) -> Self {
        Self { offset, script_hash }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointerItem {
    pos: usize,
    script: Bytes,
    hash: [u8; 20], // Uint160 equivalent
}
