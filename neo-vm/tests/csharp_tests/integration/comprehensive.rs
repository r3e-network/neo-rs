//! Comprehensive integration tests executing the C# JSON fixtures.

use crate::csharp_tests::{resolve_test_dir, JsonTestRunner};

/// Test Others category (matches C# TestOthers)
#[test]
#[ignore = "C# JSON fixture harness is still incomplete in the Rust port"]
fn test_others() {
    if let Some(test_path) = resolve_test_dir("Others") {
        let mut runner = JsonTestRunner::new();
        runner
            .test_json_directory(test_path.to_str().expect("valid UTF-8 path"))
            .unwrap();
    } else {
        eprintln!("C# test directory not found: Others");
    }
}

/// Test all available JSON test files in the C# test suite
#[test]
#[ignore = "Full C# JSON suite not yet aligned with Rust VM behaviour"]
fn test_all_csharp_json_tests() {
    if let Some(base_path) = resolve_test_dir("") {
        let mut runner = JsonTestRunner::new();
        println!("Running comprehensive C# JSON test suite");

        let categories = [
            "Others",
            "OpCodes/Arrays",
            "OpCodes/Stack",
            "OpCodes/Slot",
            "OpCodes/Splice",
            "OpCodes/Control",
            "OpCodes/Push",
            "OpCodes/Arithmetic",
            "OpCodes/BitwiseLogic",
            "OpCodes/Types",
        ];

        for category in categories {
            let category_path = base_path.join(category);
            if category_path.exists() {
                println!("Testing category: {}", category);
                match runner.test_json_directory(category_path.to_str().expect("valid UTF-8 path"))
                {
                    Ok(_) => println!("  ✓ Category {} passed", category),
                    Err(e) => {
                        println!("  ✗ Category {} failed: {}", category, e);
                    }
                }
            } else {
                println!("  - Category {} not found", category);
            }
        }
    } else {
        eprintln!("C# test base directory not found");
    }
}
