//! Type-safe models for RPC request/response payloads.
//!
//! This module provides strongly-typed Rust structures that mirror the JSON
//! payloads used in Neo N3 JSON-RPC communication. All models implement
//! serialization via `serde` and custom JSON conversion methods matching
//! the C# reference implementation.
//!
//! # Model Categories
//!
//! - **Core RPC types**: Request/response wrappers ([`RpcRequest`], [`RpcResponse`])
//! - **Block types**: Block and header representations ([`RpcBlock`], [`RpcBlockHeader`])
//! - **Contract types**: Contract state and execution ([`RpcContractState`], [`RpcInvokeResult`])
//! - **Token types**: NEP-17 and NEP-11 balance/transfer records
//! - **State types**: State service and proof types ([`RpcStateRoot`], [`RpcFoundStates`])
//! - **Network types**: Peer and version information ([`RpcPeers`], [`RpcVersion`])

mod vm_state_utils;

#[cfg(test)]
#[path = "../../tests/client/models/test_fixtures.rs"]
pub(crate) mod test_fixtures;

// Core RPC types
/// JSON-RPC request payload model.
pub mod rpc_request;
/// JSON-RPC response payload model.
pub mod rpc_response;

// Generic response wrapper (new)
/// Generic JSON-RPC response wrapper model.
pub mod rpc_response_wrapper;

// Block and transaction types
/// Block response model.
pub mod rpc_block;
/// Block header response model.
pub mod rpc_block_header;
/// Transaction response model.
pub mod rpc_transaction;

// Contract and execution types
/// Contract state response model.
pub mod rpc_contract_state;
/// Contract invocation result model.
pub mod rpc_invoke_result;
/// Contract method token model.
pub mod rpc_method_token;
/// NEF file response model.
pub mod rpc_nef_file;
/// VM stack item response model.
pub mod rpc_stack;

// NEP17 token types
/// NEP-17 balance response models.
pub mod rpc_nep17_balances;
/// NEP-17 token information model.
pub mod rpc_nep17_token_info;
/// NEP-17 transfer response models.
pub mod rpc_nep17_transfers;

// NEP11 token types
/// NEP-11 balance response models.
pub mod rpc_nep11_balances;
/// NEP-11 transfer response models.
pub mod rpc_nep11_transfers;

// State service types
/// State service search response model.
pub mod rpc_found_states;
/// State root response model.
pub mod rpc_state_root;

// Network and peer types
/// Peer list response models.
pub mod rpc_peers;
/// Plugin information model.
pub mod rpc_plugin;
/// Node version response model.
pub mod rpc_version;

// Application and execution types
/// Application log response models.
pub mod rpc_application_log;
/// Raw mempool response model.
pub mod rpc_raw_mem_pool;

// Account and wallet types
/// Wallet account response model.
pub mod rpc_account;
/// Wallet transfer output model.
pub mod rpc_transfer_out;
/// Unclaimed GAS response model.
pub mod rpc_unclaimed_gas;
/// Address validation response model.
pub mod rpc_validate_address_result;
/// Validator response model.
pub mod rpc_validator;

// Optional/legacy modules (kept for compatibility)
/// Legacy get-peers response model.
pub mod rpc_get_peers;
/// Legacy accepted-mempool response model.
pub mod rpc_mempool_accepted;
/// Legacy unverified-mempool response model.
pub mod rpc_mempool_unverified;
/// Legacy method invocation model.
pub mod rpc_method_invocation;
/// Legacy notification event model.
pub mod rpc_notification_event;
/// Oracle response model.
pub mod rpc_oracle_response;
/// Legacy plugin information model.
pub mod rpc_plugin_info;

// Re-export main types
pub use rpc_account::RpcAccount;
pub use rpc_application_log::{Execution, RpcApplicationLog, RpcNotifyEventArgs};
pub use rpc_block::RpcBlock;
pub use rpc_block_header::RpcBlockHeader;
pub use rpc_contract_state::RpcContractState;
pub use rpc_found_states::RpcFoundStates;
pub use rpc_invoke_result::RpcInvokeResult;
pub use rpc_method_token::RpcMethodToken;
pub use rpc_nef_file::RpcNefFile;
pub use rpc_nep11_balances::{RpcNep11Balance, RpcNep11Balances, RpcNep11TokenBalance};
pub use rpc_nep11_transfers::{RpcNep11Transfer, RpcNep11Transfers};
pub use rpc_nep17_balances::{RpcNep17Balance, RpcNep17Balances};
pub use rpc_nep17_token_info::RpcNep17TokenInfo;
pub use rpc_nep17_transfers::{RpcNep17Transfer, RpcNep17Transfers};
pub use rpc_peers::{RpcPeer, RpcPeers};
pub use rpc_plugin::RpcPlugin;
pub use rpc_raw_mem_pool::RpcRawMemPool;
pub use rpc_request::RpcRequest;
pub use rpc_response::{RpcResponse, RpcResponseError};
pub use rpc_response_wrapper::RpcResponse as RpcResponseWrapper;
pub use rpc_stack::RpcStack;
pub use rpc_state_root::RpcStateRoot;
pub use rpc_transaction::RpcTransaction;
pub use rpc_transfer_out::RpcTransferOut;
pub use rpc_unclaimed_gas::RpcUnclaimedGas;
pub use rpc_validate_address_result::RpcValidateAddressResult;
pub use rpc_validator::RpcValidator;
pub use rpc_version::{RpcProtocol, RpcVersion};
