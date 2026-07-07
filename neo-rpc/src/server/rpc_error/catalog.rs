//! Named Neo RPC error constructors.
//!
//! The root module owns the `RpcError` record and formatting behavior; this
//! module owns the C#-compatible error catalog so codes/messages stay grouped
//! and auditable.

use neo_primitives::UInt160;

use super::RpcError;

macro_rules! rpc_error_constructors {
    (
        $(
            $(#[$meta:meta])*
            $name:ident => ($code:expr_2021, $message:expr_2021 $(, data = $data:expr_2021)?);
        )+
    ) => {
        $(
            $(#[$meta])*
            #[must_use]
            pub fn $name() -> Self {
                Self::new($code, $message, rpc_error_constructors!(@data $($data)?))
           }
        )+
   };

    (@data) => {
        None
   };

    (@data $data:expr_2021) => {
        Some($data.to_string())
   };
}

impl RpcError {
    /// Error for a contract that has no compatible `verify` method.
    pub fn invalid_contract_verification_hash(contract_hash: &UInt160, pcount: i32) -> RpcError {
        RpcError::invalid_contract_verification().with_data(format!(
            "The smart contract {contract_hash} haven't got verify method with {pcount} input parameters."
        ))
    }

    rpc_error_constructors! {
         /// Invalid JSON-RPC request (spec defined).
         invalid_request => (-32600, "Invalid request");
         /// Unknown RPC method.
         method_not_found => (-32601, "Method not found");
         /// Invalid method parameters.
         invalid_params => (-32602, "Invalid params");
         /// Internal JSON-RPC error.
         internal_server_error => (-32603, "Internal server RpcError");
         /// Server-side rate limiting triggered.
         ///
         /// Uses the JSON-RPC server error range (-32000..-32099).
         too_many_requests => (-32001, "Too many requests");
         /// Malformed JSON payload.
         bad_request => (-32700, "Bad request");
         /// Unknown block referenced in the request.
         unknown_block => (-101, "Unknown block");
         /// Unknown contract referenced in the request.
         unknown_contract => (-102, "Unknown contract");
         /// Unknown transaction referenced in the request.
         unknown_transaction => (-103, "Unknown transaction");
         /// Unknown storage item referenced in the request.
         unknown_storage_item => (-104, "Unknown storage item");
         /// Unknown script container referenced in the request.
         unknown_script_container => (-105, "Unknown script container");
         /// Unknown state root referenced in the request.
         unknown_state_root => (-106, "Unknown state root");
         /// Unknown iterator identifier.
         unknown_iterator => (-108, "Unknown iterator");
         /// Unknown iterator session identifier.
         unknown_session => (-107, "Unknown session");
         /// Unknown block height.
         unknown_height => (-109, "Unknown height");
         /// Insufficient funds inside a wallet context.
         insufficient_funds_wallet => (-300, "Insufficient funds in wallet");
         /// Wallet fee limit exceeded.
         wallet_fee_limit => (
             -301,
             "Wallet fee limit exceeded",
             data = "The necessary fee is more than the MaxFee, this transaction is failed. Please increase your MaxFee value."
         );
         /// No wallet opened.
         no_opened_wallet => (-302, "No opened wallet");
         /// Wallet not found.
         wallet_not_found => (-303, "Wallet not found");
         /// Wallet type not supported.
         wallet_not_supported => (-304, "Wallet not supported");
         /// Unknown account referenced in request.
         unknown_account => (-305, "Unknown account");
         /// Inventory verification failed.
         verification_failed => (-500, "Inventory verification failed");
         /// Inventory already exists.
         already_exists => (-501, "Inventory already exists");
         /// Mempool capacity reached.
         mempool_cap_reached => (-502, "Memory pool capacity reached");
         /// Inventory already present in pool.
         already_in_pool => (-503, "Already in pool");
         /// Insufficient network fee supplied.
         insufficient_network_fee => (-504, "Insufficient network fee");
         /// Policy check failed.
         policy_failed => (-505, "Policy check failed");
         /// Transaction script invalid.
         invalid_script => (-506, "Invalid transaction script");
         /// Invalid transaction attribute.
         invalid_attribute => (-507, "Invalid transaction attribute");
         /// Invalid signature detected.
         invalid_signature => (-508, "Invalid signature");
         /// Inventory payload size invalid.
         invalid_size => (-509, "Invalid inventory size");
         /// Transaction expired.
         expired_transaction => (-510, "Expired transaction");
         /// Insufficient funds to cover fees.
         insufficient_funds => (-511, "Insufficient funds for fee");
         /// Contract verification routine invalid.
         invalid_contract_verification => (-512, "Invalid contract verification function");
         /// Access denied for the requested operation.
         access_denied => (-600, "Access denied");
         /// Iterator session feature disabled.
         sessions_disabled => (-601, "State iterator sessions disabled");
         /// Oracle service disabled.
         oracle_disabled => (-602, "Oracle service disabled");
         /// Oracle request already finished.
         oracle_request_finished => (-603, "Oracle request already finished");
         /// Oracle request not found.
         oracle_request_not_found => (-604, "Oracle request not found");
         /// Node is not designated oracle node.
         oracle_not_designated_node => (-605, "Not a designated oracle node");
         /// Requested state is not supported (old state).
         unsupported_state => (-606, "Old state not supported");
         /// Invalid state proof supplied.
         invalid_proof => (-607, "Invalid state proof");
         /// Contract execution failed.
         execution_failed => (-608, "Contract execution failed");
    }
}
