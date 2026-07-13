// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License

//! Fuzz target for VM script parsing.
//!
//! This fuzzer targets the script parsing code to find:
//! - Invalid instruction sequences that cause panics
//! - Jump targets pointing outside script bounds
//! - Resource exhaustion attacks

#![no_main]

use libfuzzer_sys::fuzz_target;
use neo_vm::validate_script;

fuzz_target!(|data: &[u8]| {
    // Relaxed validation parses opcode boundaries and operand widths.
    if let Ok(script) = validate_script(data, false) {
        let _ = script.has_instruction_at(0);
    }

    // Strict validation additionally checks jump/try targets and type operands.
    let _ = validate_script(data, true);
});
