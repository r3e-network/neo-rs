use std::sync::Arc;
use std::str::FromStr;
use std::convert::TryFrom;
use bigint::U256;
use neo_core2::config::Config;
use neo_core2::rpcclient::{Client, InternalClient};
use neo_core2::rpcclient::actor::SimpleActor;
use neo_core2::rpcclient::gas::{GasReader, Gas};
use neo_core2::rpcclient::invoker::Invoker;
use neo_core2::util::{Uint160, Uint256};
use neo_core2::wallet::Account;
use neo_core2::internal::testchain;

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::runtime::Runtime;

    #[test]
    fn test_local_client() {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let (_, rpc_srv, _) = init_clear_server_with_custom_config(|cfg: &mut Config| {
                // No addresses configured -> RPC server listens nothing (but it
                // has MaxGasInvoke, sessions and other stuff).
                cfg.application_configuration.rpc.basic_service.enabled = true;
                cfg.application_configuration.rpc.basic_service.addresses = None;
                cfg.application_configuration.rpc.tls_config.addresses = None;
            });

            // RPC server listens nothing (not exposed in any way), but it works for internal clients.
            let c = InternalClient::new(rpc_srv.register_local()).await.unwrap();
            c.init().await.unwrap();

            // Invokers can use this client.
            let gas_reader = GasReader::new(Arc::new(Invoker::new(Arc::new(c.clone()), None)));
            let d = gas_reader.decimals().await.unwrap();
            assert_eq!(8, d);

            // Actors can use it as well
            let priv_key = testchain::private_key_by_id(0);
            let acc = Account::from_private_key(&priv_key);
            let addr = priv_key.public_key().get_script_hash();

            let act = SimpleActor::new(Arc::new(c.clone()), acc).await.unwrap();
            let gasprom = Gas::new(Arc::new(act));
            let (tx_hash, _, _) = gasprom.transfer(&addr, &Uint160::default(), &U256::from(1000), None).await.unwrap();

            // No new blocks are produced here, but the tx is OK and is in the mempool.
            let txes = c.get_raw_mem_pool().await.unwrap();
            assert_eq!(vec![tx_hash], txes);
            // Subscriptions are checked by other tests.
        });
    }
}
