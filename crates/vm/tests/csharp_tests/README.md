# C# JSON Test Suite for Neo VM

This directory contains a comprehensive test suite that executes C# Neo VM JSON test files to ensure compatibility between the Rust and C# implementations.

## 🎯 **Major Achievement: PUSHINT64 Bug Fixed!**

During the reorganization, we discovered and fixed a critical bug in the `OperandSizePrefix::size()` method that was preventing PUSHINT64 (and other large operand instructions) from working correctly. The fix ensures all PUSHINT instruction families work perfectly:

- ✅ **PUSHINT8** - Working
- ✅ **PUSHINT16** - Working  
- ✅ **PUSHINT32** - Working
- ✅ **PUSHINT64** - **Now working** (was broken)
- ✅ **PUSHINT128** - Now working (operand size 16 added)
- ✅ **PUSHINT256** - Now working (operand size 32 added)

## 📁 **Directory Structure**

```
csharp_tests/
├── mod.rs                    # Module declarations and re-exports
├── common.rs                 # Shared data structures (VMUT, VMUTTest, etc.)
├── runner.rs                 # JsonTestRunner implementation
├── opcodes/                  # Opcode-specific tests
│   ├── mod.rs               # Opcode module declarations
│   ├── push.rs              # PUSH* opcode tests (PUSHNULL, PUSHDATA*, PUSHINT*, etc.)
│   ├── arithmetic.rs        # Arithmetic operations (ADD, SUB, MUL, DIV, etc.)
│   ├── stack.rs             # Stack manipulation (DUP, SWAP, ROT, etc.)
│   ├── control.rs           # Control flow (JMP, CALL, RET, etc.)
│   ├── arrays.rs            # Array operations (NEWARRAY, APPEND, etc.)
│   ├── bitwise_logic.rs     # Bitwise and logical operations (AND, OR, XOR, etc.)
│   ├── slot.rs              # Slot operations (LDLOC, STLOC, etc.)
│   ├── splice.rs            # Splice operations (CAT, SUBSTR, etc.)
│   └── types.rs             # Type operations (ISNULL, ISTYPE, etc.)
├── integration/              # Integration tests
│   ├── mod.rs               # Integration module declarations
│   ├── comprehensive.rs     # Full test suite runner
│   └── performance.rs       # Benchmark and performance tests
└── unit/                     # Unit tests
    ├── mod.rs               # Unit test module declarations
    ├── script_compilation.rs # Script compilation tests
    ├── instruction_parsing.rs # Instruction parsing tests
    └── vm_execution.rs      # Basic VM execution tests
```

## 🚀 **Usage**

### Run Specific Test Categories

```bash
# Test PUSH opcodes
cargo test -p neo-vm --test csharp_json_tests csharp_tests::opcodes::push

# Test arithmetic opcodes
cargo test -p neo-vm --test csharp_json_tests csharp_tests::opcodes::arithmetic

# Test script compilation
cargo test -p neo-vm --test csharp_json_tests csharp_tests::unit::script_compilation

# Test VM execution
cargo test -p neo-vm --test csharp_json_tests csharp_tests::unit::vm_execution
```

### Run Specific Tests

```bash
# Test PUSHINT opcodes specifically
cargo test -p neo-vm --test csharp_json_tests csharp_tests::opcodes::push::test_pushint_opcodes -- --nocapture

# Test PUSHA opcode execution
cargo test -p neo-vm --test csharp_json_tests csharp_tests::opcodes::push::test_pusha_vm_execution -- --nocapture

# Test script compilation edge cases
cargo test -p neo-vm --test csharp_json_tests csharp_tests::unit::script_compilation::test_script_compilation_edge_cases
```

### Run Integration Tests

```bash
# Run comprehensive test suite
cargo test -p neo-vm --test csharp_json_tests csharp_tests::integration::comprehensive

# Run performance benchmarks
cargo test -p neo-vm --test csharp_json_tests csharp_tests::integration::performance
```

## 📊 **Test Categories**

### **Opcodes Tests** (`opcodes/`)
- **push.rs**: All PUSH-related opcodes including PUSHNULL, PUSHDATA*, PUSHINT*, PUSHA
- **arithmetic.rs**: Mathematical operations
- **stack.rs**: Stack manipulation operations
- **control.rs**: Control flow operations
- **arrays.rs**: Array operations
- **bitwise_logic.rs**: Bitwise and logical operations
- **slot.rs**: Local/static slot operations
- **splice.rs**: String/buffer operations
- **types.rs**: Type checking operations

### **Integration Tests** (`integration/`)
- **comprehensive.rs**: Full C# test suite execution
- **performance.rs**: Benchmarks and error handling tests

### **Unit Tests** (`unit/`)
- **script_compilation.rs**: Script compilation edge cases
- **instruction_parsing.rs**: Instruction parsing tests
- **vm_execution.rs**: Basic VM execution tests

## 🔧 **Key Components**

### **Common Structures** (`common.rs`)
- `VMUT`: VM Unit Test structure
- `VMUTTest`: Individual test case
- `VMUTStep`: Test execution step
- `VMUTExecutionEngineState`: Expected VM state
- `VMUTStackItem`: Stack item representation

### **Test Runner** (`runner.rs`)
- `JsonTestRunner`: Main test execution engine
- Script compilation from opcode strings to bytecode
- Test execution and result verification
- Support for all C# test file formats

## 🎯 **Benefits of New Structure**

1. **🔍 Easy Navigation**: Find specific tests quickly
2. **📝 Better Organization**: Logical grouping by functionality
3. **🚀 Faster Development**: Add new tests in the right category
4. **🧪 Targeted Testing**: Run only the tests you need
5. **📚 Clear Documentation**: Each module is self-documenting
6. **🔧 Maintainability**: Smaller, focused files are easier to maintain

## 🐛 **Bug Fixes Included**

- **PUSHINT64 Operand Parsing**: Fixed `OperandSizePrefix::size()` to handle 8, 16, and 32-byte operands
- **Module Organization**: Resolved naming conflicts and import issues
- **Test Structure**: Improved test organization and execution

This modular structure makes the C# JSON test suite much more manageable and provides a solid foundation for comprehensive Neo VM testing!
