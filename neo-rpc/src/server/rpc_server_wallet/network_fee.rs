//! Wallet transaction network-fee estimation handler.

use neo_primitives::UInt160;
use serde_json::Value;

use super::RpcServerWallet;
use super::request::NetworkFeeRequest;
use super::response::network_fee_to_json;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::invalid_params;
use crate::server::rpc_server::RpcServer;
use crate::server::wallet_compat;

impl RpcServerWallet {
    pub(super) fn calculate_network_fee(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let request = NetworkFeeRequest::parse(params)?;
        let system = server.system();
        let store = system.store_cache();
        let settings = system.settings();
        let native_contract_provider = system.native_contract_provider();
        let wallet = server.wallet();
        let account_script = |hash: &UInt160| -> Option<Vec<u8>> {
            wallet.as_ref().and_then(|wallet| {
                wallet
                    .account(hash)
                    .and_then(|account| account.contract().map(|contract| contract.script.clone()))
            })
        };
        let fee = wallet_compat::calculate_network_fee(
            &request.transaction,
            store.data_cache(),
            &settings,
            &native_contract_provider,
            &account_script,
            server.settings().max_gas_invoke,
        )
        .map_err(|err| invalid_params(err.to_string()))?;
        Ok(network_fee_to_json(fee))
    }
}
