//! C# JSON Test Suite for Neo VM
//!
//! This module contains a comprehensive test suite that executes C# Neo VM JSON test files
//! to ensure compatibility between the Rust and C# implementations.
//!
//! ## Structure
//!
//! - `common`: Shared data structures for JSON deserialization
//! - `runner`: Test execution engine
//! - `opcodes`: Tests organized by opcode categories
//! - `integration`: Comprehensive integration tests
//! - `unit`: Focused unit tests for specific components

pub mod common;
pub mod runner;

pub mod integration;
pub mod opcodes;
pub mod unit;

pub use common::*;
pub use runner::JsonTestRunner;

use std::path::{Path, PathBuf};

/// Root of the checked-in C# Neo VM JSON tests.
pub fn csharp_vm_tests_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../neo_csharp/tests/Neo.VM.Tests/Tests")
}

/// Resolves a test directory relative to the C# Neo VM test tree.
pub fn resolve_test_dir(relative: &str) -> Option<PathBuf> {
    let path = csharp_vm_tests_root().join(relative);
    if Path::new(&path).exists() {
        Some(path)
    } else {
        None
    }
}
