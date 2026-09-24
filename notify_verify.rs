// Quick verification script for Notify syscall registration
use sha2::{Digest, Sha256};
use neo_vm::syscalls::static_registry::{get_gas_cost, init, find_by_method_name};

fn main() {
    init();
    
    println!("=== Verifying System.Runtime.Notify Registration ===\n");
    
    // Test 1: Find by method name
    println!("✓ Test 1: Finding Notify by method name...");
    let notify_entry = find_by_method_name("System.Runtime.Notify")
        .expect("Notify should be registered!");
    println!("  Method ID: {}", notify_entry.method_id);
    println!("  Gas Price: {} (expected 32768)", notify_entry.gas_price);
    assert_eq!(notify_entry.gas_price, 32768, "Gas price mismatch!");
    
    // Test 2: Get gas cost by hash
    println!("\n✓ Test 2: Computing hash and getting gas cost...");
    let digest = Sha256::digest("System.Runtime.Notify".as_bytes());
    let hash: [u8; 32] = digest.into();
    let gas_cost = get_gas_cost(&hash);
    match gas_cost {
        Some(cost) => {
            println!("  Hash: {:02x?}", &hash[..8]);
            println!("  Gas Cost: {} ✓", cost);
            assert_eq!(cost, 32768, "Gas cost from hash should be 32768!");
        }
        None => panic!("✗ FAILED: Notify not found via hash!"),
    }
    
    // Test 3: Verify total count
    println!("\n✓ Test 3: Registry completeness check...");
    use neo_vm::syscalls::static_registry::BUILTIN_METHODS;
    println!("  Total syscalls registered: {}", BUILTIN_METHODS.len());
    assert_eq!(BUILTIN_METHODS.len(), 38, "Should have 38 syscalls!");
    
    println!("\n✅ ALL CHECKS PASSED!");
    println!("   - Notify is properly registered");
    println!("   - Gas pricing is correct (32768 units)");
    println!("   - Hash lookup works correctly");
}
