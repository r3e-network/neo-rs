use super::{
    storage::max_traceable_blocks_from_snapshot, HashOrIndex, LedgerContract,
    LedgerTransactionStates, PersistedTransactionState,
};
use crate::error::{CoreError as Error, CoreResult as Result};
use crate::hardfork::Hardfork;
use crate::network::p2p::payloads::transaction_attribute::TransactionAttribute;
use crate::persistence::i_read_only_store::IReadOnlyStoreGeneric;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::binary_serializer::BinarySerializer;
use crate::smart_contract::native::{
    policy_contract::PolicyContract, NativeContract, NativeMethod,
};
use crate::smart_contract::{IInteroperable, StorageItem, StorageKey};
use crate::vm_runtime::StackItem;
use crate::{UInt160, UInt256};
use neo_vm_rs::ExecutionEngineLimits;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

impl NativeContract for LedgerContract {
    fn id(&self) -> i32 {
        self.id
    }

    fn name(&self) -> &str {
        "LedgerContract"
    }

    fn hash(&self) -> UInt160 {
        self.hash
    }

    fn methods(&self) -> &[NativeMethod] {
        &self.methods
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        let snapshot_arc = engine.snapshot_cache();
        let snapshot = snapshot_arc.as_ref();
        let current_index = self.current_index(snapshot)?;
        let max_traceable_blocks = self.resolve_max_traceable_blocks(engine, snapshot);

        match method {
            "currentHash" => {
                if !args.is_empty() {
                    return Err(Error::invalid_argument(
                        "currentHash requires no arguments".to_string(),
                    ));
                }
                let hash = self.current_hash(snapshot)?;
                Ok(hash.to_bytes().to_vec())
            }
            "currentIndex" => {
                if !args.is_empty() {
                    return Err(Error::invalid_argument(
                        "currentIndex requires no arguments".to_string(),
                    ));
                }
                let index = current_index;
                Ok(index.to_le_bytes().to_vec())
            }
            "getBlock" => {
                if args.len() != 1 {
                    return Err(Error::invalid_argument(
                        "getBlock requires 1 argument".to_string(),
                    ));
                }
                let selector = &args[0];
                let target = self.parse_index_or_hash(selector, "indexOrHash")?;

                let maybe_trimmed = match &target {
                    HashOrIndex::Hash(hash) => self.get_trimmed_block(snapshot, hash)?,
                    HashOrIndex::Index(index) => {
                        if let Some(hash) = self.load_block_hash(snapshot, *index)? {
                            self.get_trimmed_block(snapshot, &hash)?
                        } else {
                            None
                        }
                    }
                };

                let item = match maybe_trimmed {
                    Some(trimmed)
                        if Self::is_traceable_block(
                            current_index,
                            trimmed.index(),
                            max_traceable_blocks,
                        ) =>
                    {
                        trimmed.to_stack_item()?
                    }
                    _ => StackItem::null(),
                };
                Self::serialize_stack_item(item)
            }
            "getTransaction" => {
                if args.len() != 1 {
                    return Err(Error::invalid_argument(
                        "getTransaction requires 1 argument".to_string(),
                    ));
                }
                let hash = UInt256::from_bytes(&args[0]).map_err(|e| {
                    Error::invalid_argument(format!("Invalid transaction hash: {e}"))
                })?;
                let item = if let Some(state) = self.get_transaction_state_if_traceable(
                    snapshot,
                    &hash,
                    current_index,
                    max_traceable_blocks,
                )? {
                    state.transaction().to_stack_item()?
                } else {
                    StackItem::null()
                };
                Self::serialize_stack_item(item)
            }
            "getTransactionFromBlock" => {
                if args.len() != 2 {
                    return Err(Error::invalid_argument(
                        "getTransactionFromBlock requires 2 arguments".to_string(),
                    ));
                }
                let target = self.parse_index_or_hash(&args[0], "blockIndexOrHash")?;
                let tx_index =
                    BigInt::from_signed_bytes_le(&args[1])
                        .to_i32()
                        .ok_or_else(|| {
                            Error::invalid_argument("Invalid transaction index".to_string())
                        })?;
                if tx_index < 0 {
                    return Err(Error::invalid_argument(
                        "Transaction index out of range".to_string(),
                    ));
                }

                let block_hash = match target {
                    HashOrIndex::Hash(hash) => Some(hash),
                    HashOrIndex::Index(index) => self.load_block_hash(snapshot, index)?,
                };

                let item = if let Some(block_hash) = block_hash {
                    if let Some(tx) = self.get_transaction_from_block(
                        snapshot,
                        &block_hash,
                        tx_index as u32,
                        current_index,
                        max_traceable_blocks,
                    )? {
                        tx.transaction().to_stack_item()?
                    } else {
                        StackItem::null()
                    }
                } else {
                    StackItem::null()
                };

                Self::serialize_stack_item(item)
            }
            "getTransactionHeight" => {
                if args.len() != 1 {
                    return Err(Error::invalid_argument(
                        "getTransactionHeight requires 1 argument".to_string(),
                    ));
                }
                let hash = UInt256::from_bytes(&args[0]).map_err(|e| {
                    Error::invalid_argument(format!("Invalid transaction hash: {e}"))
                })?;
                let bytes = if let Some(state) = self.get_transaction_state_if_traceable(
                    snapshot,
                    &hash,
                    current_index,
                    max_traceable_blocks,
                )? {
                    BigInt::from(state.block_index()).to_signed_bytes_le()
                } else {
                    BigInt::from(-1).to_signed_bytes_le()
                };
                Ok(bytes)
            }
            "getTransactionSigners" => {
                if args.len() != 1 {
                    return Err(Error::invalid_argument(
                        "getTransactionSigners requires 1 argument".to_string(),
                    ));
                }
                let hash = UInt256::from_bytes(&args[0]).map_err(|e| {
                    Error::invalid_argument(format!("Invalid transaction hash: {e}"))
                })?;
                let item = if let Some(state) = self.get_transaction_state_if_traceable(
                    snapshot,
                    &hash,
                    current_index,
                    max_traceable_blocks,
                )? {
                    let items = state
                        .transaction()
                        .signers()
                        .iter()
                        .map(|signer| signer.to_stack_item())
                        .collect::<std::result::Result<Vec<_>, _>>()?;
                    StackItem::from_array(items)
                } else {
                    StackItem::null()
                };
                Self::serialize_stack_item(item)
            }
            "getTransactionVMState" => {
                if args.len() != 1 {
                    return Err(Error::invalid_argument(
                        "getTransactionVMState requires 1 argument".to_string(),
                    ));
                }
                let hash = UInt256::from_bytes(&args[0]).map_err(|e| {
                    Error::invalid_argument(format!("Invalid transaction hash: {e}"))
                })?;
                if let Some(state) = self.get_transaction_state_if_traceable(
                    snapshot,
                    &hash,
                    current_index,
                    max_traceable_blocks,
                )? {
                    Ok(vec![state.vm_state_raw()])
                } else {
                    Ok(vec![0])
                }
            }
            _ => Err(Error::native_contract(format!(
                "Method {} not found",
                method
            ))),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn initialize(&self, _engine: &mut ApplicationEngine) -> Result<()> {
        Ok(())
    }

    fn on_persist(&self, engine: &mut ApplicationEngine) -> Result<()> {
        let snapshot = engine.snapshot_cache();
        let block = engine
            .persisting_block()
            .cloned()
            .ok_or_else(|| Error::native_contract("No current block available for persistence"))?;
        let tx_states: Vec<PersistedTransactionState> = block
            .transactions
            .iter()
            .map(|tx| PersistedTransactionState::new(tx, block.index()))
            .collect();
        engine.set_state(LedgerTransactionStates::new(tx_states.clone()));
        self.store_block_state(snapshot.as_ref(), &block, &tx_states)?;

        for tx in &block.transactions {
            let conflicts: Vec<UInt256> = tx
                .attributes()
                .iter()
                .filter_map(|attr| match attr {
                    TransactionAttribute::Conflicts(conflict) => Some(conflict.hash),
                    _ => None,
                })
                .collect();

            if conflicts.is_empty() {
                continue;
            }

            let signer_accounts: Vec<UInt160> =
                tx.signers().iter().map(|signer| signer.account).collect();

            for conflict_hash in conflicts {
                self.persist_conflict_stub(
                    snapshot.as_ref(),
                    &conflict_hash,
                    block.index(),
                    &signer_accounts,
                )?;
            }
        }

        Ok(())
    }

    fn post_persist(&self, engine: &mut ApplicationEngine) -> Result<()> {
        let snapshot = engine.snapshot_cache();
        let block = engine
            .persisting_block()
            .ok_or_else(|| Error::native_contract("No current block available for persistence"))?;
        let block_clone = block.clone();
        let hash = block_clone.hash();
        let index = block_clone.index();
        self.update_current_block_state(snapshot.as_ref(), &hash, index)?;

        if let Some(state_cache) = engine.take_state::<LedgerTransactionStates>() {
            let updates = state_cache.try_into_updates()?;
            if !updates.is_empty() {
                self.update_transaction_vm_states(snapshot.as_ref(), &updates)?;
            }
        }

        Ok(())
    }
}

impl Default for LedgerContract {
    fn default() -> Self {
        Self::new()
    }
}

impl LedgerContract {
    fn parse_index_or_hash(&self, data: &[u8], name: &str) -> Result<HashOrIndex> {
        if data.len() == 32 {
            let hash = UInt256::from_bytes(data)
                .map_err(|e| Error::invalid_argument(format!("Invalid {name}: {e}")))?;
            Ok(HashOrIndex::Hash(hash))
        } else if data.len() < 32 {
            let index = BigInt::from_signed_bytes_le(data)
                .to_u32()
                .ok_or_else(|| Error::invalid_argument(format!("Invalid {name} value")))?;
            Ok(HashOrIndex::Index(index))
        } else {
            Err(Error::invalid_argument(format!(
                "Invalid {name} length: {}",
                data.len()
            )))
        }
    }

    fn serialize_stack_item(item: StackItem) -> Result<Vec<u8>> {
        BinarySerializer::serialize(&item, &ExecutionEngineLimits::default())
            .map_err(|e| Error::serialization(format!("Failed to serialize ledger result: {e}")))
    }

    fn resolve_max_traceable_blocks<S>(&self, engine: &ApplicationEngine, snapshot: &S) -> u32
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let settings = engine.protocol_settings();
        let mut value = if engine.is_hardfork_enabled(Hardfork::HfEchidna) {
            max_traceable_blocks_from_snapshot(snapshot, settings.max_traceable_blocks)
        } else {
            settings.max_traceable_blocks
        };

        if value == 0 {
            value = settings.max_traceable_blocks;
        }

        value = value.min(PolicyContract::MAX_MAX_TRACEABLE_BLOCKS);
        value.max(1)
    }

    fn get_transaction_state_if_traceable<S>(
        &self,
        snapshot: &S,
        hash: &UInt256,
        current_index: u32,
        max_traceable: u32,
    ) -> Result<Option<PersistedTransactionState>>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        if let Some(state) = self.try_read_transaction_state(snapshot, hash)? {
            if Self::is_traceable_block(current_index, state.block_index(), max_traceable) {
                return Ok(Some(state));
            }
        }
        Ok(None)
    }

    fn get_transaction_from_block<S>(
        &self,
        snapshot: &S,
        block_hash: &UInt256,
        tx_index: u32,
        current_index: u32,
        max_traceable: u32,
    ) -> Result<Option<PersistedTransactionState>>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        if let Some(block) = self.try_read_block(snapshot, block_hash)? {
            if !Self::is_traceable_block(current_index, block.index(), max_traceable) {
                return Ok(None);
            }

            let tx_index = tx_index as usize;
            if tx_index >= block.transactions.len() {
                return Err(Error::invalid_argument(
                    "Transaction index out of range".to_string(),
                ));
            }

            let tx = &block.transactions[tx_index];
            let tx_hash = tx.try_hash()?;
            return self.get_transaction_state_if_traceable(
                snapshot,
                &tx_hash,
                current_index,
                max_traceable,
            );
        }
        Ok(None)
    }
}
