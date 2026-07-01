//! # neo-rpc::client::models
//!
//! Client-side RPC request and response models for neo-rpc.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `vm_state_utils`: VM state conversion helpers.
//! - `test_fixtures`: RPC model test fixtures.
//! - `rpc_request`: JSON-RPC request model.
//! - `rpc_response`: JSON-RPC response model.
//! - `rpc_response_wrapper`: typed JSON-RPC response wrapper.
//! - `rpc_block`: block RPC response model.
//! - `rpc_block_header`: block-header RPC response model.
//! - `rpc_transaction`: transaction RPC response model.
//! - `rpc_contract_state`: contract-state RPC response model.
//! - `rpc_invoke_result`: invoke-result RPC response model.
//! - `rpc_method_token`: method-token RPC model.
//! - `rpc_nef_file`: NEF-file RPC model.
//! - `rpc_stack`: VM stack RPC model.
//! - `rpc_nep17_balances`: NEP-17 balances RPC model.
//! - `rpc_nep17_token_info`: NEP-17 token-info RPC model.
//! - `rpc_nep17_transfers`: NEP-17 transfer RPC model.
//! - `rpc_nep11_balances`: NEP-11 balances RPC model.
//! - `rpc_nep11_transfers`: NEP-11 transfer RPC model.
//! - `rpc_found_states`: state search RPC model.
//! - `rpc_state_root`: state-root RPC model.
//! - `rpc_peers`: peer-list RPC model.
//! - `rpc_plugin`: plugin RPC model.
//! - `rpc_version`: node version RPC model.
//! - `rpc_application_log`: application-log RPC model.
//! - `rpc_raw_mem_pool`: raw mempool RPC model.
//! - `rpc_account`: wallet account RPC model.
//! - `rpc_transfer_out`: wallet transfer output RPC model.
//! - `rpc_unclaimed_gas`: unclaimed GAS RPC model.
//! - `rpc_validate_address_result`: address validation RPC model.
//! - `rpc_validator`: validator RPC model.
//! - `rpc_get_peers`: get-peers RPC model.
//! - `rpc_mempool_accepted`: accepted mempool RPC model.
//! - `rpc_mempool_unverified`: unverified mempool RPC model.
//! - `rpc_method_invocation`: method invocation RPC model.
//! - `rpc_notification_event`: notification event RPC model.
//! - `rpc_oracle_response`: oracle response RPC model.
//! - `rpc_plugin_info`: plugin information RPC model.

#[path = "support/vm_state_utils.rs"]
mod vm_state_utils;

#[cfg(test)]
#[path = "../../tests/client/models/support/test_fixtures.rs"]
pub(crate) mod test_fixtures;

// Core RPC types
/// JSON-RPC request payload model.
#[path = "core/rpc_request.rs"]
pub mod rpc_request;
/// JSON-RPC response payload model.
#[path = "core/rpc_response.rs"]
pub mod rpc_response;

// Generic response wrapper (new)
/// Generic JSON-RPC response wrapper model.
#[path = "core/rpc_response_wrapper.rs"]
pub mod rpc_response_wrapper;

// Block and transaction types
/// Block response model.
#[path = "ledger/rpc_block.rs"]
pub mod rpc_block;
/// Block header response model.
#[path = "ledger/rpc_block_header.rs"]
pub mod rpc_block_header;
/// Transaction response model.
#[path = "ledger/rpc_transaction.rs"]
pub mod rpc_transaction;

// Contract and execution types
/// Contract state response model.
#[path = "contracts/rpc_contract_state.rs"]
pub mod rpc_contract_state;
/// Contract invocation result model.
#[path = "contracts/rpc_invoke_result.rs"]
pub mod rpc_invoke_result;
/// Contract method token model.
#[path = "contracts/rpc_method_token.rs"]
pub mod rpc_method_token;
/// NEF file response model.
#[path = "contracts/rpc_nef_file.rs"]
pub mod rpc_nef_file;
/// VM stack item response model.
#[path = "contracts/rpc_stack.rs"]
pub mod rpc_stack;

// NEP17 token types
/// NEP-17 balance response models.
#[path = "tokens/rpc_nep17_balances.rs"]
pub mod rpc_nep17_balances;
/// NEP-17 token information model.
#[path = "tokens/rpc_nep17_token_info.rs"]
pub mod rpc_nep17_token_info;
/// NEP-17 transfer response models.
#[path = "tokens/rpc_nep17_transfers.rs"]
pub mod rpc_nep17_transfers;

// NEP11 token types
/// NEP-11 balance response models.
#[path = "tokens/rpc_nep11_balances.rs"]
pub mod rpc_nep11_balances;
/// NEP-11 transfer response models.
#[path = "tokens/rpc_nep11_transfers.rs"]
pub mod rpc_nep11_transfers;

// State service types
/// State service search response model.
#[path = "state/rpc_found_states.rs"]
pub mod rpc_found_states;
/// State root response model.
#[path = "state/rpc_state_root.rs"]
pub mod rpc_state_root;

// Network and peer types
/// Peer list response models.
#[path = "network/rpc_peers.rs"]
pub mod rpc_peers;
/// Plugin information model.
#[path = "network/rpc_plugin.rs"]
pub mod rpc_plugin;
/// Node version response model.
#[path = "network/rpc_version.rs"]
pub mod rpc_version;

// Application and execution types
/// Application log response models.
#[path = "execution/rpc_application_log.rs"]
pub mod rpc_application_log;
/// Raw mempool response model.
#[path = "ledger/rpc_raw_mem_pool.rs"]
pub mod rpc_raw_mem_pool;

// Account and wallet types
/// Wallet account response model.
#[path = "wallet/rpc_account.rs"]
pub mod rpc_account;
/// Wallet transfer output model.
#[path = "wallet/rpc_transfer_out.rs"]
pub mod rpc_transfer_out;
/// Unclaimed GAS response model.
#[path = "wallet/rpc_unclaimed_gas.rs"]
pub mod rpc_unclaimed_gas;
/// Address validation response model.
#[path = "wallet/rpc_validate_address_result.rs"]
pub mod rpc_validate_address_result;
/// Validator response model.
#[path = "wallet/rpc_validator.rs"]
pub mod rpc_validator;

// Optional/legacy modules (kept for compatibility)
/// Legacy get-peers response model.
#[path = "network/rpc_get_peers.rs"]
pub mod rpc_get_peers;
/// Legacy accepted-mempool response model.
#[path = "ledger/rpc_mempool_accepted.rs"]
pub mod rpc_mempool_accepted;
/// Legacy unverified-mempool response model.
#[path = "ledger/rpc_mempool_unverified.rs"]
pub mod rpc_mempool_unverified;
/// Legacy method invocation model.
#[path = "execution/rpc_method_invocation.rs"]
pub mod rpc_method_invocation;
/// Legacy notification event model.
#[path = "execution/rpc_notification_event.rs"]
pub mod rpc_notification_event;
/// Oracle response model.
#[path = "state/rpc_oracle_response.rs"]
pub mod rpc_oracle_response;
/// Legacy plugin information model.
#[path = "network/rpc_plugin_info.rs"]
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
