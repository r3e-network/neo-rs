//! NEP account-balance RPC handlers.
//!
//! Balance handlers read tracker index records, enrich each asset through the
//! native contract snapshot, and build Neo-compatible NEP-11/NEP-17 response
//! payloads. The root module remains the route map.

use std::collections::HashMap;
use std::sync::Arc;

use crate::plugins::tokens_tracker::{
    Nep11BalanceKey, Nep11Tracker, Nep17BalanceKey, Nep17Tracker, TokenBalance, find_prefix,
};
use crate::server::contract_state_provider::{
    DeployedContractProvider, DeployedContractProviderFactory,
    NativeDeployedContractProviderFactory,
};
use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::internal_error;
use crate::server::rpc_server::RpcServer;
use neo_primitives::{UInt160, hex_util};
use serde_json::Value;

use super::RpcServerTokensTracker;
use super::helpers::{query_asset_metadata, tracker_service};
use super::request::AccountRequest;
use super::response::{
    account_balances, nep11_balance_entry, nep11_token_entry, nep17_balance_entry,
};

impl RpcServerTokensTracker {
    pub(super) fn get_nep11_balances(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let service = tracker_service(server)?;
        if !service.settings().enabled_nep11() {
            return Err(RpcException::from(RpcError::method_not_found()));
        }

        let address_version = server.system().settings().address_version;
        let request = AccountRequest::parse(params, "getnep11balances", address_version)?;

        let (balance_prefix, _, _) = Nep11Tracker::rpc_prefixes();
        let mut prefix = Vec::with_capacity(1 + UInt160::LENGTH);
        prefix.push(balance_prefix);
        prefix.extend_from_slice(&request.script_hash.to_bytes());

        let balances =
            find_prefix::<_, Nep11BalanceKey, TokenBalance>(service.store().as_ref(), &prefix)
                .map_err(internal_error)?;

        let store_cache = server.system().store_cache();
        let snapshot = Arc::new(store_cache.data_cache().clone());
        let max_results = service.settings().max_results_limit();
        let deployed_contracts = NativeDeployedContractProviderFactory.provider();

        let mut grouped: HashMap<UInt160, Vec<(String, TokenBalance)>> = HashMap::new();
        let mut count = 0usize;

        for (key, value) in balances {
            if count >= max_results {
                break;
            }
            let Some(_) = deployed_contracts
                .contract_state_by_hash(snapshot.as_ref(), &key.asset_script_hash)
                .map_err(|err| internal_error(err.to_string()))?
            else {
                continue;
            };

            grouped
                .entry(key.asset_script_hash)
                .or_default()
                .push((hex_util::encode_hex(&key.token), value));
            count += 1;
        }

        let mut results = Vec::new();
        for (asset, tokens) in grouped {
            let Some(contract) = deployed_contracts
                .contract_state_by_hash(snapshot.as_ref(), &asset)
                .map_err(|err| internal_error(err.to_string()))?
            else {
                continue;
            };

            let Some((symbol, decimals)) = query_asset_metadata(
                snapshot.as_ref(),
                &server.system().settings(),
                server.system().native_contract_provider(),
                &asset,
            ) else {
                continue;
            };

            let token_entries = tokens
                .into_iter()
                .map(|(token_id, balance)| nep11_token_entry(token_id, balance))
                .collect::<Vec<_>>();

            results.push(nep11_balance_entry(
                &asset,
                &contract.manifest.name,
                &symbol,
                decimals,
                token_entries,
            ));
        }

        Ok(account_balances(
            &request.script_hash,
            address_version,
            results,
        ))
    }

    pub(super) fn get_nep17_balances(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let service = tracker_service(server)?;
        if !service.settings().enabled_nep17() {
            return Err(RpcException::from(RpcError::method_not_found()));
        }

        let address_version = server.system().settings().address_version;
        let request = AccountRequest::parse(params, "getnep17balances", address_version)?;

        let (balance_prefix, _, _) = Nep17Tracker::rpc_prefixes();
        let mut prefix = Vec::with_capacity(1 + UInt160::LENGTH);
        prefix.push(balance_prefix);
        prefix.extend_from_slice(&request.script_hash.to_bytes());

        let balances =
            find_prefix::<_, Nep17BalanceKey, TokenBalance>(service.store().as_ref(), &prefix)
                .map_err(internal_error)?;

        let store_cache = server.system().store_cache();
        let snapshot = Arc::new(store_cache.data_cache().clone());
        let mut results = Vec::new();
        let max_results = service.settings().max_results_limit();
        let deployed_contracts = NativeDeployedContractProviderFactory.provider();

        for (key, value) in balances {
            if results.len() >= max_results {
                break;
            }
            let Some(contract) = deployed_contracts
                .contract_state_by_hash(snapshot.as_ref(), &key.asset_script_hash)
                .map_err(|err| internal_error(err.to_string()))?
            else {
                continue;
            };

            let Some((symbol, decimals)) = query_asset_metadata(
                snapshot.as_ref(),
                &server.system().settings(),
                server.system().native_contract_provider(),
                &key.asset_script_hash,
            ) else {
                continue;
            };

            results.push(nep17_balance_entry(
                &key.asset_script_hash,
                &contract.manifest.name,
                &symbol,
                decimals,
                &value,
            ));
        }

        Ok(account_balances(
            &request.script_hash,
            address_version,
            results,
        ))
    }
}
