//! # neo-rpc::types
//!
//! Transport-neutral Neo JSON-RPC request and response records.
//!
//! ## Boundary
//!
//! These records own C#-compatible RPC JSON projections. They do not send
//! client requests, dispatch server methods, or access node services.
//!
//! ## Contents
//!
//! - [`RpcContractState`]: deployed-contract response projection.
//! - [`RpcMethodToken`]: NEF method-token response projection.
//! - [`RpcNefFile`]: NEF response projection.
//! - [`RpcPeers`]: peer-list response projection.
//! - [`RpcRawMemPool`]: raw-mempool response projection.

mod contract_state;
pub(crate) mod json;
mod method_token;
mod nef_file;
mod peers;
mod raw_mempool;

#[cfg(test)]
#[path = "../tests/types/test_fixtures.rs"]
pub(crate) mod test_fixtures;

pub use contract_state::RpcContractState;
pub use method_token::RpcMethodToken;
pub use nef_file::RpcNefFile;
pub use peers::{RpcPeer, RpcPeers};
pub use raw_mempool::RpcRawMemPool;
