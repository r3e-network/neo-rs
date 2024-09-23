use serde::{Deserialize, Serialize};
use std::fmt;

// Error represents JSON-RPC 2.0 error type.
#[derive(Debug, Serialize, Deserialize)]
pub struct Error {
    code: i64,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<String>,
}

// Standard RPC error codes defined by the JSON-RPC 2.0 specification.
pub const INTERNAL_SERVER_ERROR_CODE: i64 = -32603;
pub const BAD_REQUEST_CODE: i64 = -32700;
pub const INVALID_REQUEST_CODE: i64 = -32600;
pub const METHOD_NOT_FOUND_CODE: i64 = -32601;
pub const INVALID_PARAMS_CODE: i64 = -32602;

// RPC error codes defined by the Neo JSON-RPC specification extension.
pub const ERR_UNKNOWN_BLOCK_CODE: i64 = -101;
pub const ERR_UNKNOWN_CONTRACT_CODE: i64 = -102;
pub const ERR_UNKNOWN_TRANSACTION_CODE: i64 = -103;
pub const ERR_UNKNOWN_STORAGE_ITEM_CODE: i64 = -104;
pub const ERR_UNKNOWN_SCRIPT_CONTAINER_CODE: i64 = -105;
pub const ERR_UNKNOWN_STATE_ROOT_CODE: i64 = -106;
pub const ERR_UNKNOWN_SESSION_CODE: i64 = -107;
pub const ERR_UNKNOWN_ITERATOR_CODE: i64 = -108;
pub const ERR_UNKNOWN_HEIGHT_CODE: i64 = -109;

pub const ERR_INSUFFICIENT_FUNDS_WALLET_CODE: i64 = -300;
pub const ERR_WALLET_FEE_LIMIT_CODE: i64 = -301;
pub const ERR_NO_OPENED_WALLET_CODE: i64 = -302;
pub const ERR_WALLET_NOT_FOUND_CODE: i64 = -303;
pub const ERR_WALLET_NOT_SUPPORTED_CODE: i64 = -304;

pub const ERR_VERIFICATION_FAILED_CODE: i64 = -500;
pub const ERR_ALREADY_EXISTS_CODE: i64 = -501;
pub const ERR_MEMPOOL_CAP_REACHED_CODE: i64 = -502;
pub const ERR_ALREADY_IN_POOL_CODE: i64 = -503;
pub const ERR_INSUFFICIENT_NETWORK_FEE_CODE: i64 = -504;
pub const ERR_POLICY_FAILED_CODE: i64 = -505;
pub const ERR_INVALID_SCRIPT_CODE: i64 = -506;
pub const ERR_INVALID_ATTRIBUTE_CODE: i64 = -507;
pub const ERR_INVALID_SIGNATURE_CODE: i64 = -508;
pub const ERR_INVALID_SIZE_CODE: i64 = -509;
pub const ERR_EXPIRED_TRANSACTION_CODE: i64 = -510;
pub const ERR_INSUFFICIENT_FUNDS_CODE: i64 = -511;
pub const ERR_INVALID_VERIFICATION_FUNCTION_CODE: i64 = -512;

pub const ERR_SESSIONS_DISABLED_CODE: i64 = -601;
pub const ERR_ORACLE_DISABLED_CODE: i64 = -602;
pub const ERR_ORACLE_REQUEST_FINISHED_CODE: i64 = -603;
pub const ERR_ORACLE_REQUEST_NOT_FOUND_CODE: i64 = -604;
pub const ERR_ORACLE_NOT_DESIGNATED_NODE_CODE: i64 = -605;
pub const ERR_UNSUPPORTED_STATE_CODE: i64 = -606;
pub const ERR_INVALID_PROOF_CODE: i64 = -607;
pub const ERR_EXECUTION_FAILED_CODE: i64 = -608;

lazy_static! {
    pub static ref ERR_COMPAT_GENERIC: Error = Error::new_with_code(-100, "RPC error");
    pub static ref ERR_COMPAT_NO_OPENED_WALLET: Error = Error::new_with_code(-400, "No opened wallet");
    pub static ref ERR_INVALID_PARAMS: Error = Error::new_invalid_params_error("Invalid params");
    pub static ref ERR_UNKNOWN_BLOCK: Error = Error::new_with_code(ERR_UNKNOWN_BLOCK_CODE, "Unknown block");
    pub static ref ERR_UNKNOWN_CONTRACT: Error = Error::new_with_code(ERR_UNKNOWN_CONTRACT_CODE, "Unknown contract");
    pub static ref ERR_UNKNOWN_TRANSACTION: Error = Error::new_with_code(ERR_UNKNOWN_TRANSACTION_CODE, "Unknown transaction");
    pub static ref ERR_UNKNOWN_STORAGE_ITEM: Error = Error::new_with_code(ERR_UNKNOWN_STORAGE_ITEM_CODE, "Unknown storage item");
    pub static ref ERR_UNKNOWN_SCRIPT_CONTAINER: Error = Error::new_with_code(ERR_UNKNOWN_SCRIPT_CONTAINER_CODE, "Unknown script container");
    pub static ref ERR_UNKNOWN_STATE_ROOT: Error = Error::new_with_code(ERR_UNKNOWN_STATE_ROOT_CODE, "Unknown state root");
    pub static ref ERR_UNKNOWN_SESSION: Error = Error::new_with_code(ERR_UNKNOWN_SESSION_CODE, "Unknown session");
    pub static ref ERR_UNKNOWN_ITERATOR: Error = Error::new_with_code(ERR_UNKNOWN_ITERATOR_CODE, "Unknown iterator");
    pub static ref ERR_UNKNOWN_HEIGHT: Error = Error::new_with_code(ERR_UNKNOWN_HEIGHT_CODE, "Unknown height");
    pub static ref ERR_INSUFFICIENT_FUNDS_WALLET: Error = Error::new_with_code(ERR_INSUFFICIENT_FUNDS_WALLET_CODE, "Insufficient funds");
    pub static ref ERR_WALLET_FEE_LIMIT: Error = Error::new_with_code(ERR_WALLET_FEE_LIMIT_CODE, "Fee limit exceeded");
    pub static ref ERR_NO_OPENED_WALLET: Error = Error::new_with_code(ERR_NO_OPENED_WALLET_CODE, "No opened wallet");
    pub static ref ERR_WALLET_NOT_FOUND: Error = Error::new_with_code(ERR_WALLET_NOT_FOUND_CODE, "Wallet not found");
    pub static ref ERR_WALLET_NOT_SUPPORTED: Error = Error::new_with_code(ERR_WALLET_NOT_SUPPORTED_CODE, "Wallet not supported");
    pub static ref ERR_VERIFICATION_FAILED: Error = Error::new_with_code(ERR_VERIFICATION_FAILED_CODE, "Unclassified inventory verification error");
    pub static ref ERR_ALREADY_EXISTS: Error = Error::new_with_code(ERR_ALREADY_EXISTS_CODE, "Inventory already exists on chain");
    pub static ref ERR_MEMPOOL_CAP_REACHED: Error = Error::new_with_code(ERR_MEMPOOL_CAP_REACHED_CODE, "The memory pool is full and no more transactions can be sent");
    pub static ref ERR_ALREADY_IN_POOL: Error = Error::new_with_code(ERR_ALREADY_IN_POOL_CODE, "Transaction already exists in the memory pool");
    pub static ref ERR_INSUFFICIENT_NETWORK_FEE: Error = Error::new_with_code(ERR_INSUFFICIENT_NETWORK_FEE_CODE, "Insufficient network fee");
    pub static ref ERR_POLICY_FAILED: Error = Error::new_with_code(ERR_POLICY_FAILED_CODE, "One of the Policy filters failed");
    pub static ref ERR_INVALID_SCRIPT: Error = Error::new_with_code(ERR_INVALID_SCRIPT_CODE, "Invalid script");
    pub static ref ERR_INVALID_ATTRIBUTE: Error = Error::new_with_code(ERR_INVALID_ATTRIBUTE_CODE, "Invalid transaction attribute");
    pub static ref ERR_INVALID_SIGNATURE: Error = Error::new_with_code(ERR_INVALID_SIGNATURE_CODE, "Invalid signature");
    pub static ref ERR_INVALID_SIZE: Error = Error::new_with_code(ERR_INVALID_SIZE_CODE, "Invalid inventory size");
    pub static ref ERR_EXPIRED_TRANSACTION: Error = Error::new_with_code(ERR_EXPIRED_TRANSACTION_CODE, "Expired transaction");
    pub static ref ERR_INSUFFICIENT_FUNDS: Error = Error::new_with_code(ERR_INSUFFICIENT_FUNDS_CODE, "Insufficient funds");
    pub static ref ERR_INVALID_VERIFICATION_FUNCTION: Error = Error::new_with_code(ERR_INVALID_VERIFICATION_FUNCTION_CODE, "Invalid verification function");
    pub static ref ERR_SESSIONS_DISABLED: Error = Error::new_with_code(ERR_SESSIONS_DISABLED_CODE, "Sessions disabled");
    pub static ref ERR_ORACLE_DISABLED: Error = Error::new_with_code(ERR_ORACLE_DISABLED_CODE, "Oracle service is not running");
    pub static ref ERR_ORACLE_REQUEST_FINISHED: Error = Error::new_with_code(ERR_ORACLE_REQUEST_FINISHED_CODE, "Oracle request has already been finished");
    pub static ref ERR_ORACLE_REQUEST_NOT_FOUND: Error = Error::new_with_code(ERR_ORACLE_REQUEST_NOT_FOUND_CODE, "Oracle request is not found");
    pub static ref ERR_ORACLE_NOT_DESIGNATED_NODE: Error = Error::new_with_code(ERR_ORACLE_NOT_DESIGNATED_NODE_CODE, "Not a designated oracle node");
    pub static ref ERR_UNSUPPORTED_STATE: Error = Error::new_with_code(ERR_UNSUPPORTED_STATE_CODE, "Old state requests are not supported");
    pub static ref ERR_INVALID_PROOF: Error = Error::new_with_code(ERR_INVALID_PROOF_CODE, "Invalid proof");
    pub static ref ERR_EXECUTION_FAILED: Error = Error::new_with_code(ERR_EXECUTION_FAILED_CODE, "Execution failed");
}

impl Error {
    // NewError is an Error constructor that takes Error contents from its parameters.
    pub fn new(code: i64, message: &str, data: Option<String>) -> Self {
        Error {
            code,
            message: message.to_string(),
            data,
        }
    }

    // NewParseError creates a new error with code -32700.
    pub fn new_parse_error(data: Option<String>) -> Self {
        Error::new(BAD_REQUEST_CODE, "Parse error", data)
    }

    // NewInvalidRequestError creates a new error with code -32600.
    pub fn new_invalid_request_error(data: Option<String>) -> Self {
        Error::new(INVALID_REQUEST_CODE, "Invalid request", data)
    }

    // NewMethodNotFoundError creates a new error with code -32601.
    pub fn new_method_not_found_error(data: Option<String>) -> Self {
        Error::new(METHOD_NOT_FOUND_CODE, "Method not found", data)
    }

    // NewInvalidParamsError creates a new error with code -32602.
    pub fn new_invalid_params_error(data: Option<String>) -> Self {
        Error::new(INVALID_PARAMS_CODE, "Invalid params", data)
    }

    // NewInternalServerError creates a new error with code -32603.
    pub fn new_internal_server_error(data: Option<String>) -> Self {
        Error::new(INTERNAL_SERVER_ERROR_CODE, "Internal error", data)
    }

    // NewErrorWithCode creates a new error with specified error code and error message.
    pub fn new_with_code(code: i64, message: &str) -> Self {
        Error::new(code, message, None)
    }

    // WrapErrorWithData returns copy of the given error with the specified data and cause.
    // It does not modify the source error.
    pub fn wrap_with_data(&self, data: Option<String>) -> Self {
        Error::new(self.code, &self.message, data)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(ref data) = self.data {
            write!(f, "{} ({}) - {}", self.message, self.code, data)
        } else {
            write!(f, "{} ({})", self.message, self.code)
        }
    }
}

impl std::error::Error for Error {
    fn is(&self, target: &dyn std::error::Error) -> bool {
        if let Some(target) = target.downcast_ref::<Error>() {
            self.code == target.code
        } else {
            false
        }
    }
}
