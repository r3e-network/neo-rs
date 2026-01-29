// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License

//! Fuzz target for VM script parsing.
//!
//! This fuzzer targets the script parsing code to find:
//! - Invalid instruction sequences that cause panics
//! - Jump targets pointing outside script bounds
//! - Stack overflow vulnerabilities
//! - Resource exhaustion attacks

#![no_main]

use libfuzzer_sys::fuzz_target;
use neo_vm::Script;

fuzz_target!(|data: &[u8]| {
    // Fuzz script parsing in both non-strict and strict modes
    // 
    // Non-strict mode: Basic validation only
    // - Should handle malformed opcodes gracefully
    // - Should detect invalid instruction boundaries
    //
    // Strict mode: Full validation including jump targets
    // - Should validate all jump targets are valid
    // - Should validate try-catch-finally blocks
    
    // Test non-strict mode (basic validation)
    let _ = Script::new(data.to_vec(), false);
    
    // Test strict mode (full validation with jump target verification)
    // This is more expensive but catches more issues
    let _ = Script::new(data.to_vec(), true);
    
    // Test the relaxed constructor (no validation)
    // Then try to iterate through instructions
    let script = Script::new_relaxed(data.to_vec());
    
    // Iterate through instructions to catch parsing bugs
    // This exercises the instruction decoder
    for result in script.instructions() {
        // We expect either a valid instruction or an error,
        // but never a panic
        match result {
            Ok((_pos, instr)) => {
                // Try to access instruction properties
                let _ = instr.opcode();
                let _ = instr.size();
                let _ = instr.pointer();
                
                // Try to get jump target if it's a jump instruction
                let _ = script.get_jump_target(&instr);
            }
            Err(_) => break, // Error is expected for malformed input
        }
    }
});
