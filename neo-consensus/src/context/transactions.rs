//! Proposal transaction availability and block-policy accounting.
//!
//! The node owns full transaction bodies; consensus tracks only hashes plus the
//! wire-size/system-fee metrics needed to mirror C# DBFT block-policy checks.

use neo_primitives::UInt256;

use super::ConsensusContext;

/// Wire size and system fee of a single proposal transaction, mirroring the
/// C# `Transaction.Size` / `Transaction.SystemFee` values `ConsensusContext`
/// reads when computing the expected block size and system fee.
#[derive(Debug, Clone, Copy)]
pub struct TxMetrics {
    /// Serialized transaction size in bytes (C# `Transaction.Size`).
    pub size: usize,
    /// Transaction system fee (C# `Transaction.SystemFee`).
    pub system_fee: i64,
}

impl ConsensusContext {
    /// Records which proposed transactions are locally available.
    ///
    /// This REPLACES the availability set with the intersection of `tx_hashes`
    /// and the proposal. Used for the one-shot snapshot fill at PrepareRequest
    /// time (C# `OnPrepareRequestReceived` bulk mempool scan). For an
    /// incrementally-arriving single transaction (C# `OnTransaction`), use
    /// [`mark_transaction_available`] instead, which is additive.
    pub fn mark_available_transactions<I>(&mut self, tx_hashes: I)
    where
        I: IntoIterator<Item = UInt256>,
    {
        self.available_tx_hashes.clear();
        for hash in tx_hashes {
            if self.proposed_tx_hashes.contains(&hash) {
                self.available_tx_hashes.insert(hash);
            }
        }
    }

    /// Additively records that a single proposed transaction is now locally
    /// available (C# `ConsensusService.AddTransaction` populating
    /// `context.Transactions[tx.Hash]`). Unlike [`mark_available_transactions`]
    /// this never clears prior availability, so a late-arriving transaction
    /// (C# `OnTransaction`) accumulates toward completeness instead of resetting
    /// it. Returns `true` if `hash` belongs to the current proposal and was not
    /// already recorded (i.e. this call made progress).
    pub fn mark_transaction_available(&mut self, hash: UInt256) -> bool {
        if !self.proposed_tx_hashes.contains(&hash) {
            return false;
        }
        self.available_tx_hashes.insert(hash)
    }

    /// Returns true when `hash` is one of the current proposal's transactions.
    #[must_use]
    pub fn is_proposed_transaction(&self, hash: &UInt256) -> bool {
        self.proposed_tx_hashes.contains(hash)
    }

    /// Returns true when the proposed transaction `hash` is already recorded as
    /// locally available for this view (C# `context.Transactions.ContainsKey`).
    #[must_use]
    pub fn has_available_transaction(&self, hash: &UInt256) -> bool {
        self.available_tx_hashes.contains(hash)
    }

    /// Returns true when a proposal references transactions this node has not received.
    #[must_use]
    pub fn has_missing_proposed_transactions(&self) -> bool {
        !self.proposed_tx_hashes.is_empty()
            && self.proposed_tx_hashes.len() > self.available_tx_hashes.len()
    }

    /// Sets the block-size / block-system-fee policy limits enforced by a backup
    /// before it sends its `PrepareResponse` (C# `DbftSettings.MaxBlockSize` /
    /// `DbftSettings.MaxBlockSystemFee`). Called by the node when it configures a
    /// consensus round so both limits track the same source the primary uses in
    /// `EnsureMaxBlockLimitation`.
    pub fn set_max_block_policy(&mut self, max_block_size: u32, max_block_system_fee: i64) {
        self.max_block_size = max_block_size;
        self.max_block_system_fee = max_block_system_fee;
    }

    /// Records the wire size and system fee of a proposal transaction whose body
    /// the node has cached (C# `ConsensusContext.Transactions[hash] = tx`). Only
    /// transactions that belong to the current proposal are retained, so the
    /// expected-block computations stay scoped to `TransactionHashes`.
    pub fn record_transaction_metrics(&mut self, hash: UInt256, metrics: TxMetrics) {
        if self.proposed_tx_hashes.contains(&hash) {
            self.available_tx_metrics.insert(hash, metrics);
        }
    }

    /// Expected serialized block size, mirroring C#
    /// `ConsensusContext.GetExpectedBlockSize()`:
    /// `GetExpectedBlockSizeWithoutTransactions(Transactions.Count) + Σ tx.Size`.
    ///
    /// The base (transaction-free) size counts the fixed header fields, the
    /// M-of-N block witness (verification script + an invocation script pushing
    /// `M` 64-byte signatures — C# `_witnessSize`), and the transaction-count
    /// var-int. Only transactions whose metrics have been recorded contribute to
    /// the per-tx sum; the caller must ensure the full proposal is available
    /// (`!has_missing_proposed_transactions`) before treating the result as the
    /// final block size, exactly as C# only reaches this check once
    /// `TransactionHashes.Length == Transactions.Count`.
    #[must_use]
    pub fn expected_block_size(&self) -> usize {
        let tx_count = self.available_tx_metrics.len();
        let base = self.expected_block_size_without_transactions(tx_count);
        let tx_bytes: usize = self.available_tx_metrics.values().map(|m| m.size).sum();
        base.saturating_add(tx_bytes)
    }

    /// Expected block system fee, mirroring C#
    /// `ConsensusContext.GetExpectedBlockSystemFee()`: `Σ tx.SystemFee`.
    #[must_use]
    pub fn expected_block_system_fee(&self) -> i64 {
        self.available_tx_metrics
            .values()
            .map(|m| m.system_fee)
            .fold(0i64, i64::saturating_add)
    }

    /// C# `ConsensusContext.GetExpectedBlockSizeWithoutTransactions`: the fixed
    /// header + witness + tx-count var-int size, independent of the transaction
    /// bodies. The witness matches C# `_witnessSize` — an `M`-of-`N` multi-sig
    /// verification script plus an invocation script that pushes `M` 64-byte
    /// commit signatures.
    #[must_use]
    fn expected_block_size_without_transactions(&self, expected_transactions: usize) -> usize {
        use neo_io::serializable::helper::SerializeHelper;
        use neo_payloads::Witness;
        use neo_vm::script_builder::{RedeemScript, ScriptBuilder};

        // Witness verification script: the canonical M-of-N multi-sig over the
        // sorted validator keys (C# `Contract.CreateMultiSigRedeemScript`).
        let n = self.validators.len();
        let m = RedeemScript::bft_threshold(n);
        let mut sorted: Vec<neo_crypto::ECPoint> = self
            .validators
            .iter()
            .map(|v| v.public_key.clone())
            .collect();
        sorted.sort();
        let mut verification = ScriptBuilder::new();
        verification.emit_push_int(m as i64);
        for key in &sorted {
            verification.emit_push(key.as_bytes());
        }
        verification.emit_push_int(n as i64);
        verification
            .emit_syscall("System.Crypto.CheckMultisig")
            .expect("infallible: in-memory emit");

        // Witness invocation script: M pushes of a 64-byte signature placeholder
        // (C# `_witnessSize` invocation: `for (x < M) sb.EmitPush(new byte[64])`).
        let mut invocation = ScriptBuilder::new();
        let signature_placeholder = [0u8; 64];
        for _ in 0..m {
            invocation.emit_push(&signature_placeholder);
        }

        let witness = Witness::new_with_scripts(invocation.to_array(), verification.to_array());

        // C# GetExpectedBlockSizeWithoutTransactions field layout.
        4               // Version (uint)
            + 32        // PrevHash (UInt256)
            + 32        // MerkleRoot (UInt256)
            + 8         // Timestamp (ulong)
            + 8         // Nonce (ulong)
            + 4         // Index (uint)
            + 1         // PrimaryIndex (byte)
            + 20        // NextConsensus (UInt160)
            + 1         // Witness array length prefix (1 witness)
            + witness.size()
            + SerializeHelper::get_var_size_usize(expected_transactions)
    }
}
