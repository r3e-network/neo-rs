//! # neo-rpc::server::native_queries
//!
//! Shared native-contract query helpers used by RPC handlers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `execution`: Read-only native-call engine execution.
//! - `registry`: Standard native-contract registry construction.
//! - `result`: Native stack-result decoding.
//! - `script`: C#-compatible dynamic-call script construction.

mod execution;
mod registry;
mod result;
mod script;

use std::sync::Arc;

use neo_error::{CoreError, CoreResult};
use neo_primitives::UInt160;
use neo_storage::persistence::DataCache;
use num_bigint::BigInt;

use crate::server::rpc_server::RpcServer;
use execution::invoke_native_read;
use result::{candidate_entries, stack_array_of_bytes};
use script::NativeArg;

/// Engine-script probes for native-contract reads.
pub(crate) struct NativeQueries;

impl NativeQueries {
    /// `NEO.unclaimedGas(account, end)` — the amount of unclaimed GAS for
    /// `account` at the `end` block height.
    pub(crate) fn neo_unclaimed_gas(
        server: &RpcServer,
        snapshot: Arc<DataCache>,
        neo_hash: &UInt160,
        account: &UInt160,
        end: u32,
    ) -> CoreResult<BigInt> {
        let account_bytes = account.to_bytes();
        let item = invoke_native_read(
            server,
            snapshot,
            neo_hash,
            "unclaimedGas",
            &[
                NativeArg::Bytes(account_bytes.as_slice()),
                NativeArg::Int(i64::from(end)),
            ],
        )?;
        item.as_int()
            .map_err(|err| CoreError::other(err.to_string()))
    }

    /// `NEO.getCommittee()` — the current committee public keys (sorted).
    pub(crate) fn neo_committee(
        server: &RpcServer,
        snapshot: Arc<DataCache>,
        neo_hash: &UInt160,
    ) -> CoreResult<Vec<Vec<u8>>> {
        let item = invoke_native_read(server, snapshot, neo_hash, "getCommittee", &[])?;
        stack_array_of_bytes(&item)
    }

    /// `NEO.getNextBlockValidators()` — the validators for the next block.
    pub(crate) fn neo_next_block_validators(
        server: &RpcServer,
        snapshot: Arc<DataCache>,
        neo_hash: &UInt160,
    ) -> CoreResult<Vec<Vec<u8>>> {
        let item = invoke_native_read(server, snapshot, neo_hash, "getNextBlockValidators", &[])?;
        stack_array_of_bytes(&item)
    }

    /// `NEO.getCandidates()` — registered candidates with their votes.
    pub(crate) fn neo_candidates(
        server: &RpcServer,
        snapshot: Arc<DataCache>,
        neo_hash: &UInt160,
    ) -> CoreResult<Vec<(Vec<u8>, BigInt)>> {
        let item = invoke_native_read(server, snapshot, neo_hash, "getCandidates", &[])?;
        candidate_entries(&item)
    }

    /// `NEO.getCandidateVote(pubkey)` — the candidate's vote count, or `-1`
    /// when the key is not a registered candidate.
    pub(crate) fn neo_candidate_vote(
        server: &RpcServer,
        snapshot: Arc<DataCache>,
        neo_hash: &UInt160,
        pubkey: &[u8],
    ) -> CoreResult<BigInt> {
        let item = invoke_native_read(
            server,
            snapshot,
            neo_hash,
            "getCandidateVote",
            &[NativeArg::Bytes(pubkey)],
        )?;
        item.as_int()
            .map_err(|err| CoreError::other(err.to_string()))
    }
}
