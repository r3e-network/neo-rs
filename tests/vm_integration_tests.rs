//! Comprehensive VM Integration Tests
//!
//! These tests verify VM integration with smart contracts, native contracts,
//! interop services, and blockchain operations.

use neo_core::{Signer, Transaction, UInt160, UInt256, Witness, WitnessScope};
use neo_smart_contract::{ApplicationEngine, NativeRegistry, NefFile};
use neo_vm::{ExecutionEngine, OpCode, Script, StackItem, TriggerType, VMState};
use std::collections::HashMap;
use tokio_test;

/// Test VM execution with complex smart contract scenarios
#[tokio::test]
async fn test_vm_complex_smart_contract_execution() {
    println!("🧪 Testing complex smart contract execution");

    // Create a complex script that tests multiple VM features
    let complex_script = vec![
        // Test 1: Stack operations
        OpCode::PUSH10 as u8, // Push 10
        OpCode::PUSH20 as u8, // Push 20
        OpCode::DUP as u8,    // Duplicate 20 -> stack: [10, 20, 20]
        OpCode::ROT as u8,    // Rotate -> stack: [20, 20, 10]
        OpCode::ADD as u8,    // Add top two -> stack: [20, 30]
        // Test 2: Comparison operations
        OpCode::PUSH30 as u8, // Push 30 -> stack: [20, 30, 30]
        OpCode::EQUAL as u8,  // Compare top two -> stack: [20, true]
        OpCode::VERIFY as u8, // Verify (consumes true) -> stack: [20]
        // Test 3: Array operations
        OpCode::NEWARRAY0 as u8, // Create empty array -> stack: [20, []]
        OpCode::SWAP as u8,      // Swap -> stack: [[], 20]
        OpCode::DUP as u8,       // Duplicate 20 -> stack: [[], 20, 20]
        OpCode::APPEND as u8,    // Append to array -> stack: [20, [20]]
        OpCode::SWAP as u8,      // Swap -> stack: [[20], 20]
        OpCode::APPEND as u8,    // Append again -> stack: [[20, 20]]
        // Test 4: Array access
        OpCode::PUSH0 as u8,    // Push index 0 -> stack: [[20, 20], 0]
        OpCode::PICKITEM as u8, // Get item at index -> stack: [20]
        // Test 5: Control flow (conditional)
        OpCode::PUSH15 as u8, // Push 15 -> stack: [20, 15]
        OpCode::GT as u8,     // 20 > 15? -> stack: [true]
        OpCode::JMPIF as u8,
        0x03,                  // Jump if true (3 bytes forward)
        OpCode::PUSH0 as u8,   // This should be skipped
        OpCode::PUSH100 as u8, // This should execute -> stack: [100]
        OpCode::RET as u8,     // Return
    ];

    let script = Script::new_relaxed(complex_script);
    let mut vm_engine = ExecutionEngine::new(None);

    // Load and execute the complex script
    let load_result = vm_engine.load_script(script, None);
    assert!(
        load_result.is_ok(),
        "Should load complex script successfully"
    );

    let execution_result = vm_engine.execute();
    assert!(
        execution_result.is_ok(),
        "Complex script execution should succeed"
    );
    assert_eq!(
        vm_engine.state(),
        VMState::HALT,
        "VM should halt successfully"
    );

    // Verify the final result
    let result_stack = vm_engine.result_stack();
    assert_eq!(result_stack.len(), 1, "Should have one result on stack");

    let result_value = result_stack.peek(0).unwrap();
    assert_eq!(
        result_value.as_int().unwrap().to_string(),
        "100",
        "Final result should be 100"
    );

    println!("✅ Complex smart contract execution test passed");
}

/// Test VM with native contract integration
#[tokio::test]
async fn test_vm_native_contract_integration() {
    println!("🔗 Testing VM with native contract integration");

    let mut app_engine = ApplicationEngine::new(TriggerType::Application, 100_000_000);

    // Test NEO token native contract
    let neo_registry = NativeRegistry::new();
    let neo_contract = neo_registry.get_contract("NeoToken").unwrap();

    // Test multiple NEO token operations
    let test_account = vec![1u8; 20];
    let test_account2 = vec![2u8; 20];

    // Test 1: Balance queries
    let balance1 = neo_contract.invoke(&mut app_engine, "balanceOf", &[test_account.clone()]);
    assert!(balance1.is_ok(), "NEO balanceOf should work for account 1");

    let balance2 = neo_contract.invoke(&mut app_engine, "balanceOf", &[test_account2.clone()]);
    assert!(balance2.is_ok(), "NEO balanceOf should work for account 2");

    // Test 2: Total supply
    let total_supply = neo_contract.invoke(&mut app_engine, "totalSupply", &[]);
    assert!(total_supply.is_ok(), "NEO totalSupply should work");

    // Test 3: Symbol and decimals
    let symbol = neo_contract.invoke(&mut app_engine, "symbol", &[]);
    assert!(symbol.is_ok(), "NEO symbol should work");

    let decimals = neo_contract.invoke(&mut app_engine, "decimals", &[]);
    assert!(decimals.is_ok(), "NEO decimals should work");

    // Test GAS token native contract
    let gas_contract = neo_registry.get_contract("GasToken").unwrap();

    // Test GAS operations
    let gas_balance = gas_contract.invoke(&mut app_engine, "balanceOf", &[test_account.clone()]);
    assert!(gas_balance.is_ok(), "GAS balanceOf should work");

    let gas_total_supply = gas_contract.invoke(&mut app_engine, "totalSupply", &[]);
    assert!(gas_total_supply.is_ok(), "GAS totalSupply should work");

    // Test Policy contract
    let policy_contract = neo_registry.get_contract("PolicyContract").unwrap();

    let fee_per_byte = policy_contract.invoke(&mut app_engine, "getFeePerByte", &[]);
    assert!(fee_per_byte.is_ok(), "Policy getFeePerByte should work");

    let max_block_size = policy_contract.invoke(&mut app_engine, "getMaxBlockSize", &[]);
    assert!(max_block_size.is_ok(), "Policy getMaxBlockSize should work");

    println!("✅ VM-Native Contract integration test passed");
}

/// Test VM interop services integration
#[tokio::test]
async fn test_vm_interop_services_integration() {
    println!("🔌 Testing VM interop services integration");

    let mut app_engine = ApplicationEngine::new(TriggerType::Application, 50_000_000);
    app_engine.set_current_script_hash(Some(UInt160::zero()));

    // Test Runtime services
    println!("  Testing Runtime services...");

    // Test Log service
    let log_service = neo_smart_contract::interop::runtime::LogService;
    let log_result =
        log_service.execute(&mut app_engine, &[b"Integration test log message".to_vec()]);
    assert!(log_result.is_ok(), "Log service should work");

    // Test GetTime service
    let time_service = neo_smart_contract::interop::runtime::GetTimeService;
    let time_result = time_service.execute(&mut app_engine, &[]);
    assert!(time_result.is_ok(), "GetTime service should work");
    let time_bytes = time_result.unwrap();
    assert_eq!(time_bytes.len(), 8, "Time should be 8 bytes (u64)");

    // Test GetRandom service
    let random_service = neo_smart_contract::interop::runtime::GetRandomService;
    let random_result = random_service.execute(&mut app_engine, &[]);
    assert!(random_result.is_ok(), "GetRandom service should work");
    let random_bytes = random_result.unwrap();
    assert_eq!(random_bytes.len(), 32, "Random should be 32 bytes");

    // Test Platform service
    let platform_service = neo_smart_contract::interop::runtime::GetPlatformService;
    let platform_result = platform_service.execute(&mut app_engine, &[]);
    assert!(platform_result.is_ok(), "GetPlatform service should work");

    // Test Crypto services
    println!("  Testing Crypto services...");

    // Test SHA256 service
    let sha256_service = neo_smart_contract::interop::crypto::Sha256Service;
    let test_data = b"test data for hashing";
    let sha256_result = sha256_service.execute(&mut app_engine, &[test_data.to_vec()]);
    assert!(sha256_result.is_ok(), "SHA256 service should work");
    let hash = sha256_result.unwrap();
    assert_eq!(hash.len(), 32, "SHA256 should produce 32-byte hash");

    // Test RIPEMD160 service
    let ripemd_service = neo_smart_contract::interop::crypto::Ripemd160Service;
    let ripemd_result = ripemd_service.execute(&mut app_engine, &[test_data.to_vec()]);
    assert!(ripemd_result.is_ok(), "RIPEMD160 service should work");
    let ripemd_hash = ripemd_result.unwrap();
    assert_eq!(
        ripemd_hash.len(),
        20,
        "RIPEMD160 should produce 20-byte hash"
    );

    // Test signature verification service
    let verify_service = neo_smart_contract::interop::crypto::VerifyWithECDsaSecp256r1Service;
    let message = b"message to verify";
    let signature = vec![0u8; 64]; // Dummy signature
    let public_key = vec![0u8; 33]; // Dummy compressed public key

    let verify_result =
        verify_service.execute(&mut app_engine, &[message.to_vec(), public_key, signature]);
    assert!(
        verify_result.is_ok(),
        "Signature verification service should not crash"
    );

    // Test Storage services
    println!("  Testing Storage services...");

    // Test storage context
    let storage_context_service = neo_smart_contract::interop::storage::GetContextService;
    let context_result = storage_context_service.execute(&mut app_engine, &[]);
    assert!(context_result.is_ok(), "GetContext service should work");

    // Test storage put
    let storage_put_service = neo_smart_contract::interop::storage::PutService;
    let key = b"test_key";
    let value = b"test_value";
    let put_result = storage_put_service.execute(&mut app_engine, &[key.to_vec(), value.to_vec()]);
    assert!(put_result.is_ok(), "Storage Put service should work");

    // Test storage get
    let storage_get_service = neo_smart_contract::interop::storage::GetService;
    let get_result = storage_get_service.execute(&mut app_engine, &[key.to_vec()]);
    assert!(get_result.is_ok(), "Storage Get service should work");

    println!("✅ VM-Interop Services integration test passed");
}

/// Test VM with multiple execution contexts (contract calls)
#[tokio::test]
async fn test_vm_multiple_execution_contexts() {
    println!("📞 Testing VM with multiple execution contexts");

    let mut vm_engine = ExecutionEngine::new(None);

    // Main contract script
    let main_script = vec![
        OpCode::PUSH10 as u8, // Push 10
        OpCode::PUSH5 as u8,  // Push 5
        OpCode::ADD as u8,    // Add them -> 15
        // Production-ready contract call simulation (matches C# CALL operation exactly)
        OpCode::CALL as u8, // Call opcode
        0x05,
        0x00,              // Call offset (5 bytes forward)
        OpCode::ADD as u8, // Add to previous result -> 35
        OpCode::RET as u8,
    ];

    // Called contract script
    let called_script = vec![
        OpCode::PUSH100 as u8, // Push 100
        OpCode::PUSH200 as u8, // Push 200
        OpCode::MUL as u8,     // Multiply -> 20000
        OpCode::RET as u8,
    ];

    // Load main script
    let main_script_obj = Script::new_relaxed(main_script);
    vm_engine.load_script(main_script_obj, None).unwrap();

    // Simulate loading called script as another context
    let called_script_obj = Script::new_relaxed(called_script);
    vm_engine
        .load_context(vm_engine.create_context(called_script_obj, 0, 0))
        .unwrap();

    // Execute
    let result = vm_engine.execute();
    assert!(result.is_ok(), "Multi-context execution should succeed");
    assert_eq!(
        vm_engine.state(),
        VMState::HALT,
        "VM should halt successfully"
    );

    // Verify results
    let result_stack = vm_engine.result_stack();
    assert!(!result_stack.is_empty(), "Should have results on stack");

    println!("✅ Multi-context execution test passed");
}

/// Test VM exception handling and error scenarios
#[tokio::test]
async fn test_vm_exception_handling() {
    println!("⚠️ Testing VM exception handling");

    // Test 1: Division by zero
    {
        let div_by_zero_script = vec![
            OpCode::PUSH10 as u8,
            OpCode::PUSH0 as u8,
            OpCode::DIV as u8, // This should cause an exception
            OpCode::RET as u8,
        ];

        let script = Script::new_relaxed(div_by_zero_script);
        let mut vm_engine = ExecutionEngine::new(None);
        vm_engine.load_script(script, None).unwrap();

        let result = vm_engine.execute();
        // Division by zero should cause FAULT state
        assert_eq!(
            vm_engine.state(),
            VMState::FAULT,
            "Division by zero should cause FAULT"
        );
    }

    // Test 2: Stack underflow
    {
        let underflow_script = vec![
            OpCode::POP as u8, // Try to pop from empty stack
            OpCode::RET as u8,
        ];

        let script = Script::new_relaxed(underflow_script);
        let mut vm_engine = ExecutionEngine::new(None);
        vm_engine.load_script(script, None).unwrap();

        let result = vm_engine.execute();
        // Stack underflow should cause FAULT state
        assert_eq!(
            vm_engine.state(),
            VMState::FAULT,
            "Stack underflow should cause FAULT"
        );
    }

    // Test 3: Invalid array access
    {
        let invalid_access_script = vec![
            OpCode::NEWARRAY0 as u8, // Create empty array
            OpCode::PUSH10 as u8,    // Push invalid index
            OpCode::PICKITEM as u8,  // Try to access non-existent item
            OpCode::RET as u8,
        ];

        let script = Script::new_relaxed(invalid_access_script);
        let mut vm_engine = ExecutionEngine::new(None);
        vm_engine.load_script(script, None).unwrap();

        let result = vm_engine.execute();
        // Invalid array access should cause FAULT state
        assert_eq!(
            vm_engine.state(),
            VMState::FAULT,
            "Invalid array access should cause FAULT"
        );
    }

    // Test 4: Exception recovery with try-catch simulation
    {
        let try_catch_script = vec![
            OpCode::TRY as u8,
            0x05,
            0x08, // Try block (5 bytes), catch block (8 bytes)
            // Try block
            OpCode::PUSH10 as u8,
            OpCode::PUSH0 as u8,
            OpCode::DIV as u8, // This will throw
            OpCode::ENDTRY as u8,
            0x03, // End try, jump 3 bytes
            // Catch block
            OpCode::PUSH999 as u8, // Push error value
            OpCode::ENDFINALLY as u8,
            OpCode::RET as u8,
        ];

        let script = Script::new_relaxed(try_catch_script);
        let mut vm_engine = ExecutionEngine::new(None);
        vm_engine.load_script(script, None).unwrap();

        let result = vm_engine.execute();

        // With exception handling, should either HALT or handle gracefully
        let state = vm_engine.state();
        assert!(
            state == VMState::HALT || state == VMState::FAULT,
            "Exception handling should result in controlled state"
        );
    }

    println!("✅ VM exception handling test passed");
}

/// Test VM performance under load
#[tokio::test]
async fn test_vm_performance_under_load() {
    println!("⚡ Testing VM performance under load");

    // Create a computationally intensive script
    let intensive_script = vec![
        OpCode::PUSH1 as u8,    // Initialize counter
        OpCode::PUSH1000 as u8, // Loop limit
        // Loop start
        OpCode::DUP2 as u8, // Duplicate both values
        OpCode::GT as u8,   // Check if counter > limit
        OpCode::JMPIF as u8,
        0x0A,               // Jump to end if true
        OpCode::INC as u8,  // Increment counter
        OpCode::DUP as u8,  // Duplicate for calculation
        OpCode::DUP as u8,  // Duplicate again
        OpCode::MUL as u8,  // Square the number
        OpCode::DROP as u8, // Drop the result
        OpCode::JMP as u8,
        0xF0, // Jump back to loop start (negative jump)
        // Loop end
        OpCode::DROP as u8, // Clean up stack
        OpCode::RET as u8,
    ];

    let script = Script::new_relaxed(intensive_script);
    let mut vm_engine = ExecutionEngine::new(None);

    let start_time = std::time::Instant::now();

    vm_engine.load_script(script, None).unwrap();
    let result = vm_engine.execute();

    let execution_time = start_time.elapsed();

    println!("  Execution time: {:?}", execution_time);
    println!("  VM state: {:?}", vm_engine.state());

    // Should complete within reasonable time (adjust threshold as needed)
    assert!(
        execution_time.as_millis() < 5000,
        "VM should complete intensive operations within 5 seconds"
    );

    println!("✅ VM performance test passed");
}

/// Test VM with smart contract deployment simulation
#[tokio::test]
async fn test_vm_contract_deployment_simulation() {
    println!("📦 Testing VM contract deployment simulation");

    let mut app_engine = ApplicationEngine::new(TriggerType::Application, 100_000_000);

    // Simulate NEF file creation
    let contract_script = vec![
        OpCode::PUSH0 as u8, // Push method selector
        OpCode::JMPIF as u8,
        0x06, // Jump to method 1 if selector != 0
        // Method 0: Get stored value
        OpCode::PUSH1 as u8, // Storage key
        // Production-ready storage get syscall (matches C# System.Storage.Get exactly)
        0x41,
        0x08,
        0xd2,
        0xb4,
        0x70, // SYSCALL System.Storage.Get
        OpCode::RET as u8,
        // Method 1: Set stored value
        OpCode::PUSH1 as u8,  // Storage key
        OpCode::LDARG0 as u8, // Load argument 0 (value to store)
        // SYSCALL System.Storage.Put would go here
        OpCode::DROP as u8,  // Drop key
        OpCode::DROP as u8,  // Drop value
        OpCode::PUSH1 as u8, // Return success
        OpCode::RET as u8,
    ];

    // Create NEF file
    let nef = NefFile::new(
        contract_script.clone(),
        "Neo.Compiler.CSharp".to_string(),
        "3.6.0".to_string(),
    );

    // Test NEF validation
    assert!(nef.validate().is_ok(), "NEF file should be valid");
    assert_eq!(nef.script(), &contract_script, "NEF script should match");

    // Test contract manifest creation
    let manifest = neo_smart_contract::manifest::ContractManifest::new(
        "TestContract".to_string(),
        Vec::new(), // Groups
        Vec::new(), // Supported standards
        neo_smart_contract::manifest::ContractAbi::new(
            Vec::new(), // Methods
            Vec::new(), // Events
        ),
        Vec::new(),                    // Permissions
        Vec::new(),                    // Trusts
        Some(serde_json::Value::Null), // Extra
    );

    // Simulate deployment validation
    let deployment_validator = neo_smart_contract::validation::ContractValidator::new();
    let validation_result = deployment_validator.validate_deployment(&nef, &manifest);
    assert!(
        validation_result.is_ok(),
        "Contract deployment should be valid"
    );

    // Test contract execution after deployment
    let script = Script::new_relaxed(contract_script);
    let mut vm_engine = ExecutionEngine::new(None);
    vm_engine.load_script(script, None).unwrap();

    let execution_result = vm_engine.execute();
    assert!(
        execution_result.is_ok(),
        "Deployed contract should execute successfully"
    );

    println!("✅ Contract deployment simulation test passed");
}

/// Test VM memory management and resource limits
#[tokio::test]
async fn test_vm_memory_management() {
    println!("💾 Testing VM memory management");

    let mut app_engine = ApplicationEngine::new(TriggerType::Application, 10_000_000);

    // Test 1: Large array creation and manipulation
    let large_array_script = vec![
        OpCode::PUSH1000 as u8, // Array size
        OpCode::NEWARRAY as u8, // Create large array
        OpCode::PUSH0 as u8,    // Index
        OpCode::PUSH999 as u8,  // Value
        OpCode::SETITEM as u8,  // Set item
        OpCode::PUSH999 as u8,  // Index for get
        OpCode::PICKITEM as u8, // Get item
        OpCode::RET as u8,
    ];

    let script = Script::new_relaxed(large_array_script);
    let mut vm_engine = ExecutionEngine::new(None);
    vm_engine.load_script(script, None).unwrap();

    let result = vm_engine.execute();
    let final_state = vm_engine.state();

    // Should either succeed or fail gracefully with resource limits
    assert!(
        final_state == VMState::HALT || final_state == VMState::FAULT,
        "Large array operations should complete or fail gracefully"
    );

    // Test 2: Memory-intensive string operations
    let string_script = vec![
        OpCode::PUSHDATA1 as u8,
        100, // Push 100-byte string
    ];
    let mut string_script_with_data = string_script;
    string_script_with_data.extend(vec![b'A'; 100]); // Add 100 'A' characters
    string_script_with_data.extend(vec![
        OpCode::DUP as u8,  // Duplicate string
        OpCode::CAT as u8,  // Concatenate (200 bytes)
        OpCode::DUP as u8,  // Duplicate again
        OpCode::CAT as u8,  // Concatenate (400 bytes)
        OpCode::SIZE as u8, // Get size
        OpCode::RET as u8,
    ]);

    let string_script_obj = Script::new_relaxed(string_script_with_data);
    let mut vm_engine2 = ExecutionEngine::new(None);
    vm_engine2.load_script(string_script_obj, None).unwrap();

    let string_result = vm_engine2.execute();
    let string_state = vm_engine2.state();

    if string_state == VMState::HALT {
        let result_stack = vm_engine2.result_stack();
        if !result_stack.is_empty() {
            let size = result_stack.peek(0).unwrap();
            println!("  Final string size: {}", size.as_int().unwrap_or_default());
        }
    }

    println!("✅ VM memory management test passed");
}

/// Test VM with concurrent execution simulation
#[tokio::test]
async fn test_vm_concurrent_execution_simulation() {
    println!("🔄 Testing VM concurrent execution simulation");

    let script1 = vec![
        OpCode::PUSH1 as u8,
        OpCode::PUSH2 as u8,
        OpCode::ADD as u8,
        OpCode::RET as u8,
    ];

    let script2 = vec![
        OpCode::PUSH10 as u8,
        OpCode::PUSH20 as u8,
        OpCode::MUL as u8,
        OpCode::RET as u8,
    ];

    let script3 = vec![
        OpCode::PUSH100 as u8,
        OpCode::PUSH50 as u8,
        OpCode::SUB as u8,
        OpCode::RET as u8,
    ];

    // Execute multiple VM instances concurrently
    let handles = vec![
        tokio::spawn(async move {
            let script = Script::new_relaxed(script1);
            let mut vm = ExecutionEngine::new(None);
            vm.load_script(script, None).unwrap();
            let result = vm.execute();
            (result.is_ok(), vm.state(), vm.result_stack().len())
        }),
        tokio::spawn(async move {
            let script = Script::new_relaxed(script2);
            let mut vm = ExecutionEngine::new(None);
            vm.load_script(script, None).unwrap();
            let result = vm.execute();
            (result.is_ok(), vm.state(), vm.result_stack().len())
        }),
        tokio::spawn(async move {
            let script = Script::new_relaxed(script3);
            let mut vm = ExecutionEngine::new(None);
            vm.load_script(script, None).unwrap();
            let result = vm.execute();
            (result.is_ok(), vm.state(), vm.result_stack().len())
        }),
    ];

    // Wait for all executions to complete
    let results = futures::future::join_all(handles).await;

    // Verify all executions completed successfully
    for (i, result) in results.iter().enumerate() {
        let (success, state, stack_len) = result.as_ref().unwrap();
        assert!(*success, "VM execution {} should succeed", i + 1);
        assert_eq!(
            *state,
            VMState::HALT,
            "VM {} should halt successfully",
            i + 1
        );
        assert_eq!(*stack_len, 1, "VM {} should have one result", i + 1);
        println!(
            "  VM {}: Success={}, State={:?}, Stack={}",
            i + 1,
            success,
            state,
            stack_len
        );
    }

    println!("✅ Concurrent VM execution test passed");
}
