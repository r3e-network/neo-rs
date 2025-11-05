use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum VmError {
    #[error("stack underflow")]
    StackUnderflow,

    #[error("invalid type")]
    InvalidType,

    #[error("division by zero")]
    DivisionByZero,

    #[error("native call failed: {0}")]
    NativeFailure(&'static str),

    #[error("unsupported syscall")]
    UnsupportedSyscall,

    #[error("script fault")]
    Fault,
}
