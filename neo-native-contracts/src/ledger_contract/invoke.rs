//! Ledger native-method handlers.
//!
//! Keeps read-only ledger query bodies out of the contract root while preserving
//! trace-window checks, conflict-stub filtering, and deterministic public return
//! encoders. Dispatch is declared by the metadata binding table and
//! `native_contract_dispatch!`.

use super::LedgerContract;
use neo_error::{CoreError, CoreResult};
use neo_execution::ApplicationEngine;
use neo_vm_rs::VmState as VMState;
use num_bigint::BigInt;

impl LedgerContract {
    pub(super) fn invoke_current_index(
        &self,
        engine: &mut ApplicationEngine,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        Ok(BigInt::from(self.current_index(&snapshot)?).to_signed_bytes_le())
    }

    pub(super) fn invoke_current_hash(
        &self,
        engine: &mut ApplicationEngine,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        Ok(self.current_hash(&snapshot)?.to_bytes())
    }

    pub(super) fn invoke_get_transaction_height(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        // All wired methods are read-only queries over persisted ledger state,
        // served from the engine's snapshot (C# `RequiredCallFlags = ReadStates`).
        let snapshot = engine.snapshot_cache();
        let hash = crate::args::raw_hash256(args, 0, "LedgerContract::getTransactionHeight")?;
        // C# `GetTransactionState` returns null for a conflict stub (its
        // `Transaction` is null), and `getTransactionHeight` returns -1 for
        // an absent or untraceable transaction; otherwise `(int)BlockIndex`.
        let height = match self.get_transaction_state(&snapshot, &hash)? {
            Some(state)
                if state.transaction.is_some()
                    && self.is_traceable_block(engine, state.block_index)? =>
            {
                i64::from(state.block_index as i32)
            }
            _ => -1,
        };
        Ok(BigInt::from(height).to_signed_bytes_le())
    }

    pub(super) fn invoke_get_transaction_vm_state(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        let hash = crate::args::raw_hash256(args, 0, "LedgerContract::getTransactionVMState")?;
        // C# returns VMState.NONE for an absent, conflict-stub, or
        // untraceable transaction; otherwise the recorded execution state.
        let vm_state = match self.get_transaction_state(&snapshot, &hash)? {
            Some(state) if state.transaction.is_some() => {
                if self.is_traceable_block(engine, state.block_index)? {
                    state.state.to_byte()
                } else {
                    VMState::NONE.to_byte()
                }
            }
            _ => VMState::NONE.to_byte(),
        };
        Ok(BigInt::from(vm_state).to_signed_bytes_le())
    }

    pub(super) fn invoke_get_transaction(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        let hash = crate::args::raw_hash256(args, 0, "LedgerContract::getTransaction")?;
        // C# returns the transaction (Array via ToStackItem) for a
        // traceable full record; null (empty payload) for an absent,
        // conflict-stub, or untraceable transaction.
        match self.get_transaction_state(&snapshot, &hash)? {
            Some(state) => {
                if let Some(tx) = &state.transaction {
                    if self.is_traceable_block(engine, state.block_index)? {
                        Self::transaction_to_bytes(tx, "getTransaction")
                    } else {
                        Ok(Vec::new())
                    }
                } else {
                    Ok(Vec::new())
                }
            }
            None => Ok(Vec::new()),
        }
    }

    pub(super) fn invoke_get_transaction_signers(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        let hash = crate::args::raw_hash256(args, 0, "LedgerContract::getTransactionSigners")?;
        // C# returns the transaction's Signer[] (Array via ToStackItem) for
        // a traceable full record; null (empty payload) otherwise.
        match self.get_transaction_state(&snapshot, &hash)? {
            Some(state) => {
                if let Some(tx) = &state.transaction {
                    if self.is_traceable_block(engine, state.block_index)? {
                        Self::signers_to_bytes(tx.signers(), "getTransactionSigners")
                    } else {
                        Ok(Vec::new())
                    }
                } else {
                    Ok(Vec::new())
                }
            }
            None => Ok(Vec::new()),
        }
    }

    pub(super) fn invoke_get_block(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        let index_or_hash =
            crate::args::raw_arg(args, 0, "LedgerContract::getBlock").map_err(|_| {
                CoreError::invalid_operation("LedgerContract::getBlock requires an indexOrHash")
            })?;
        // C#: resolve the index/hash to a block hash, load the trimmed
        // block, and return it (Array via ToStackItem) only if traceable;
        // null (empty payload) for an absent or untraceable block.
        let Some(hash) = self.resolve_block_hash(&snapshot, index_or_hash)? else {
            return Ok(Vec::new());
        };
        match self.get_trimmed_block(&snapshot, &hash)? {
            Some(block) if self.is_traceable_block(engine, block.index())? => {
                Self::trimmed_block_to_bytes(&block, "getBlock")
            }
            _ => Ok(Vec::new()),
        }
    }

    pub(super) fn invoke_get_transaction_from_block(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        let index_or_hash =
            crate::args::raw_arg(args, 0, "LedgerContract::getTransactionFromBlock").map_err(
                |_| {
                    CoreError::invalid_operation(
                        "LedgerContract::getTransactionFromBlock requires a blockIndexOrHash",
                    )
                },
            )?;
        let tx_index_bytes =
            crate::args::raw_arg(args, 1, "LedgerContract::getTransactionFromBlock").map_err(
                |_| {
                    CoreError::invalid_operation(
                        "LedgerContract::getTransactionFromBlock requires a txIndex",
                    )
                },
            )?;
        let tx_index = crate::args::raw_integer_bytes_to_i32(
            tx_index_bytes,
            "LedgerContract::getTransactionFromBlock: txIndex",
        )
        .map_err(|_| {
            CoreError::invalid_operation(
                "LedgerContract::getTransactionFromBlock: txIndex out of int range",
            )
        })?;
        let Some(hash) = self.resolve_block_hash(&snapshot, index_or_hash)? else {
            return Ok(Vec::new());
        };
        // The block must exist and be traceable; otherwise null.
        let block = match self.get_trimmed_block(&snapshot, &hash)? {
            Some(block) if self.is_traceable_block(engine, block.index())? => block,
            _ => return Ok(Vec::new()),
        };
        // C# throws ArgumentOutOfRangeException for an out-of-range txIndex.
        if tx_index < 0 || tx_index as usize >= block.hashes.len() {
            return Err(CoreError::invalid_operation(format!(
                "LedgerContract::getTransactionFromBlock: txIndex {tx_index} out of range (len {})",
                block.hashes.len()
            )));
        }
        let tx_hash = block.hashes[tx_index as usize];
        // C# public GetTransaction(snapshot, hash): the transaction (no
        // extra traceability re-check, the block is already traceable),
        // or null for a conflict-stub/absent transaction.
        let tx = self
            .get_transaction_state(&snapshot, &tx_hash)?
            .and_then(|state| state.transaction);
        match tx {
            Some(tx) => Self::transaction_to_bytes(&tx, "getTransactionFromBlock"),
            None => Ok(Vec::new()),
        }
    }
}
