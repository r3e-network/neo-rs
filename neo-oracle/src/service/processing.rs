//! Request and timer loops, URL dispatch and per-request processing.
//!
//! Mirrors the reference implementation's `service/processing/*`.

use super::{
    MAX_URL_LENGTH, OracleService, OracleServiceError, OracleSigner, OracleSnapshot, OracleStatus,
    OracleWallet, REFRESH_INTERVAL, ResponseTxBuilder, SignatureVerifier, SnapshotFactory, TxRelay,
};
use crate::filter::filter_json;
use crate::request::OracleRequest;

use neo_primitives::OracleResponseCode;
use neo_protocol::get_sign_data_vec;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTime};

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
    /// Main request processing loop.
    pub(crate) async fn process_requests_loop(self: Arc<Self>) {
        while !self.cancel.load(Ordering::SeqCst) {
            let snapshot = self.snapshots.snapshot();
            self.sync_pending_queue(&snapshot);

            let requests = snapshot.get_requests().unwrap_or_default();

            for (request_id, request) in requests {
                if self.cancel.load(Ordering::SeqCst) {
                    break;
                }

                if self.is_request_finished(request_id) {
                    continue;
                }

                // Check for duplicate requests
                if self.settings.enable_deduplication
                    && self.is_duplicate_request(request_id, &request.url)
                {
                    tracing::debug!(
                        target: "neo::oracle",
                        request_id,
                        url = %request.url,
                        "Skipping duplicate request"
                    );
                    continue;
                }

                // Validate URL before processing
                if let Err(err) = self.validate_url(&request.url) {
                    tracing::warn!(
                        target: "neo::oracle",
                        request_id,
                        url = %request.url,
                        error = %err,
                        "URL validation failed"
                    );
                    // Clean up in-flight on error
                    self.cleanup_in_flight(&request.url);
                    continue;
                }

                let should_process = {
                    let queue = self.pending_queue.lock();
                    match queue.get(&request_id) {
                        Some(task) => task.tx.is_none(),
                        None => true,
                    }
                };

                if should_process {
                    let url = request.url.clone();
                    if let Err(err) = self.process_request(&snapshot, request_id, request).await {
                        self.handle_error(&err);
                        // Clean up in-flight on error
                        self.cleanup_in_flight(&url);
                    } else {
                        // Mark as completed on success
                        self.mark_request_completed(request_id, &url);
                    }
                }
            }

            if self.cancel.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        self.status
            .store(OracleStatus::Stopped.as_u8(), Ordering::SeqCst);
    }

    /// Timer loop for cache cleanup and signature resending.
    pub(crate) async fn timer_loop(self: Arc<Self>) {
        let mut interval = tokio::time::interval(REFRESH_INTERVAL);
        loop {
            interval.tick().await;
            if self.cancel.load(Ordering::SeqCst) {
                break;
            }

            let now = SystemTime::now();
            let mut expired_requests = Vec::new();
            let mut resend = Vec::new();
            let mut send_tasks = Vec::new();

            {
                let queue = self.pending_queue.lock();
                let wallet = self.wallet.read().clone();
                if let Some(wallet) = wallet {
                    for (request_id, task) in queue.iter() {
                        if let Ok(span) = now.duration_since(task.timestamp) {
                            if span > self.settings.max_task_timeout {
                                expired_requests.push(*request_id);
                                continue;
                            }
                            if span > REFRESH_INTERVAL {
                                let oracles: Vec<_> = task.backup_signs.keys().cloned().collect();
                                for (account, pubkey) in wallet.oracle_accounts(&oracles) {
                                    if let Some(sign) = task.backup_signs.get(&pubkey) {
                                        resend.push((*request_id, sign.clone(), account, pubkey));
                                    }
                                }
                            }
                        }
                    }
                }
            }

            for (request_id, sign, account, pubkey) in resend {
                match self
                    .send_response_signature(request_id, sign, account, pubkey)
                    .await
                {
                    Ok(task) => send_tasks.push(task),
                    Err(error) => tracing::warn!(
                        target: "neo::oracle",
                        request_id,
                        %error,
                        "failed to authorize refreshed oracle response signature"
                    ),
                }
            }
            if !send_tasks.is_empty() {
                futures::future::join_all(send_tasks).await;
            }

            if !expired_requests.is_empty() {
                let mut queue = self.pending_queue.lock();
                for request_id in expired_requests {
                    queue.remove(&request_id);
                }
            }

            self.cleanup_finished_cache(now);

            // Periodically clean up stale in-flight requests (safety measure)
            if self.settings.enable_deduplication {
                self.cleanup_stale_in_flight(now);
            }
        }
    }

    /// Cleans up stale in-flight requests.
    fn cleanup_stale_in_flight(&self, _now: SystemTime) {
        let in_flight_count = self.in_flight_count();
        let dedup_cache_size = self.dedup_cache_size();

        if in_flight_count > 0 || dedup_cache_size > 0 {
            tracing::debug!(
                target: "neo::oracle",
                in_flight = in_flight_count,
                dedup_cache = dedup_cache_size,
                "Deduplication cache status"
            );
        }
    }

    /// Drops pending-queue entries whose request is no longer on-chain.
    pub(crate) fn sync_pending_queue(&self, snapshot: &S::Snapshot) {
        let offchain = snapshot
            .get_requests()
            .unwrap_or_default()
            .into_iter()
            .map(|(id, _)| id)
            .collect::<HashSet<_>>();

        let mut queue = self.pending_queue.lock();
        queue.retain(|id, _| offchain.contains(id));
    }

    /// Checks if a request has already been responded to.
    pub fn is_request_finished(&self, request_id: u64) -> bool {
        self.finished_cache.lock().contains(&request_id)
    }

    /// Prunes expired entries from the finished-request cache.
    pub fn cleanup_finished_cache(&self, now: SystemTime) {
        self.finished_cache
            .lock()
            .prune_expired(now, crate::dedup::ExpiryBoundary::Inclusive);
    }

    /// Fetches `url` and returns the oracle response code and body.
    ///
    /// Faithful port of the reference `process_url`: length and emptiness are
    /// checked first, `neofs:` requests fail closed (both by prefix and by
    /// parsed scheme), only `https` is dispatched to the HTTP client under a
    /// hard 60-second ceiling, and every other scheme yields
    /// [`OracleResponseCode::ProtocolNotSupported`].
    pub async fn process_url(&self, url: &str) -> (OracleResponseCode, String) {
        // Validate URL length
        if url.len() > MAX_URL_LENGTH {
            tracing::warn!(
                target: "neo::oracle",
                url_length = url.len(),
                max_length = MAX_URL_LENGTH,
                "URL too long"
            );
            return (
                OracleResponseCode::Error,
                format!("URL too long: {} > {}", url.len(), MAX_URL_LENGTH),
            );
        }

        // Validate URL is not empty
        if url.is_empty() {
            return (OracleResponseCode::Error, "Empty URL".to_string());
        }

        // Handle NeoFS protocol
        if url.len() >= 6 && url[..6].eq_ignore_ascii_case("neofs:") {
            tracing::warn!(
                target: "neo::oracle",
                "NeoFS oracle requests are fail-closed until dedicated signer algorithms are migrated"
            );
            return (
                OracleResponseCode::ProtocolNotSupported,
                format!("Invalid Protocol:<{url}>"),
            );
        }

        // Parse and validate URL
        let parsed = match url::Url::parse(url) {
            Ok(uri) => uri,
            Err(e) => {
                tracing::warn!(
                    target: "neo::oracle",
                    url = %url,
                    error = %e,
                    "Failed to parse URL"
                );
                return (OracleResponseCode::Error, format!("Invalid url:<{url}>"));
            }
        };

        // Validate scheme
        let scheme = parsed.scheme();
        if scheme.eq_ignore_ascii_case("https") {
            // Apply timeout with a maximum ceiling
            let timeout = std::cmp::min(self.settings.max_oracle_timeout, Duration::from_secs(60));
            let fut = self.https.process(&self.settings, parsed);
            return match tokio::time::timeout(timeout, fut).await {
                Ok(result) => result,
                Err(_) => {
                    tracing::warn!(
                        target: "neo::oracle",
                        url = %url,
                        timeout_secs = timeout.as_secs(),
                        "Oracle request timed out"
                    );
                    (OracleResponseCode::Timeout, String::new())
                }
            };
        }

        if scheme.eq_ignore_ascii_case("neofs") {
            tracing::warn!(
                target: "neo::oracle",
                "NeoFS oracle requests are fail-closed until dedicated signer algorithms are migrated"
            );
            return (
                OracleResponseCode::ProtocolNotSupported,
                format!("Invalid Protocol:<{url}>"),
            );
        }

        (
            OracleResponseCode::ProtocolNotSupported,
            format!("Invalid Protocol:<{url}>"),
        )
    }

    /// Processes a single oracle request.
    pub(crate) async fn process_request(
        &self,
        snapshot: &S::Snapshot,
        request_id: u64,
        request: OracleRequest,
    ) -> Result<(), OracleServiceError> {
        tracing::debug!(
            target: "neo::oracle",
            request_id,
            url = %request.url,
            "processing oracle request"
        );

        let height = snapshot.ledger_height();
        let oracle_nodes = snapshot
            .designated_oracles(height)
            .map_err(|err| OracleServiceError::Processing(err.to_string()))?;
        if oracle_nodes.is_empty() {
            return Err(OracleServiceError::Processing(
                "oracle nodes not designated".to_string(),
            ));
        }

        let (mut code, data) = self.process_url(&request.url).await;
        let response_pairs = snapshot
            .get_requests_by_url(&request.url)
            .map_err(|err| OracleServiceError::Processing(err.to_string()))?;

        let mut tasks = Vec::new();
        for (pending_id, pending_request) in response_pairs {
            let mut response_code = code;
            let mut response_result = Vec::new();

            if response_code == OracleResponseCode::Success {
                match filter_json(&data, pending_request.filter.as_deref()) {
                    Ok(result) => response_result = result,
                    Err(err) => {
                        response_code = OracleResponseCode::Error;
                        code = OracleResponseCode::Error;
                        tracing::warn!(
                            target: "neo::oracle",
                            request_id,
                            filter = ?pending_request.filter,
                            error = %err,
                            "oracle filter failed"
                        );
                    }
                }
            }

            // The builder reports the effective code, which may have been
            // downgraded to `ResponseTooLarge` or `InsufficientFunds`.
            let (response_tx, response_code) = self.tx_builder.build_response_tx(
                snapshot,
                &pending_request,
                pending_id,
                response_code,
                response_result,
                &oracle_nodes,
                self.settings.network,
                false,
            )?;

            let (backup_tx, _) = self.tx_builder.build_response_tx(
                snapshot,
                &pending_request,
                pending_id,
                OracleResponseCode::ConsensusUnreachable,
                Vec::new(),
                &oracle_nodes,
                self.settings.network,
                true,
            )?;

            tracing::debug!(
                target: "neo::oracle",
                request_id,
                pending_id,
                response_hash = %response_tx.hash(),
                backup_hash = %backup_tx.hash(),
                code = ?response_code,
                "oracle response transactions built"
            );

            let wallet = self.wallet.read().clone().ok_or_else(|| {
                OracleServiceError::Processing("wallet not available".to_string())
            })?;
            let accounts = wallet.oracle_accounts(&oracle_nodes);
            if accounts.is_empty() {
                return Err(OracleServiceError::Processing(
                    "wallet has no unlocked designated oracle account".to_string(),
                ));
            }
            let tx_data = get_sign_data_vec(&response_tx, self.settings.network)
                .map_err(|error| OracleServiceError::Processing(error.to_string()))?;
            let backup_data = get_sign_data_vec(&backup_tx, self.settings.network)
                .map_err(|error| OracleServiceError::Processing(error.to_string()))?;

            for (account, oracle_pub) in accounts {
                let tx_operation = format!(
                    "oracle-tx:{}:{}:{}:{}",
                    self.settings.network,
                    pending_id,
                    account,
                    response_tx.hash(),
                );
                let tx_sign = self
                    .sign_neo_payload(
                        tx_operation,
                        account,
                        tx_data.clone(),
                        "neo-n3-oracle-response-transaction",
                        std::collections::BTreeMap::from([
                            ("request_id".to_string(), pending_id.to_string()),
                            (
                                "transaction_hash".to_string(),
                                response_tx.hash().to_string(),
                            ),
                            ("backup".to_string(), "false".to_string()),
                        ]),
                    )
                    .await?;
                let backup_operation = format!(
                    "oracle-tx:{}:{}:{}:{}",
                    self.settings.network,
                    pending_id,
                    account,
                    backup_tx.hash(),
                );
                let backup_sign = self
                    .sign_neo_payload(
                        backup_operation,
                        account,
                        backup_data.clone(),
                        "neo-n3-oracle-response-transaction",
                        std::collections::BTreeMap::from([
                            ("request_id".to_string(), pending_id.to_string()),
                            ("transaction_hash".to_string(), backup_tx.hash().to_string()),
                            ("backup".to_string(), "true".to_string()),
                        ]),
                    )
                    .await?;

                self.add_response_tx_sign(
                    snapshot,
                    pending_id,
                    oracle_pub.clone(),
                    tx_sign.clone(),
                    Some(response_tx.clone()),
                    Some(backup_tx.clone()),
                    Some(backup_sign),
                )?;

                tasks.push(
                    self.send_response_signature(pending_id, tx_sign, account, oracle_pub)
                        .await?,
                );
            }
        }

        if !tasks.is_empty() {
            futures::future::join_all(tasks).await;
        }

        tracing::debug!(target: "neo::oracle", request_id, "oracle request processed");
        Ok(())
    }
}
