//! Pending-queue signature aggregation, threshold relay and inbound submissions.
//!
//! Mirrors the reference implementation's `service/transactions/{queue,signature}.rs`
//! and `service/processing/requests/submit.rs`.
//!
//! `transactions/response.rs` (fee/witness computation for the response
//! transaction) is intentionally *not* duplicated here: it is expressed by the
//! [`ResponseTxBuilder`](crate::ports::ResponseTxBuilder) port so the Oracle
//! service stays free of ApplicationEngine and native-contract coupling.

use super::{
    MAX_PENDING_QUEUE_SIZE, MAX_TASK_PENDING_TIME, OracleService, OracleServiceError, OracleSigner,
    OracleSnapshot, OracleTask, OracleWallet, ResponseTxBuilder, SIGNATURE_SEND_TIMEOUT,
    SignatureVerifier, SnapshotFactory, TxRelay,
};

use base64::Engine as _;
use neo_crypto::{Crypto, ECPoint};
use neo_primitives::UInt160;
use neo_protocol::contract::Contract;
use neo_protocol::get_sign_data_vec;
use neo_protocol::payloads::Transaction;
use neo_vm::ScriptBuilder;
use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::time::SystemTime;
use tokio::task::JoinHandle;

impl<S, W, Sig, TB, R, V> OracleService<S, W, Sig, TB, R, V>
where
    S: SnapshotFactory + Send + Sync + 'static,
    S::Snapshot: OracleSnapshot,
    W: OracleWallet + Send + Sync + 'static,
    Sig: OracleSigner + Send + Sync + 'static,
    TB: ResponseTxBuilder + Send + Sync + 'static,
    R: TxRelay + Send + Sync + 'static,
    V: SignatureVerifier + Send + Sync + 'static,
{
    /// Records a signature for `request_id`, validating and aggregating it
    /// into the pending queue, and relays the response transaction once the
    /// multi-sig threshold is reached.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn add_response_tx_sign(
        &self,
        snapshot: &S::Snapshot,
        request_id: u64,
        oracle_pub: ECPoint,
        sign: Vec<u8>,
        response_tx: Option<Transaction>,
        backup_tx: Option<Transaction>,
        backup_sign: Option<Vec<u8>>,
    ) -> Result<(), OracleServiceError> {
        // Validate queue size to prevent memory exhaustion
        {
            let queue = self.pending_queue.lock();
            if queue.len() >= MAX_PENDING_QUEUE_SIZE && !queue.contains_key(&request_id) {
                return Err(OracleServiceError::Processing(
                    "Pending queue is full".to_string(),
                ));
            }
        }

        let mut queue = self.pending_queue.lock();
        if let std::collections::hash_map::Entry::Vacant(e) = queue.entry(request_id) {
            let request = snapshot
                .get_request(request_id)
                .map_err(|err| OracleServiceError::Processing(err.to_string()))?
                .ok_or(OracleServiceError::RequestNotFound)?;
            if snapshot
                .transaction_block_index(&request.original_tx_id)
                .map_err(|err| OracleServiceError::Processing(err.to_string()))?
                .is_none()
            {
                return Err(OracleServiceError::RequestTransactionNotFound);
            }

            // Validate URL before adding to queue
            if !self.settings.is_url_allowed(&request.url) {
                return Err(OracleServiceError::UrlBlocked);
            }

            e.insert(OracleTask {
                tx: None,
                backup_tx: None,
                signs: BTreeMap::new(),
                backup_signs: BTreeMap::new(),
                timestamp: SystemTime::now(),
            });
        }
        let task = queue.get_mut(&request_id).expect("oracle task inserted");

        // Check if task has been pending for too long
        if let Ok(elapsed) = SystemTime::now().duration_since(task.timestamp)
            && elapsed > MAX_TASK_PENDING_TIME
        {
            return Err(OracleServiceError::Processing(
                "Task has been pending for too long".to_string(),
            ));
        }

        if let Some(tx) = response_tx {
            // Validate transaction before storing
            if tx.valid_until_block() <= snapshot.ledger_height() {
                return Err(OracleServiceError::Processing(
                    "Transaction has expired".to_string(),
                ));
            }

            task.tx = Some(tx.clone());
            let data = get_sign_data_vec(&tx, self.settings.network)
                .map_err(|err| OracleServiceError::Processing(err.to_string()))?;
            task.signs
                .retain(|key, value| self.verify_oracle_signature(key, &data, value));
        }

        if let Some(tx) = backup_tx {
            task.backup_tx = Some(tx.clone());
            let data = get_sign_data_vec(&tx, self.settings.network)
                .map_err(|err| OracleServiceError::Processing(err.to_string()))?;
            task.backup_signs
                .retain(|key, value| self.verify_oracle_signature(key, &data, value));
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

        let tx_data = get_sign_data_vec(tx, self.settings.network)
            .map_err(|err| OracleServiceError::Processing(err.to_string()))?;
        let backup_data = get_sign_data_vec(backup_tx, self.settings.network)
            .map_err(|err| OracleServiceError::Processing(err.to_string()))?;

        if self.verify_oracle_signature(&oracle_pub, &tx_data, &sign) {
            task.signs.insert(oracle_pub.clone(), sign);
        } else if self.verify_oracle_signature(&oracle_pub, &backup_data, &sign) {
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
                .insert_at(request_id, SystemTime::UNIX_EPOCH);
            queue.remove(&request_id);
        }

        Ok(())
    }

    /// Aggregates `signs` into a multi-sig invocation script and, once the
    /// `n - (n - 1) / 3` threshold is met and the transaction is still valid,
    /// relays it. Returns true when the transaction was accepted for relay.
    pub(crate) fn check_tx_sign(
        &self,
        snapshot: &S::Snapshot,
        tx: &Transaction,
        signs: &BTreeMap<ECPoint, Vec<u8>>,
    ) -> bool {
        let height = snapshot.ledger_height();
        if tx.valid_until_block() <= height {
            return false;
        }

        let Ok(oracle_nodes) = snapshot.designated_oracles(height) else {
            return false;
        };

        if oracle_nodes.is_empty() {
            return false;
        }

        let needed_threshold = oracle_nodes.len() - (oracle_nodes.len() - 1) / 3;
        if signs.len() < needed_threshold {
            return false;
        }

        let contract = Contract::create_multi_sig_contract(needed_threshold, &oracle_nodes);
        let mut builder = ScriptBuilder::new();
        let mut remaining = needed_threshold;
        for sign in signs.values() {
            builder.emit_push(sign.as_slice());
            remaining -= 1;
            if remaining == 0 {
                break;
            }
        }
        let invocation_script = builder.to_array();

        let hashes = tx.signer_accounts();
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

        if let Err(error) = self.relay.relay_transaction(tx_mut) {
            tracing::warn!(target: "neo::oracle", %error, "failed to relay oracle response tx");
            return false;
        }

        tracing::debug!(target: "neo::oracle", tx = %tx.hash(), "oracle response tx relayed");
        true
    }

    /// Signs the submission message for `request_id` and spawns the task that
    /// broadcasts `submitoracleresponse` to every configured oracle node.
    pub(crate) async fn send_response_signature(
        &self,
        request_id: u64,
        tx_sign: Vec<u8>,
        account: UInt160,
        public_key: ECPoint,
    ) -> Result<JoinHandle<()>, OracleServiceError> {
        let mut message = Vec::with_capacity(public_key.as_bytes().len() + 8 + tx_sign.len());
        message.extend_from_slice(public_key.as_bytes());
        message.extend_from_slice(&request_id.to_le_bytes());
        message.extend_from_slice(&tx_sign);
        let operation_id = format!(
            "oracle-submission:{}:{}:{}:{}",
            self.settings.network,
            request_id,
            account,
            hex::encode(Crypto::sha256(&message)),
        );
        let sign = self
            .sign_neo_payload(
                operation_id,
                account,
                message,
                "neo-n3-oracle-response-submission",
                BTreeMap::from([
                    ("request_id".to_string(), request_id.to_string()),
                    (
                        "oracle_public_key".to_string(),
                        hex::encode(public_key.as_bytes()),
                    ),
                ]),
            )
            .await?;

        let settings = self.settings.clone();
        let counter = self.counter.fetch_add(1, Ordering::SeqCst);
        let https = self.https.clone();
        Ok(tokio::spawn(async move {
            let payload = serde_json::json!({
                "id": counter,
                "jsonrpc": "2.0",
                "method": "submitoracleresponse",
                "params": [
                    base64::engine::general_purpose::STANDARD.encode(public_key.as_bytes()),
                    request_id,
                    base64::engine::general_purpose::STANDARD.encode(&tx_sign),
                    base64::engine::general_purpose::STANDARD.encode(&sign)
                ]
            });

            for node in settings.nodes.iter() {
                let Ok(url) = url::Url::parse(node) else {
                    tracing::warn!(target: "neo::oracle", node = %node, "invalid oracle node endpoint");
                    continue;
                };
                let res = https
                    .client()
                    .post(url)
                    .timeout(SIGNATURE_SEND_TIMEOUT)
                    .json(&payload)
                    .send()
                    .await;
                match res {
                    Ok(response) => {
                        if !response.status().is_success() {
                            tracing::warn!(
                                target: "neo::oracle",
                                node = %node,
                                status = %response.status(),
                                "oracle signature rejected"
                            );
                        }
                    }
                    Err(err) => {
                        tracing::warn!(target: "neo::oracle", node = %node, %err, "failed to send oracle signature");
                    }
                }
            }
        }))
    }

    /// Records a validator's response signatures for `request_id` and
    /// submits the response transaction once enough signatures accumulate.
    pub fn submit_oracle_response(
        &self,
        oracle_pub: ECPoint,
        request_id: u64,
        tx_sign: Vec<u8>,
        msg_sign: Vec<u8>,
    ) -> Result<(), OracleServiceError> {
        if !self.is_running() {
            return Err(OracleServiceError::Disabled);
        }

        if self.finished_cache.lock().contains(&request_id) {
            return Err(OracleServiceError::RequestFinished);
        }

        let snapshot = self.snapshots.snapshot();
        let height = snapshot.ledger_height();
        let oracles = snapshot
            .designated_oracles(height)
            .map_err(|err| OracleServiceError::Processing(err.to_string()))?;

        if !oracles.iter().any(|key| key == &oracle_pub) {
            return Err(OracleServiceError::NotDesignated(format!(
                "{} isn't an oracle node.",
                oracle_pub
            )));
        }

        let request = snapshot
            .get_request(request_id)
            .map_err(|err| OracleServiceError::Processing(err.to_string()))?;
        if request.is_none() {
            return Err(OracleServiceError::RequestNotFound);
        }

        let mut message = Vec::with_capacity(oracle_pub.as_bytes().len() + 8 + tx_sign.len());
        message.extend_from_slice(oracle_pub.as_bytes());
        message.extend_from_slice(&request_id.to_le_bytes());
        message.extend_from_slice(&tx_sign);

        if !self.verify_oracle_signature(&oracle_pub, &message, &msg_sign) {
            return Err(OracleServiceError::InvalidSignature(format!(
                "Invalid oracle response transaction signature from '{}'.",
                oracle_pub
            )));
        }

        self.add_response_tx_sign(&snapshot, request_id, oracle_pub, tx_sign, None, None, None)
    }

    /// Gets the current pending queue size (for monitoring).
    pub fn pending_queue_size(&self) -> usize {
        self.pending_queue.lock().len()
    }

    /// Checks if the pending queue is at capacity.
    pub fn is_queue_full(&self) -> bool {
        self.pending_queue.lock().len() >= MAX_PENDING_QUEUE_SIZE
    }
}
