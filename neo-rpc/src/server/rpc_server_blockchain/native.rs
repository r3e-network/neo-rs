//! Native contract and governance RPC handlers.
//!
//! These handlers read native NEO/GAS contract state through the RPC native
//! query facade. Keeping them here avoids mixing governance projection details
//! into the blockchain route map.

use std::cmp::Reverse;
use std::sync::Arc;

use crate::server::native_queries;
use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::internal_error;
use crate::server::rpc_server::RpcServer;
use neo_native_contracts::{LedgerContract, contract_management::ContractManagement};
use neo_primitives::hex_util;
use num_traits::ToPrimitive;
use serde_json::{Value, json};

use super::RpcServerBlockchain;
use super::responses::contract_state_to_json;

impl RpcServerBlockchain {
    pub(super) fn get_native_contracts(
        server: &RpcServer,
        _params: &[Value],
    ) -> Result<Value, RpcException> {
        if let Some(remote) = server.remote_ledger_rpc() {
            return remote
                .call("getnativecontracts", &[])
                .map_err(RpcException::from);
        }
        let system = server.system();
        let store = system.store_cache();
        let settings = system.settings();
        let ledger = LedgerContract::new();
        let block_height = ledger
            .current_index(store.data_cache())
            .map_err(internal_error)?;

        let registry = native_queries::NativeQueries::native_registry();
        let mut contract_states = Vec::new();

        for contract in registry.contracts() {
            let state = ContractManagement::get_contract_from_snapshot(
                store.data_cache(),
                &contract.hash(),
            )
            .map_err(internal_error)?
            .or_else(|| contract.contract_state(&settings, block_height));

            if let Some(state) = state {
                contract_states.push(state);
            }
        }

        contract_states.sort_by_key(|state| Reverse(state.id));

        Ok(Value::Array(
            contract_states.iter().map(contract_state_to_json).collect(),
        ))
    }

    pub(super) fn get_next_block_validators(
        server: &RpcServer,
        _params: &[Value],
    ) -> Result<Value, RpcException> {
        if let Some(remote) = server.remote_ledger_rpc() {
            return remote
                .call("getnextblockvalidators", &[])
                .map_err(RpcException::from);
        }
        let system = server.system();
        let store = system.store_cache();
        let snapshot = Arc::new(store.data_cache().clone());
        let neo_hash = neo_native_contracts::NeoToken::script_hash();
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
            result.push(json!({
                "publickey": hex_util::encode_hex(&point),
                "votes": votes_value}));
        }
        Ok(Value::Array(result))
    }

    pub(super) fn get_candidates(
        server: &RpcServer,
        _params: &[Value],
    ) -> Result<Value, RpcException> {
        if let Some(remote) = server.remote_ledger_rpc() {
            return remote
                .call("getcandidates", &[])
                .map_err(RpcException::from);
        }
        let system = server.system();
        let store = system.store_cache();
        let snapshot = Arc::new(store.data_cache().clone());
        let neo_hash = neo_native_contracts::NeoToken::script_hash();
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
            result.push(json!({
                "publickey": hex_util::encode_hex(point),
                "votes": votes.to_string(),
                "active": active}));
        }
        Ok(Value::Array(result))
    }

    pub(super) fn get_committee(
        server: &RpcServer,
        _params: &[Value],
    ) -> Result<Value, RpcException> {
        if let Some(remote) = server.remote_ledger_rpc() {
            return remote.call("getcommittee", &[]).map_err(RpcException::from);
        }
        let store = server.system().store_cache();
        let snapshot = Arc::new(store.data_cache().clone());
        let neo_hash = neo_native_contracts::NeoToken::script_hash();
        let committee = native_queries::NativeQueries::neo_committee(server, snapshot, &neo_hash)
            .map_err(|err| {
            let error = RpcError::internal_server_error()
                .with_data(format!("committee not available: {err}"));
            RpcException::from(error)
        })?;
        let members: Vec<Value> = committee
            .iter()
            .map(|point| Value::String(hex_util::encode_hex(point)))
            .collect();
        Ok(Value::Array(members))
    }
}
