//! ApplicationLogs service for capturing execution logs and serving RPC queries.

use neo_error::{CoreError, CoreResult};
use neo_payloads::ApplicationExecuted;
use neo_payloads::Block;
use neo_primitives::UInt256;
use neo_primitives::panic_message;
use neo_runtime::{CommittedHandler, CommittingHandler};
use neo_storage::persistence::providers::MemoryStore;
use neo_storage::persistence::{DataCache, ReadOnlyStoreGeneric, Store, StoreSnapshot, WriteStore};
use parking_lot::Mutex;
use serde_json::Value;
use std::any::Any;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::error;

use super::ApplicationLogsSettings;

/// ApplicationLogs storage and commit handler.
pub struct ApplicationLogsService<S: Store = MemoryStore> {
    settings: ApplicationLogsSettings,
    store: Arc<S>,
    snapshot: Mutex<Option<Arc<S::Snapshot>>>,
    disabled: AtomicBool,
}

impl<S> ApplicationLogsService<S>
where
    S: Store,
{
    const PREFIX_BLOCK: u8 = 0x40;
    const PREFIX_TX: u8 = 0x41;

    /// Creates a new ApplicationLogs service.
    pub fn new(settings: ApplicationLogsSettings, store: Arc<S>) -> Self {
        Self {
            settings,
            store,
            snapshot: Mutex::new(None),
            disabled: AtomicBool::new(false),
        }
    }

    /// Returns the settings in use.
    pub fn settings(&self) -> &ApplicationLogsSettings {
        &self.settings
    }

    /// Returns the stored block log JSON, if present.
    pub fn get_block_log(&self, hash: &UInt256) -> Option<Value> {
        self.read_log(Self::PREFIX_BLOCK, hash)
    }

    /// Returns the stored transaction log JSON, if present.
    pub fn get_transaction_log(&self, hash: &UInt256) -> Option<Value> {
        self.read_log(Self::PREFIX_TX, hash)
    }

    fn start_batch(&self) {
        let mut guard = self.snapshot.lock();
        *guard = Some(self.store.snapshot());
    }

    fn commit_batch(&self) -> CoreResult<()> {
        let mut guard = self.snapshot.lock();
        let Some(snapshot_arc) = guard.as_mut() else {
            return Ok(());
        };
        let Some(snapshot) = Arc::get_mut(snapshot_arc) else {
            return Err(CoreError::other(
                "application logs commit failed: snapshot is still shared",
            ));
        };
        snapshot
            .try_commit()
            .map_err(|err| CoreError::other(format!("application logs commit failed: {err}")))
    }

    fn handle_panic(&self, payload: Box<dyn Any + Send>, phase: &'static str) {
        error!(
            target: "neo::application_logs",
            phase,
            error = panic_message(payload.as_ref(), "unknown panic payload"),
            "application logs handler panicked"
        );
        self.settings
            .exception_policy
            .apply(|| self.disabled.store(true, Ordering::SeqCst));
    }

    fn handle_error(&self, err: &str, phase: &'static str) {
        error!(
            target: "neo::application_logs",
            phase,
            error = err,
            "application logs handler failed"
        );
        self.settings
            .exception_policy
            .apply(|| self.disabled.store(true, Ordering::SeqCst));
    }

    fn write_log(&self, prefix: u8, hash: &UInt256, value: Value) -> CoreResult<()> {
        let mut guard = self.snapshot.lock();
        let Some(snapshot_arc) = guard.as_mut() else {
            return Ok(());
        };
        let Some(snapshot) = Arc::get_mut(snapshot_arc) else {
            return Err(CoreError::other(
                "application logs write failed: snapshot is still shared",
            ));
        };
        let mut key = Vec::with_capacity(1 + 32);
        key.push(prefix);
        key.extend_from_slice(&hash.to_bytes());
        let bytes = serde_json::to_vec(&value).map_err(|err| {
            CoreError::other(format!("failed to serialize application log: {err}"))
        })?;
        snapshot.put(key, bytes).map_err(|err| {
            CoreError::other(format!("failed to write application log to storage: {err}"))
        })?;
        Ok(())
    }

    fn read_log(&self, prefix: u8, hash: &UInt256) -> Option<Value> {
        let mut key = Vec::with_capacity(1 + 32);
        key.push(prefix);
        key.extend_from_slice(&hash.to_bytes());
        let snapshot = self.store.snapshot();
        let raw = snapshot.try_get(&key)?;
        serde_json::from_slice(&raw).ok()
    }

    fn block_log_json(&self, block_hash: &UInt256, executions: &[ApplicationExecuted]) -> Value {
        super::rendering::block_log_json(
            block_hash,
            executions,
            self.settings.debug,
            self.settings.max_stack_size,
        )
    }

    fn transaction_log_json(&self, tx_hash: &UInt256, exec: &ApplicationExecuted) -> Value {
        super::rendering::transaction_log_json(
            tx_hash,
            exec,
            self.settings.debug,
            self.settings.max_stack_size,
        )
    }
}

impl<S> CommittingHandler for ApplicationLogsService<S>
where
    S: Store,
{
    fn blockchain_committing_handler<B: neo_storage::CacheRead>(
        &self,
        network: u32,
        block: &Block,
        _snapshot: &DataCache<B>,
        application_executed_list: &[ApplicationExecuted],
    ) {
        if self.disabled.load(Ordering::Relaxed) {
            return;
        }
        if network != self.settings.network {
            return;
        }
        let result = panic::catch_unwind(AssertUnwindSafe(|| -> CoreResult<()> {
            self.start_batch();

            let block_hash = block.hash();
            let block_log = self.block_log_json(&block_hash, application_executed_list);
            self.write_log(Self::PREFIX_BLOCK, &block_hash, block_log)?;

            for exec in application_executed_list {
                let Some(tx) = exec.transaction.as_ref() else {
                    continue;
                };
                let tx_hash = tx.hash();
                let tx_log = self.transaction_log_json(&tx_hash, exec);
                self.write_log(Self::PREFIX_TX, &tx_hash, tx_log)?;
            }
            Ok(())
        }));
        match result {
            Ok(Ok(())) => {}
            Ok(Err(err)) => self.handle_error(&err.to_string(), "committing"),
            Err(payload) => self.handle_panic(payload, "committing"),
        }
    }
}

impl<S> CommittedHandler for ApplicationLogsService<S>
where
    S: Store,
{
    fn blockchain_committed_handler(&self, network: u32, _block: &Block) {
        if self.disabled.load(Ordering::Relaxed) {
            return;
        }
        if network != self.settings.network {
            return;
        }
        let result = panic::catch_unwind(AssertUnwindSafe(|| self.commit_batch()));
        match result {
            Ok(Ok(())) => {}
            Ok(Err(err)) => self.handle_error(&err.to_string(), "committed"),
            Err(payload) => self.handle_panic(payload, "committed"),
        }
    }
}

#[cfg(test)]
#[path = "../tests/application_logs/service.rs"]
mod tests;
