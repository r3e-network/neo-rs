use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum ApplicationEngineError {
    #[error("System error")]
    SystemError,
    #[error("Invalid instruction")]
    InvalidInstruction,
    #[error("Invalid contract call")]
    InvalidContractCall,
    #[error("Stack overflow")]
    StackOverflow,
    #[error("Stack underflow")]
    StackUnderflow,
    #[error("Out of gas")]
    OutOfGas,
    #[error("Execution halted")]
    ExecutionHalted,
    #[error("Unauthorized operation")]
    UnauthorizedOperation,
    #[error("Invalid attempt to load script")]
    InvalidAttemptToLoadScript,
    #[error("Storage error")]
    StorageError,
    #[error("Storage key not found")]
    StorageKeyNotFound,
    #[error("Storage key too large")]
    StorageKeyTooLarge,
    #[error("Storage value too large")]
    StorageValueTooLarge,
    #[error("Contract not found")]
    ContractNotFound,
    #[error("Method not found")]
    MethodNotFound,
    #[error("Invalid method signature")]
    InvalidMethodSignature,
    #[error("Invalid argument")]
    InvalidArgument,
    #[error("Incorrect number of arguments")]
    NumberOfArgumentsIncorrect,
    #[error("Invalid state")]
    InvalidState,
    #[error("Unknown error")]
    UnknownError,
}

