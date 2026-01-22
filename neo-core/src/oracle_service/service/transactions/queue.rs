use super::super::utils::{ledger_height, verify_oracle_signature};
use super::super::{OracleService, OracleServiceError, OracleTask};
use crate::cryptography::ECPoint;
use crate::neo_system::TransactionRouterMessage;
use crate::network::p2p::helper::get_sign_data_vec;
use crate::network::p2p::payloads::Transaction;
use crate::persistence::DataCache;
use crate::smart_contract::native::{LedgerContract, OracleContract, Role, RoleManagement};
use crate::smart_contract::Contract;
use crate::IVerifiable;
use std::collections::BTreeMap;
use std::time::SystemTime;
use tracing::{debug, warn};

impl OracleService {
    pub(in super::super) fn add_response_tx_sign(
        &self,
        snapshot: &DataCache,
        request_id: u64,
        oracle_pub: ECPoint,
        sign: Vec<u8>,
        response_tx: Option<Transaction>,
        backup_tx: Option<Transaction>,
        backup_sign: Option<Vec<u8>>,
    ) -> Result<(), OracleServiceError> {
        let mut queue = self.pending_queue.lock();
        if !queue.contains_key(&request_id) {
            let request = OracleContract::new()
                .get_request(snapshot, request_id)
                .map_err(|err| OracleServiceError::Processing(err.to_string()))?
                .ok_or(OracleServiceError::RequestNotFound)?;
            let ledger = LedgerContract::new();
            let _state = ledger
                .get_transaction_state(snapshot, &request.original_tx_id)
                .map_err(|err| OracleServiceError::Processing(err.to_string()))?
                .ok_or(OracleServiceError::RequestTransactionNotFound)?;
            queue.insert(
                request_id,
                OracleTask {
                    tx: None,
                    backup_tx: None,
                    signs: BTreeMap::new(),
                    backup_signs: BTreeMap::new(),
                    timestamp: SystemTime::now(),
                },
            );
        }
        let task = queue.get_mut(&request_id).expect("oracle task inserted");

        if let Some(tx) = response_tx {
            task.tx = Some(tx.clone());
            let data = get_sign_data_vec(&tx, self.system.settings().network)
                .map_err(|err| OracleServiceError::Processing(err.to_string()))?;
            task.signs
                .retain(|key, value| verify_oracle_signature(key, &data, value));
        }

        if let Some(tx) = backup_tx {
            task.backup_tx = Some(tx.clone());
            let data = get_sign_data_vec(&tx, self.system.settings().network)
                .map_err(|err| OracleServiceError::Processing(err.to_string()))?;
            task.backup_signs
                .retain(|key, value| verify_oracle_signature(key, &data, value));
            if let Some(backup_sign) = backup_sign {
                task.backup_signs.insert(oracle_pub.clone(), backup_sign);
            }
        }

        if task.tx.is_none() {
            task.signs.insert(oracle_pub.clone(), sign.clone());
            task.backup_signs.insert(oracle_pub, sign);
            return Ok(());
        }

        let tx = task.tx.as_ref().expect("oracle tx available");
        let backup_tx = task.backup_tx.as_ref().expect("oracle backup tx available");

        let tx_data = get_sign_data_vec(tx, self.system.settings().network)
            .map_err(|err| OracleServiceError::Processing(err.to_string()))?;
        let backup_data = get_sign_data_vec(backup_tx, self.system.settings().network)
            .map_err(|err| OracleServiceError::Processing(err.to_string()))?;

        if verify_oracle_signature(&oracle_pub, &tx_data, &sign) {
            task.signs.insert(oracle_pub.clone(), sign);
        } else if verify_oracle_signature(&oracle_pub, &backup_data, &sign) {
            task.backup_signs.insert(oracle_pub.clone(), sign);
        } else {
            return Err(OracleServiceError::InvalidSignature(format!(
                "Invalid oracle response transaction signature from '{}'.",
                oracle_pub
            )));
        }

        let tx_ready = self.check_tx_sign(snapshot, tx, &task.signs);
        let backup_ready = self.check_tx_sign(snapshot, backup_tx, &task.backup_signs);
        if tx_ready || backup_ready {
            // Match C# plugin behavior: finished cache entries are cleared on the next timer sweep.
            self.finished_cache
                .lock()
                .insert(request_id, SystemTime::UNIX_EPOCH);
            queue.remove(&request_id);
        }

        Ok(())
    }

    pub(in super::super) fn check_tx_sign(
        &self,
        snapshot: &DataCache,
        tx: &Transaction,
        signs: &BTreeMap<ECPoint, Vec<u8>>,
    ) -> bool {
        let height = ledger_height(snapshot);
        if tx.valid_until_block() <= height {
            return false;
        }

        let oracle_nodes =
            match RoleManagement::new().get_designated_by_role_at(snapshot, Role::Oracle, height) {
                Ok(nodes) => nodes,
                Err(_) => return false,
            };

        if oracle_nodes.is_empty() {
            return false;
        }

        let needed_threshold = oracle_nodes.len() - (oracle_nodes.len() - 1) / 3;
        if signs.len() < needed_threshold {
            return false;
        }

        let contract = Contract::create_multi_sig_contract(needed_threshold, &oracle_nodes);
        let mut builder = neo_vm::ScriptBuilder::new();
        let mut remaining = needed_threshold;
        for (_key, sign) in signs.iter() {
            builder.emit_push(sign.as_slice());
            remaining -= 1;
            if remaining == 0 {
                break;
            }
        }
        let invocation_script = builder.to_array();

        let hashes = tx.get_script_hashes_for_verifying(snapshot);
        let idx = if hashes.first() == Some(&contract.script_hash()) {
            0
        } else {
            1
        };

        let mut tx_mut = tx.clone();
        let mut witnesses = tx_mut.witnesses().to_vec();
        if let Some(witness) = witnesses.get_mut(idx) {
            witness.invocation_script = invocation_script;
        }
        tx_mut.set_witnesses(witnesses);

        if let Err(error) =
            self.system
                .tx_router_actor()
                .tell(TransactionRouterMessage::Preverify {
                    transaction: tx_mut,
                    relay: true,
                })
        {
            warn!(target: "neo::oracle", %error, "failed to relay oracle response tx");
            return false;
        }

        debug!(target: "neo::oracle", tx = %tx.hash(), "oracle response tx relayed");
        true
    }
}
