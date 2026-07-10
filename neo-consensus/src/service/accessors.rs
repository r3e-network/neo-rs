use crate::context::{ConsensusContext, ValidatorInfo};
use crate::{ConsensusError, ConsensusResult, ConsensusSigner};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::ConsensusService;

impl<S> ConsensusService<S>
where
    S: ConsensusSigner,
{
    /// Returns our validator index, or an error if we're not a validator.
    /// This is a safe alternative to directly unwrapping `my_index` in
    /// production code.
    #[inline]
    pub(super) fn my_index(&self) -> ConsensusResult<u8> {
        self.context.my_index.ok_or(ConsensusError::NotValidator)
    }

    /// Returns the current context (for testing/debugging)
    #[must_use]
    pub const fn context(&self) -> &ConsensusContext {
        &self.context
    }

    /// Updates the validator set and local validator index.
    pub fn update_validators(&mut self, validators: Vec<ValidatorInfo>, my_index: Option<u8>) {
        self.context.validators = validators;
        self.context.my_index = my_index;
    }

    /// Sets the expected block time (in milliseconds) for timeout calculations.
    pub fn set_expected_block_time(&mut self, expected_block_time_ms: u64) {
        self.context.expected_block_time = expected_block_time_ms;
    }

    /// Sets the protocol `MaxTransactionsPerBlock` consensus limit.
    pub fn set_max_transactions_per_block(&mut self, max_transactions_per_block: u32) {
        self.max_transactions_per_block = max_transactions_per_block;
    }

    /// Returns the configured protocol `MaxTransactionsPerBlock` consensus limit.
    #[must_use]
    pub const fn max_transactions_per_block(&self) -> u32 {
        self.max_transactions_per_block
    }

    /// Sets the block-size / block-system-fee policy limits a backup enforces in
    /// `CheckPrepareResponse` before sending its `PrepareResponse`
    /// (C# `DbftSettings.MaxBlockSize` / `DbftSettings.MaxBlockSystemFee`).
    pub fn set_max_block_policy(&mut self, max_block_size: u32, max_block_system_fee: i64) {
        self.context
            .set_max_block_policy(max_block_size, max_block_system_fee);
    }

    /// Records the wire size and system fee of a proposal transaction whose body
    /// the node has cached, so the backup can compute the expected block size /
    /// system fee for its `CheckPrepareResponse` policy checks. Mirrors C#
    /// `ConsensusContext.Transactions[hash] = tx`, but keeps the consensus crate
    /// hash-only by carrying just the two policy-relevant metrics.
    pub fn record_transaction_metrics(
        &mut self,
        hash: neo_primitives::UInt256,
        size: usize,
        system_fee: i64,
    ) {
        self.context
            .record_transaction_metrics(hash, crate::context::TxMetrics { size, system_fee });
    }

    /// Updates the private key used for signing consensus messages.
    /// The key is wrapped in `Zeroizing` so it is wiped from memory on drop.
    pub fn set_private_key(&mut self, private_key: Vec<u8>) {
        self.private_key = zeroize::Zeroizing::new(private_key);
    }

    /// Updates the signer used for consensus messages.
    pub fn set_signer(&mut self, signer: Option<Arc<S>>) {
        self.signer = signer;
    }

    /// Persists the current consensus context to disk for recovery.
    pub fn save_context(&self, path: &Path) -> ConsensusResult<()> {
        self.context.save(path)
    }

    /// Sets the recovery-log file path used for crash-recovery persistence.
    ///
    /// When set, the context is saved immediately before this node signs and
    /// broadcasts its own Commit and is reloaded on startup — mirroring C#
    /// `ConsensusContext.Save`/`Load` guarded by `DbftSettings.RecoveryLogs`.
    /// Passing `None` disables persistence (C# `IgnoreRecoveryLogs = true`).
    pub fn set_state_path(&mut self, path: Option<PathBuf>) {
        self.state_path = path;
    }

    /// Persists the consensus context to the configured recovery-log path, if any.
    ///
    /// This is the Rust analogue of C# `ConsensusContext.Save()`. It is called
    /// from `check_prepare_responses` exactly once — immediately before this node
    /// broadcasts its own Commit — so a crash/restart resumes from a state that
    /// already records the signed commit and cannot equivocate at the same
    /// (height, view). A persistence failure here propagates so the Commit is
    /// NOT broadcast (C# `store.PutSync` throwing aborts the same code path
    /// before `localNode.Tell`).
    pub(in crate::service) fn save_context_if_configured(&self) -> ConsensusResult<()> {
        if let Some(path) = self.state_path.as_deref() {
            self.context.save(path)?;
        }
        Ok(())
    }

    /// Returns the network magic number this service is configured for.
    #[must_use]
    pub const fn network(&self) -> u32 {
        self.network
    }

    /// Returns whether the service is running
    #[must_use]
    pub const fn is_running(&self) -> bool {
        self.running
    }
}
