//! Performance and benchmark tests for the JSON runner.

use crate::csharp_tests::{JsonTestRunner, VMUT, resolve_test_dir};
use serde_json;
use std::fs;
use std::time::Instant;

/// Benchmark test to compare performance with C# implementation
#[test]
fn benchmark_json_test_performance() {
    if let Some(test_path) = resolve_test_dir("OpCodes/Push/PUSHNULL.json") {
        let path_str = test_path.to_str().expect("valid UTF-8 path");
        let mut runner = JsonTestRunner::new();

        let start = Instant::now();
        for _ in 0..100 {
            runner.test_json_file(path_str).unwrap();
        }
        let duration = start.elapsed();

        println!("100 iterations of PUSHNULL test took: {:?}", duration);
        println!("Average per test: {:?}", duration / 100);
    } else {
        eprintln!("C# test file not found for benchmark: OpCodes/Push/PUSHNULL.json");
    }
}

/// Test error handling for malformed JSON tests
#[test]
fn test_malformed_json_handling() {
    let mut runner = JsonTestRunner::new();

    // Test with non-existent file
    let result = runner.test_json_file("/non/existent/path.json");
    assert!(result.is_err(), "Should fail for non-existent file");

    // Test with non-existent directory
    let result = runner.test_json_directory("/non/existent/directory");
    assert!(result.is_err(), "Should fail for non-existent directory");
}

/// Test JSON file parsing without execution
#[test]
fn test_json_file_parsing() {
    if let Some(test_path) = resolve_test_dir("OpCodes/Push/PUSHNULL.json") {
        let path_str = test_path.to_str().expect("valid UTF-8 path");
        println!("📁 Loading JSON test file: {}", path_str);

        // Try to read and parse the JSON file
        let file_content = fs::read_to_string(path_str).expect("Failed to read test file");
        println!("📄 File content length: {} bytes", file_content.len());

        // Try to parse as VMUT structure
        let vmut_result: Result<VMUT, _> = serde_json::from_str(&file_content);
        match vmut_result {
            Ok(vmut) => {
                println!("✅ JSON parsing successful!");
                println!("   Category: {}", vmut.category);
                println!("   Name: {}", vmut.name);
                println!("   Number of tests: {}", vmut.tests.len());

                if let Some(first_test) = vmut.tests.first() {
                    println!("   First test: {}", first_test.name);
                    println!("   Script: {:?}", first_test.script);

                    let runner = JsonTestRunner::new();
                    match runner.compile_script(&first_test.script) {
                        Ok(bytecode) => {
                            println!("   ✅ Script compilation successful: {:?}", bytecode);
                        }
                        Err(e) => {
                            println!("   ❌ Script compilation failed: {}", e);
                        }
                    }
                }
            }
            Err(e) => {
                println!("❌ JSON parsing failed: {}", e);
                panic!("Failed to parse JSON test file");
            }
        }
    } else {
        eprintln!("C# test file not found for parsing test: OpCodes/Push/PUSHNULL.json");
    }
}
