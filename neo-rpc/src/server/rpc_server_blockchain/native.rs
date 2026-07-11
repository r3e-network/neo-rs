//! Native contract and governance RPC handlers.
//!
//! These handlers read native NEO/GAS contract state through the RPC native
//! query facade. Keeping them here avoids mixing governance query flow into the
//! blockchain route map; response projection stays in `responses`.

use std::cmp::Reverse;
use std::sync::Arc;

use crate::server::contract_state_provider::{
    DeployedContractProvider, DeployedContractProviderFactory,
    NativeDeployedContractProviderFactory,
};
use crate::server::native_queries;
use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::internal_error;
use crate::server::rpc_server::RpcServer;
use neo_execution::NativeContract;
use num_traits::ToPrimitive;
use serde_json::Value;

use super::RpcServerBlockchain;
use super::ledger_provider::{
    BlockchainLedgerProvider, BlockchainLedgerProviderFactory,
    NativeBlockchainLedgerProviderFactory,
};
use super::request_helpers::NoParamsRequest;
use super::responses::{
    candidate_to_json, candidates_to_json, committee_to_json, native_contracts_to_json,
    next_block_validator_to_json, next_block_validators_to_json,
};

impl RpcServerBlockchain {
    pub(super) fn get_native_contracts(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        NoParamsRequest::parse(params, "getnativecontracts")?;
        if let Some(remote) = server.remote_ledger_rpc() {
            return remote
                .call("getnativecontracts", &[])
                .map_err(RpcException::from);
        }
        let system = server.system();
        let store = system.store_cache();
        let settings = system.settings();
        let block_height = NativeBlockchainLedgerProviderFactory::new(system.as_ref())
            .provider()
            .current_height(store.data_cache())?;

        let registry = native_queries::NativeQueries::native_registry();
        let mut contract_states = Vec::new();
        let deployed_contracts = NativeDeployedContractProviderFactory.provider();

        for contract in registry.contracts() {
            let state = deployed_contracts
                .contract_state_by_hash(store.data_cache(), &contract.hash())
                .map_err(internal_error)?
                .or_else(|| {
                    <neo_native_contracts::StandardNativeContract as NativeContract<
                        neo_native_contracts::StandardNativeProvider,
                    >>::contract_state(&contract, &settings, block_height)
                });

            if let Some(state) = state {
                contract_states.push(state);
            }
        }

        contract_states.sort_by_key(|state| Reverse(state.id));

        Ok(native_contracts_to_json(&contract_states))
    }

    pub(super) fn get_next_block_validators(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        NoParamsRequest::parse(params, "getnextblockvalidators")?;
        if let Some(remote) = server.remote_ledger_rpc() {
            return remote
                .call("getnextblockvalidators", &[])
                .map_err(RpcException::from);
        }
        let system = server.system();
        let store = system.store_cache();
        let snapshot = Arc::new(store.data_cache().clone());
        let neo_hash = native_queries::NativeQueries::neo_script_hash();
        let validators = native_queries::NativeQueries::neo_next_block_validators(
            server,
            Arc::clone(&snapshot),
            &neo_hash,
        )
        .map_err(internal_error)?;
        let mut result = Vec::with_capacity(validators.len());
        for point in validators {
            let votes = native_queries::NativeQueries::neo_candidate_vote(
                server,
                Arc::clone(&snapshot),
                &neo_hash,
                &point,
            )
            .map_err(internal_error)?;
            let votes_value = votes.to_i64().ok_or_else(|| {
                RpcException::from(
                    RpcError::internal_server_error().with_data("candidate vote out of range"),
                )
            })?;
            result.push(next_block_validator_to_json(&point, votes_value));
        }
        Ok(next_block_validators_to_json(result))
    }

    pub(super) fn get_candidates(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        NoParamsRequest::parse(params, "getcandidates")?;
        if let Some(remote) = server.remote_ledger_rpc() {
            return remote
                .call("getcandidates", &[])
                .map_err(RpcException::from);
        }
        let system = server.system();
        let store = system.store_cache();
        let snapshot = Arc::new(store.data_cache().clone());
        let neo_hash = native_queries::NativeQueries::neo_script_hash();
        let candidates =
            native_queries::NativeQueries::neo_candidates(server, Arc::clone(&snapshot), &neo_hash)
                .map_err(|_| {
                    RpcException::from(
                        RpcError::internal_server_error().with_data("Can't get candidates."),
                    )
                })?;
        let validators = native_queries::NativeQueries::neo_next_block_validators(
            server,
            Arc::clone(&snapshot),
            &neo_hash,
        )
        .map_err(|_| {
            RpcException::from(
                RpcError::internal_server_error().with_data("Can't get next block validators."),
            )
        })?;
        let mut result = Vec::with_capacity(candidates.len());
        for (point, votes) in &candidates {
            let active = validators.iter().any(|validator| validator == point);
            result.push(candidate_to_json(point, votes, active));
        }
        Ok(candidates_to_json(result))
    }

    pub(super) fn get_committee(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        NoParamsRequest::parse(params, "getcommittee")?;
        if let Some(remote) = server.remote_ledger_rpc() {
            return remote.call("getcommittee", &[]).map_err(RpcException::from);
        }
        let store = server.system().store_cache();
        let snapshot = Arc::new(store.data_cache().clone());
        let neo_hash = native_queries::NativeQueries::neo_script_hash();
        let committee = native_queries::NativeQueries::neo_committee(server, snapshot, &neo_hash)
            .map_err(|err| {
            let error = RpcError::internal_server_error()
                .with_data(format!("committee not available: {err}"));
            RpcException::from(error)
        })?;
        Ok(committee_to_json(&committee))
    }
}
