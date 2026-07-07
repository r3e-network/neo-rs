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
//! - `result`: Native stack-result decoding.
//! - `script`: C#-compatible dynamic-call script construction.

mod result;
mod script;

use std::sync::Arc;

use neo_error::{CoreError, CoreResult};
use neo_execution::ApplicationEngine;
use neo_manifest::CallFlags;
use neo_primitives::{TriggerType, UInt160};
use neo_storage::persistence::DataCache;
use neo_vm::StackItem;
use neo_vm_rs::VmState as VMState;
use num_bigint::BigInt;

use crate::server::rpc_server::RpcServer;
use result::{candidate_entries, stack_array_of_bytes};
use script::{NativeArg, build_native_call_script};

/// Engine-script probes for native-contract reads.
pub(crate) struct NativeQueries;

impl NativeQueries {
    /// Builds a [`neo_execution::NativeRegistry`] populated with the
    /// standard native contracts. `NativeRegistry::new()` is *empty* by
    /// design; the canonical contract set lives in
    /// [`neo_native_contracts::standard_native_contracts`].
    pub(crate) fn native_registry() -> neo_execution::NativeRegistry {
        let mut registry = neo_execution::NativeRegistry::new();
        for contract in neo_native_contracts::standard_native_contracts() {
            registry.register(contract);
        }
        registry
    }

    /// Runs a single read-only native-method call against `snapshot` and
    /// returns the top of the result stack. Faults are surfaced as errors
    /// (the native reads probed here cannot fault on healthy state).
    pub(crate) fn invoke_native_read(
        server: &RpcServer,
        snapshot: Arc<DataCache>,
        contract: &UInt160,
        method: &str,
        args: &[NativeArg<'_>],
    ) -> CoreResult<StackItem> {
        let script = build_native_call_script(contract, method, args)?;

        let system = server.system();
        let settings = system.settings().as_ref().clone();
        let mut engine = ApplicationEngine::new_with_shared_block_and_native_contract_provider(
            TriggerType::Application,
            None,
            snapshot,
            None,
            settings,
            server.settings().max_gas_invoke,
            None,
            Some(system.native_contract_provider()),
        )
        .map_err(|err| CoreError::other(err.to_string()))?;
        engine
            .load_script(script, CallFlags::READ_ONLY, None)
            .map_err(|err| CoreError::other(err.to_string()))?;
        let state = engine.execute_allow_fault();
        if state != VMState::HALT {
            return Err(CoreError::other(format!(
                "native read '{method}' did not HALT (VM state: {state:?})"
            )));
        }
        engine
            .result_stack()
            .peek(0)
            .cloned()
            .map_err(|err| CoreError::other(err.to_string()))
    }

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
        let item = NativeQueries::invoke_native_read(
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
        let item =
            NativeQueries::invoke_native_read(server, snapshot, neo_hash, "getCommittee", &[])?;
        stack_array_of_bytes(&item)
    }

    /// `NEO.getNextBlockValidators()` — the validators for the next block.
    pub(crate) fn neo_next_block_validators(
        server: &RpcServer,
        snapshot: Arc<DataCache>,
        neo_hash: &UInt160,
    ) -> CoreResult<Vec<Vec<u8>>> {
        let item = NativeQueries::invoke_native_read(
            server,
            snapshot,
            neo_hash,
            "getNextBlockValidators",
            &[],
        )?;
        stack_array_of_bytes(&item)
    }

    /// `NEO.getCandidates()` — registered candidates with their votes.
    pub(crate) fn neo_candidates(
        server: &RpcServer,
        snapshot: Arc<DataCache>,
        neo_hash: &UInt160,
    ) -> CoreResult<Vec<(Vec<u8>, BigInt)>> {
        let item =
            NativeQueries::invoke_native_read(server, snapshot, neo_hash, "getCandidates", &[])?;
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
        let item = NativeQueries::invoke_native_read(
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
