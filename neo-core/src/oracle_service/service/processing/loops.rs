use super::super::{OracleService, OracleStatus, REFRESH_INTERVAL};
use crate::smart_contract::native::OracleContract;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

impl OracleService {
    pub(in super::super) async fn process_requests_loop(self: Arc<Self>) {
        while !self.cancel.load(Ordering::SeqCst) {
            let snapshot = self.snapshot_cache();
            self.sync_pending_queue(&snapshot);

            let requests = OracleContract::new()
                .get_requests(&snapshot)
                .unwrap_or_default();

            for (request_id, request) in requests {
                if self.cancel.load(Ordering::SeqCst) {
                    break;
                }

                if self.is_request_finished(request_id) {
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
                    if let Err(err) = self.process_request(&snapshot, request_id, request).await {
                        self.handle_error(&err);
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

    pub(in super::super) async fn timer_loop(self: Arc<Self>) {
        let mut interval = tokio::time::interval(REFRESH_INTERVAL);
        loop {
            interval.tick().await;
            if self.cancel.load(Ordering::SeqCst) {
                break;
            }

            let now = SystemTime::now();
            let mut expired_requests = Vec::new();
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
                                for account in wallet.get_accounts() {
                                    if !account.has_key() || account.is_locked() {
                                        continue;
                                    }
                                    let Some(key) = account.get_key() else {
                                        continue;
                                    };
                                    let Ok(pubkey) = key.get_public_key_point() else {
                                        continue;
                                    };
                                    if let Some(sign) = task.backup_signs.get(&pubkey) {
                                        send_tasks.push(self.send_response_signature(
                                            *request_id,
                                            sign.clone(),
                                            key,
                                        ));
                                    }
                                }
                            }
                        }
                    }
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
        }
    }
}
