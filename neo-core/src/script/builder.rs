// Copyright @ 2025 - present, R3E Network
// All Rights Reserved

use alloc::vec::Vec;

pub struct ScriptBuilder {
    script: Vec<u8>,
}

impl ScriptBuilder {
    pub fn new() -> Self {
        Self { script: Vec::new() }
    }
}