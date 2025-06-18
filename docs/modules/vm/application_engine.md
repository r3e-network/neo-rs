# ApplicationEngine Module

## Overview

The ApplicationEngine module extends the Neo Virtual Machine (NeoVM) with Neo blockchain-specific functionality. It provides a specialized execution engine for running smart contracts in the Neo blockchain environment, including gas tracking, permission management, and blockchain state access.

## Implementation Details

### ApplicationEngine Structure

The ApplicationEngine struct extends the ExecutionEngine:

```rust
pub struct ApplicationEngine {
    /// The base execution engine
    engine: ExecutionEngine,
    
    /// The gas consumed by the execution
    gas_consumed: i64,
    
    /// The maximum gas allowed to be consumed
    gas_limit: i64,
    
    /// The price per instruction
    price_per_instruction: i64,
    
    /// The trigger of execution
    trigger: TriggerType,
    
    /// The snapshots of blockchain state
    snapshots: HashMap<Vec<u8>, Vec<u8>>,
    
    /// The notification messages
    notifications: Vec<NotificationEvent>,
    
    /// The interop service
    interop_service: InteropService,
}

/// The trigger types for script execution
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerType {
    /// The script is being executed directly
    Application,
    
    /// The script is being executed as part of a verification
    Verification,
    
    /// The script is being executed in the system context
    System,
}

/// A notification event emitted by a smart contract
pub struct NotificationEvent {
    /// The contract that emitted the notification
    pub script_hash: Vec<u8>,
    
    /// The name of the notification
    pub name: String,
    
    /// The arguments of the notification
    pub arguments: Vec<StackItem>,
}
```

### Core Functionality

The ApplicationEngine module provides the following core functionality:

1. **Gas Tracking**: Tracking and limiting gas consumption during execution
2. **Trigger Management**: Handling different execution contexts
3. **Blockchain Interaction**: Providing access to blockchain state
4. **Notification Handling**: Managing smart contract notifications
5. **Interop Services**: Providing blockchain-specific interop services
6. **Permission Management**: Enforcing permission rules for smart contracts

### API

```rust
impl ApplicationEngine {
    /// Creates a new application engine
    pub fn new(trigger: TriggerType, gas_limit: i64) -> Self;
    
    /// Returns the gas consumed
    pub fn gas_consumed(&self) -> i64;
    
    /// Returns the gas limit
    pub fn gas_limit(&self) -> i64;
    
    /// Returns the trigger type
    pub fn trigger(&self) -> TriggerType;
    
    /// Returns the notifications
    pub fn notifications(&self) -> &[NotificationEvent];
    
    /// Returns the interop service
    pub fn interop_service(&self) -> &InteropService;
    
    /// Returns the interop service (mutable)
    pub fn interop_service_mut(&mut self) -> &mut InteropService;
    
    /// Consumes gas
    pub fn consume_gas(&mut self, gas: i64) -> Result<()>;
    
    /// Adds a notification
    pub fn add_notification(&mut self, notification: NotificationEvent);
    
    /// Gets a snapshot of the blockchain state
    pub fn get_snapshot(&self, key: &[u8]) -> Option<&[u8]>;
    
    /// Sets a snapshot of the blockchain state
    pub fn set_snapshot(&mut self, key: Vec<u8>, value: Vec<u8>);
    
    /// Executes a script
    pub fn execute(&mut self, script: Script) -> VMState;
    
    /// Loads a script
    pub fn load_script(&mut self, script: Script, rvcount: i32, initial_position: usize) -> Result<ExecutionContext>;
}

impl From<ApplicationEngine> for ExecutionEngine {
    fn from(engine: ApplicationEngine) -> Self;
}
```

## Usage Examples

```rust
// Create a new application engine
let mut engine = ApplicationEngine::new(TriggerType::Application, 10_000_000);

// Register blockchain-specific interop services
let interop = engine.interop_service_mut();
interop.register(InteropDescriptor {
    name: "Neo.Blockchain.GetHeight".to_string(),
    handler: |engine| {
        // Get the current blockchain height
        let height = 12345; // Placeholder
        
        // Push the height onto the stack
        let context = engine.current_context_mut().unwrap();
        context.evaluation_stack_mut().push(StackItem::from_int(height));
        
        Ok(())
    },
    price: 1,
});

// Create a script that calls the interop service
let mut builder = ScriptBuilder::new();
builder
    .emit_syscall("Neo.Blockchain.GetHeight")
    .emit_opcode(OpCode::RET);
let script = builder.to_script();

// Execute the script
let state = engine.execute(script);
assert_eq!(state, VMState::HALT);

// Check gas consumption
println!("Gas consumed: {}", engine.gas_consumed());

// Check the result
let result = engine.result_stack().pop().unwrap();
println!("Result: {}", result.as_int().unwrap());
```

## Considerations

1. **Gas Calculation**: Gas must be calculated accurately to prevent abuse.

2. **State Isolation**: Execution state must be properly isolated for different contracts.

3. **Blockchain Integration**: The engine must integrate properly with the blockchain state.

4. **Error Handling**: Errors must be handled properly to ensure contract execution safety.

5. **Storage Limitations**: Storage operations must respect blockchain limitations.

6. **Permission Model**: The engine must enforce the Neo permission model.

7. **Compatibility**: The behavior must match the C# implementation to ensure consistent contract execution.

## Implementation Approach

The ApplicationEngine implementation follows these principles:

1. Extend the ExecutionEngine with blockchain-specific functionality
2. Implement gas tracking and limitation
3. Provide blockchain state access through snapshots
4. Register blockchain-specific interop services
5. Enforce permission rules for different trigger types
6. Ensure compatibility with the C# implementation 