//! Error types for the Neo VM crate
//!
//! This module provides comprehensive error handling for VM operations,
//! including instruction parsing, execution errors, and stack management.

use thiserror::Error;

/// VM execution errors
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum VmError {
    /// Parse error with context
    #[error("Parse error: {message}")]
    Parse { message: String },

    /// Invalid instruction with opcode
    #[error("Invalid instruction: opcode {opcode:#04x}, reason: {reason}")]
    InvalidInstruction { opcode: u8, reason: String },

    /// Invalid opcode
    #[error("Invalid opcode: {opcode:#04x}")]
    InvalidOpCode { opcode: u8 },

    /// Unsupported operation
    #[error("Unsupported operation: {operation}")]
    UnsupportedOperation { operation: String },

    /// Catchable exception that can be handled by TRY/CATCH blocks
    #[error("Catchable exception: {message}")]
    CatchableException { message: String },

    /// Invalid operation with context
    #[error("Invalid operation: {operation}, reason: {reason}")]
    InvalidOperation { operation: String, reason: String },

    /// Invalid operand
    #[error("Invalid operand: expected {expected}, got {actual}")]
    InvalidOperand { expected: String, actual: String },

    /// Invalid script
    #[error("Invalid script: {reason}")]
    InvalidScript { reason: String },

    /// Stack underflow
    #[error(
        "Stack underflow: attempted to access {requested} items, but only {available} available"
    )]
    StackUnderflow { requested: usize, available: usize },

    /// Stack overflow
    #[error("Stack overflow: maximum stack size {max_size} exceeded")]
    StackOverflow { max_size: usize },

    /// Arithmetic overflow
    #[error("Arithmetic overflow in operation: {operation}")]
    Overflow { operation: String },

    /// Arithmetic underflow
    #[error("Arithmetic underflow in operation: {operation}")]
    Underflow { operation: String },

    /// Division by zero
    #[error("Division by zero in operation: {operation}")]
    DivisionByZero { operation: String },

    /// Insufficient stack items
    #[error("Insufficient stack items: required {required}, available {available}")]
    InsufficientStackItems { required: usize, available: usize },

    /// Invalid type conversion
    #[error("Invalid type conversion: cannot convert {from} to {to}")]
    InvalidType { from: String, to: String },

    /// Execution halted
    #[error("Execution halted: {reason}")]
    ExecutionHalted { reason: String },

    /// VM fault state
    #[error("VM fault: {fault_type}, details: {details}")]
    VmFault { fault_type: String, details: String },

    /// I/O error
    #[error("I/O error: {message}")]
    Io { message: String },

    /// Memory limit exceeded
    #[error("Memory limit exceeded: used {used} bytes, limit {limit} bytes")]
    MemoryLimitExceeded { used: usize, limit: usize },

    /// Instruction limit exceeded
    #[error("Instruction limit exceeded: executed {executed}, limit {limit}")]
    InstructionLimitExceeded { executed: u64, limit: u64 },

    /// Call depth limit exceeded
    #[error("Call depth limit exceeded: depth {depth}, limit {limit}")]
    CallDepthLimitExceeded { depth: usize, limit: usize },

    /// Gas exhausted
    #[error("Gas exhausted: used {used}, limit {limit}")]
    GasExhausted { used: u64, limit: u64 },

    /// Invalid contract state
    #[error("Invalid contract state: {reason}")]
    InvalidContractState { reason: String },

    /// Interop service error
    #[error("Interop service error: service {service}, error: {error}")]
    InteropService { service: String, error: String },

    /// Script execution timeout
    #[error("Script execution timeout: exceeded {timeout_ms}ms")]
    ExecutionTimeout { timeout_ms: u64 },

    /// Invalid script hash
    #[error("Invalid script hash: {hash}")]
    InvalidScriptHash { hash: String },

    /// Contract not found
    #[error("Contract not found: {hash}")]
    ContractNotFound { hash: String },

    /// Method not found
    #[error("Method not found: contract {contract}, method {method}")]
    MethodNotFound { contract: String, method: String },

    /// Invalid parameters
    #[error("Invalid parameters: expected {expected}, got {actual}")]
    InvalidParameters { expected: String, actual: String },

    /// Invalid witness
    #[error("Invalid witness: {reason}")]
    InvalidWitness { reason: String },

    /// Verification failed
    #[error("Verification failed: {reason}")]
    VerificationFailed { reason: String },

    /// Invalid jump
    #[error("Invalid jump: position {0}")]
    InvalidJump(i32),

    /// Unhandled exception
    #[error("Unhandled exception")]
    UnhandledException(crate::stack_item::StackItem),

    /// Max try nesting depth exceeded
    #[error("Max try nesting depth exceeded")]
    MaxTryNestingDepthExceeded,

    /// Abort operation
    #[error("ABORT is executed")]
    Abort,

    /// Assert failed
    #[error("ASSERT is executed with false result")]
    AssertFailed,

    /// Implementation provided I/O error (for testing)
    #[cfg(test)]
    #[allow(dead_code)]
    #[error("Mock I/O error: {message}")]
    MockIo { message: String },
}

impl VmError {
    /// Create a new parse error
    pub fn parse<S: Into<String>>(message: S) -> Self {
        Self::Parse {
            message: message.into(),
        }
    }

    /// Create a new invalid instruction error
    pub fn invalid_instruction<S: Into<String>>(opcode: u8, reason: S) -> Self {
        Self::InvalidInstruction {
            opcode,
            reason: reason.into(),
        }
    }

    /// Create a new invalid opcode error
    #[must_use] 
    pub const fn invalid_opcode(opcode: u8) -> Self {
        Self::InvalidOpCode { opcode }
    }

    /// Create a new unsupported operation error
    pub fn unsupported_operation<S: Into<String>>(operation: S) -> Self {
        Self::UnsupportedOperation {
            operation: operation.into(),
        }
    }

    /// Create a catchable exception error used for TRY/CATCH interop
    pub fn catchable_exception_msg<S: Into<String>>(message: S) -> Self {
        Self::CatchableException {
            message: message.into(),
        }
    }

    /// Create a new invalid operation error
    pub fn invalid_operation<S: Into<String>>(operation: S, reason: S) -> Self {
        Self::InvalidOperation {
            operation: operation.into(),
            reason: reason.into(),
        }
    }

    /// Create a new invalid operation error with message
    pub fn invalid_operation_msg<S: Into<String>>(msg: S) -> Self {
        let msg = msg.into();
        Self::InvalidOperation {
            operation: msg.clone(),
            reason: msg,
        }
    }

    /// Create a new invalid operation error with a simple message
    pub fn invalid_operation_simple<S: Into<String>>(message: S) -> Self {
        let msg = message.into();
        Self::InvalidOperation {
            operation: msg,
            reason: String::new(),
        }
    }

    /// Create a new invalid operand error
    pub fn invalid_operand<S: Into<String>>(expected: S, actual: S) -> Self {
        Self::InvalidOperand {
            expected: expected.into(),
            actual: actual.into(),
        }
    }

    /// Create a new invalid script error
    pub fn invalid_script<S: Into<String>>(reason: S) -> Self {
        Self::InvalidScript {
            reason: reason.into(),
        }
    }

    /// Create a new stack underflow error
    #[must_use] 
    pub const fn stack_underflow(requested: usize, available: usize) -> Self {
        Self::StackUnderflow {
            requested,
            available,
        }
    }

    /// Create a new stack overflow error
    #[must_use] 
    pub const fn stack_overflow(max_size: usize) -> Self {
        Self::StackOverflow { max_size }
    }

    /// Create a new overflow error
    pub fn overflow<S: Into<String>>(operation: S) -> Self {
        Self::Overflow {
            operation: operation.into(),
        }
    }

    /// Create a new underflow error
    pub fn underflow<S: Into<String>>(operation: S) -> Self {
        Self::Underflow {
            operation: operation.into(),
        }
    }

    /// Create a new division by zero error
    pub fn division_by_zero<S: Into<String>>(operation: S) -> Self {
        Self::DivisionByZero {
            operation: operation.into(),
        }
    }

    /// Create a new insufficient stack items error
    #[must_use] 
    pub const fn insufficient_stack_items(required: usize, available: usize) -> Self {
        Self::InsufficientStackItems {
            required,
            available,
        }
    }

    /// Create a new invalid type error
    pub fn invalid_type<S: Into<String>>(from: S, to: S) -> Self {
        Self::InvalidType {
            from: from.into(),
            to: to.into(),
        }
    }

    /// Create a new invalid type error with a simple message
    pub fn invalid_type_simple<S: Into<String>>(message: S) -> Self {
        let msg = message.into();
        Self::InvalidType {
            from: msg,
            to: String::new(),
        }
    }

    /// Create a new execution halted error
    pub fn execution_halted<S: Into<String>>(reason: S) -> Self {
        Self::ExecutionHalted {
            reason: reason.into(),
        }
    }

    /// Create a new VM fault error
    pub fn vm_fault<S: Into<String>>(fault_type: S, details: S) -> Self {
        Self::VmFault {
            fault_type: fault_type.into(),
            details: details.into(),
        }
    }

    /// Create a new I/O error
    pub fn io<S: Into<String>>(message: S) -> Self {
        Self::Io {
            message: message.into(),
        }
    }

    /// Create a new memory limit exceeded error
    #[must_use] 
    pub const fn memory_limit_exceeded(used: usize, limit: usize) -> Self {
        Self::MemoryLimitExceeded { used, limit }
    }

    /// Create a new instruction limit exceeded error
    #[must_use] 
    pub const fn instruction_limit_exceeded(executed: u64, limit: u64) -> Self {
        Self::InstructionLimitExceeded { executed, limit }
    }

    /// Create a new call depth limit exceeded error
    #[must_use] 
    pub const fn call_depth_limit_exceeded(depth: usize, limit: usize) -> Self {
        Self::CallDepthLimitExceeded { depth, limit }
    }

    /// Create a new gas exhausted error
    #[must_use] 
    pub const fn gas_exhausted(used: u64, limit: u64) -> Self {
        Self::GasExhausted { used, limit }
    }

    /// Implementation provided I/O error (for testing)
    #[cfg(test)]
    #[allow(dead_code)]
    pub fn real_io<S: Into<String>>(message: S) -> Self {
        Self::MockIo {
            message: message.into(),
        }
    }

    /// Check if this error is retryable
    #[must_use] 
    pub const fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Io { .. } | Self::ExecutionTimeout { .. } | Self::InteropService { .. }
        )
    }

    /// Check if this error is a resource limit error
    #[must_use] 
    pub const fn is_resource_limit(&self) -> bool {
        matches!(
            self,
            Self::MemoryLimitExceeded { .. }
                | Self::InstructionLimitExceeded { .. }
                | Self::CallDepthLimitExceeded { .. }
                | Self::GasExhausted { .. }
                | Self::StackOverflow { .. }
        )
    }

    /// Check if this error is a user error (vs system error)
    #[must_use] 
    pub const fn is_user_error(&self) -> bool {
        matches!(
            self,
            Self::Parse { .. }
                | Self::InvalidInstruction { .. }
                | Self::InvalidOpCode { .. }
                | Self::CatchableException { .. }
                | Self::InvalidOperation { .. }
                | Self::InvalidOperand { .. }
                | Self::InvalidScript { .. }
                | Self::InvalidType { .. }
                | Self::InvalidParameters { .. }
                | Self::InvalidWitness { .. }
                | Self::VerificationFailed { .. }
        )
    }

    /// Check if this error should cause a VM fault
    #[must_use] 
    pub const fn should_fault(&self) -> bool {
        matches!(
            self,
            Self::StackUnderflow { .. }
                | Self::StackOverflow { .. }
                | Self::Overflow { .. }
                | Self::Underflow { .. }
                | Self::DivisionByZero { .. }
                | Self::InvalidType { .. }
                | Self::VmFault { .. }
                | Self::MemoryLimitExceeded { .. }
                | Self::InstructionLimitExceeded { .. }
                | Self::CallDepthLimitExceeded { .. }
                | Self::GasExhausted { .. }
                | Self::InvalidJump(_)
                | Self::UnhandledException(_)
                | Self::MaxTryNestingDepthExceeded
                | Self::Abort
                | Self::AssertFailed
        )
    }

    /// Get error category for logging/metrics
    #[must_use] 
    pub const fn category(&self) -> &'static str {
        match self {
            Self::Parse { .. } => "parse",
            Self::InvalidInstruction { .. } | Self::InvalidOpCode { .. } => "instruction",
            Self::UnsupportedOperation { .. }
            | Self::CatchableException { .. }
            | Self::InvalidOperation { .. } => "operation",
            Self::InvalidOperand { .. } | Self::InvalidParameters { .. } => "operand",
            Self::InvalidScript { .. } => "script",
            Self::StackUnderflow { .. }
            | Self::StackOverflow { .. }
            | Self::InsufficientStackItems { .. } => "stack",
            Self::Overflow { .. }
            | Self::Underflow { .. }
            | Self::DivisionByZero { .. } => "arithmetic",
            Self::InvalidType { .. } => "type",
            Self::ExecutionHalted { .. } | Self::VmFault { .. } => "execution",
            Self::Io { .. } => "io",
            Self::MemoryLimitExceeded { .. }
            | Self::InstructionLimitExceeded { .. }
            | Self::CallDepthLimitExceeded { .. }
            | Self::GasExhausted { .. } => "resource",
            Self::InvalidContractState { .. }
            | Self::ContractNotFound { .. }
            | Self::MethodNotFound { .. } => "contract",
            Self::InteropService { .. } => "interop",
            Self::ExecutionTimeout { .. } => "timeout",
            Self::InvalidScriptHash { .. } => "hash",
            Self::InvalidWitness { .. } | Self::VerificationFailed { .. } => "verification",
            Self::InvalidJump(_) => "control",
            Self::UnhandledException(_) => "exception",
            Self::MaxTryNestingDepthExceeded => "resource",
            Self::Abort | Self::AssertFailed => "control",
            #[cfg(test)]
            #[allow(dead_code)]
            VmError::MockIo { .. } => "real_io",
        }
    }
}

/// Result type for VM operations
pub type VmResult<T> = std::result::Result<T, VmError>;

/// Alias for compatibility with existing code
pub type Result<T, E = VmError> = std::result::Result<T, E>;

// Standard library error conversions
impl From<std::io::Error> for VmError {
    fn from(_error: std::io::Error) -> Self {
        Self::io(_error.to_string())
    }
}

impl From<std::fmt::Error> for VmError {
    fn from(error: std::fmt::Error) -> Self {
        Self::parse(error.to_string())
    }
}

impl From<std::num::ParseIntError> for VmError {
    fn from(_error: std::num::ParseIntError) -> Self {
        Self::invalid_type("string", "integer")
    }
}

impl From<std::num::ParseFloatError> for VmError {
    fn from(_error: std::num::ParseFloatError) -> Self {
        Self::invalid_type("string", "float")
    }
}

// Neo-specific error conversions
impl From<neo_io::IoError> for VmError {
    fn from(error: neo_io::IoError) -> Self {
        match error {
            neo_io::IoError::Format => Self::parse("format error"),
            neo_io::IoError::InvalidUtf8 => Self::parse("invalid utf-8 data"),
            neo_io::IoError::InvalidData { context, value } => {
                Self::parse(format!("{context}: {value}"))
            }
            neo_io::IoError::Io(inner) => Self::io(inner.to_string()),
        }
    }
}

// Test-specific error conversions for comprehensive testing

impl VmError {
    /// Create `InvalidInstruction` from a single message
    pub fn invalid_instruction_msg<S: Into<String>>(message: S) -> Self {
        Self::InvalidInstruction {
            opcode: 0,
            reason: message.into(),
        }
    }

    /// Create `InvalidOperand` from a single message
    pub fn invalid_operand_msg<S: Into<String>>(message: S) -> Self {
        let msg = message.into();
        Self::InvalidOperand {
            expected: msg,
            actual: String::new(),
        }
    }

    /// Create `ExecutionHalted` from a single message
    pub fn execution_halted_msg<S: Into<String>>(message: S) -> Self {
        Self::ExecutionHalted {
            reason: message.into(),
        }
    }

    /// Create `UnsupportedOperation` from a single message
    pub fn unsupported_operation_msg<S: Into<String>>(message: S) -> Self {
        Self::UnsupportedOperation {
            operation: message.into(),
        }
    }

    /// Create `InvalidScript` from a single message
    pub fn invalid_script_msg<S: Into<String>>(message: S) -> Self {
        Self::InvalidScript {
            reason: message.into(),
        }
    }

    /// Create `StackUnderflow` from parameters
    #[must_use] 
    pub const fn stack_underflow_msg(requested: usize, available: usize) -> Self {
        Self::StackUnderflow {
            requested,
            available,
        }
    }

    /// Create `InsufficientStackItems` from parameters
    #[must_use] 
    pub const fn insufficient_stack_items_msg(required: usize, available: usize) -> Self {
        Self::InsufficientStackItems {
            required,
            available,
        }
    }

    /// Create `DivisionByZero` from operation
    pub fn division_by_zero_msg<S: Into<String>>(operation: S) -> Self {
        Self::DivisionByZero {
            operation: operation.into(),
        }
    }
}

#[cfg(test)]
#[allow(dead_code)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let error = VmError::parse("test message");
        assert!(matches!(error, VmError::Parse { .. }));
        assert_eq!(error.to_string(), "Parse error: test message");
    }

    #[test]
    fn test_error_categories() {
        assert_eq!(VmError::parse("test").category(), "parse");
        assert_eq!(VmError::invalid_opcode(0x42).category(), "instruction");
        assert_eq!(VmError::stack_underflow(1, 0).category(), "stack");
    }

    #[test]
    fn test_error_classification() {
        assert!(VmError::io("test").is_retryable());
        assert!(!VmError::parse("test").is_retryable());

        assert!(VmError::gas_exhausted(1000, 500).is_resource_limit());
        assert!(!VmError::parse("test").is_resource_limit());

        assert!(VmError::parse("test").is_user_error());
        assert!(!VmError::io("test").is_user_error());

        assert!(VmError::stack_underflow(1, 0).should_fault());
        assert!(!VmError::parse("test").should_fault());
    }

    #[test]
    fn test_stack_errors() {
        let error = VmError::stack_underflow(5, 2);
        assert_eq!(
            error.to_string(),
            "Stack underflow: attempted to access 5 items, but only 2 available"
        );

        let error = VmError::insufficient_stack_items(3, 1);
        assert_eq!(
            error.to_string(),
            "Insufficient stack items: required 3, available 1"
        );
    }

    #[test]
    fn test_resource_limit_errors() {
        let limit =
            crate::execution_engine_limits::ExecutionEngineLimits::DEFAULT.max_item_size as usize;
        let error = VmError::memory_limit_exceeded(2048, limit);
        // C#: ushort.MaxValue = 65535
        assert_eq!(
            error.to_string(),
            format!(
                "Memory limit exceeded: used 2048 bytes, limit {} bytes",
                limit
            )
        );

        let error = VmError::gas_exhausted(1000, 800);
        assert_eq!(error.to_string(), "Gas exhausted: used 1000, limit 800");
    }
}
