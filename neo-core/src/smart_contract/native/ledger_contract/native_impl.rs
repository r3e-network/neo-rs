use super::*;
use crate::network::p2p::payloads::transaction_attribute::TransactionAttribute;
use crate::smart_contract::native::NativeContract;

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
                let hash = Self::parse_transaction_hash(&args[0])?;
                let item = match self.get_transaction_state_if_traceable(
                    snapshot,
                    &hash,
                    current_index,
                    max_traceable_blocks,
                )? {
                    Some(state) => state.transaction().to_stack_item()?,
                    _ => StackItem::null(),
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
                    match self.get_transaction_from_block(
                        snapshot,
                        &block_hash,
                        tx_index as u32,
                        current_index,
                        max_traceable_blocks,
                    )? {
                        Some(tx) => tx.transaction().to_stack_item()?,
                        _ => StackItem::null(),
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
                let hash = Self::parse_transaction_hash(&args[0])?;
                let bytes = match self.get_transaction_state_if_traceable(
                    snapshot,
                    &hash,
                    current_index,
                    max_traceable_blocks,
                )? {
                    Some(state) => BigInt::from(state.block_index()).to_signed_bytes_le(),
                    _ => BigInt::from(-1).to_signed_bytes_le(),
                };
                Ok(bytes)
            }
            "getTransactionSigners" => {
                if args.len() != 1 {
                    return Err(Error::invalid_argument(
                        "getTransactionSigners requires 1 argument".to_string(),
                    ));
                }
                let hash = Self::parse_transaction_hash(&args[0])?;
                let item = match self.get_transaction_state_if_traceable(
                    snapshot,
                    &hash,
                    current_index,
                    max_traceable_blocks,
                )? {
                    Some(state) => {
                        let items = state
                            .transaction()
                            .signers()
                            .iter()
                            .map(|signer| signer.to_stack_item())
                            .collect::<std::result::Result<Vec<_>, _>>()?;
                        StackItem::from_array(items)
                    }
                    _ => StackItem::null(),
                };
                Self::serialize_stack_item(item)
            }
            "getTransactionVMState" => {
                if args.len() != 1 {
                    return Err(Error::invalid_argument(
                        "getTransactionVMState requires 1 argument".to_string(),
                    ));
                }
                let hash = Self::parse_transaction_hash(&args[0])?;
                match self.get_transaction_state_if_traceable(
                    snapshot,
                    &hash,
                    current_index,
                    max_traceable_blocks,
                )? {
                    Some(state) => Ok(vec![state.vm_state_raw()]),
                    _ => Ok(vec![0]),
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
            let updates = state_cache.into_updates();
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
