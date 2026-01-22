use super::super::super::utils::{filter_json, ledger_height, select_oracle_key, sign_transaction};
use super::super::super::{OracleService, OracleServiceError};
use crate::network::p2p::payloads::{OracleResponse, OracleResponseCode};
use crate::persistence::DataCache;
use crate::smart_contract::native::{OracleContract, Role, RoleManagement};
use tracing::{debug, warn};

impl OracleService {
    pub(in super::super::super) async fn process_request(
        &self,
        snapshot: &DataCache,
        request_id: u64,
        request: crate::smart_contract::native::OracleRequest,
    ) -> Result<(), OracleServiceError> {
        debug!(
            target: "neo::oracle",
            request_id,
            url = %request.url,
            "processing oracle request"
        );

        let height = ledger_height(snapshot);
        let oracle_nodes = RoleManagement::new()
            .get_designated_by_role_at(snapshot, Role::Oracle, height)
            .map_err(|err| OracleServiceError::Processing(err.to_string()))?;
        if oracle_nodes.is_empty() {
            return Err(OracleServiceError::Processing(
                "oracle nodes not designated".to_string(),
            ));
        }

        let oracle_key = self
            .wallet
            .read()
            .clone()
            .and_then(|wallet| select_oracle_key(wallet.as_ref(), &oracle_nodes));
        let (mut code, data) = self.process_url(&request.url, oracle_key.as_ref()).await;
        let response_pairs = OracleContract::new()
            .get_requests_by_url(snapshot, &request.url)
            .map_err(|err| OracleServiceError::Processing(err.to_string()))?;

        let mut tasks = Vec::new();
        for (pending_id, pending_request) in response_pairs {
            let mut response = OracleResponse::new(pending_id, code, Vec::new());

            if response.code == OracleResponseCode::Success {
                match filter_json(&data, pending_request.filter.as_deref()) {
                    Ok(result) => response.result = result,
                    Err(err) => {
                        response.code = OracleResponseCode::Error;
                        code = OracleResponseCode::Error;
                        warn!(
                            target: "neo::oracle",
                            request_id,
                            filter = ?pending_request.filter,
                            error = %err,
                            "oracle filter failed"
                        );
                    }
                }
            }

            let response_tx = self.create_response_tx(
                snapshot,
                &pending_request,
                &mut response,
                &oracle_nodes,
                self.system.settings(),
                false,
            )?;

            let mut backup_response = OracleResponse::new(
                pending_id,
                OracleResponseCode::ConsensusUnreachable,
                Vec::new(),
            );
            let backup_tx = self.create_response_tx(
                snapshot,
                &pending_request,
                &mut backup_response,
                &oracle_nodes,
                self.system.settings(),
                true,
            )?;

            debug!(
                target: "neo::oracle",
                request_id,
                pending_id,
                response_hash = %response_tx.hash(),
                backup_hash = %backup_tx.hash(),
                code = ?response.code,
                "oracle response transactions built"
            );

            let wallet = self.wallet.read().clone().ok_or_else(|| {
                OracleServiceError::Processing("wallet not available".to_string())
            })?;

            for account in wallet.get_accounts() {
                if !account.has_key() || account.is_locked() {
                    continue;
                }
                let Some(key) = account.get_key() else {
                    continue;
                };
                let Ok(oracle_pub) = key.get_public_key_point() else {
                    continue;
                };
                if !oracle_nodes.iter().any(|p| p == &oracle_pub) {
                    continue;
                }

                let tx_sign = sign_transaction(&response_tx, &key, self.system.settings().network);
                let backup_sign =
                    sign_transaction(&backup_tx, &key, self.system.settings().network);

                self.add_response_tx_sign(
                    snapshot,
                    pending_id,
                    oracle_pub.clone(),
                    tx_sign.clone(),
                    Some(response_tx.clone()),
                    Some(backup_tx.clone()),
                    Some(backup_sign),
                )?;

                tasks.push(self.send_response_signature(pending_id, tx_sign, key));
            }
        }

        if !tasks.is_empty() {
            futures::future::join_all(tasks).await;
        }

        debug!(target: "neo::oracle", request_id, "oracle request processed");
        Ok(())
    }
}
