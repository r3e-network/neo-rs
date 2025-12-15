//! Persistence pipeline helpers for `NeoSystem`.
//!
//! This module keeps the block execution and commit pipeline isolated from the
//! core orchestration logic.

use std::sync::Arc;

use super::converters::convert_payload_block;
use super::NeoSystem;
use crate::error::{CoreError, CoreResult};
use crate::events::PluginEvent;
use crate::ledger::block::Block as LedgerBlock;
use crate::ledger::blockchain_application_executed::ApplicationExecuted;
use crate::network::p2p::payloads::block::Block;
use crate::persistence::data_cache::DataCache;
use crate::persistence::StoreTransaction;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::application_engine::TEST_MODE_GAS;
use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::native::ledger_contract::{
    LedgerTransactionStates, PersistedTransactionState,
};
use crate::smart_contract::trigger_type::TriggerType;
use neo_vm::vm_state::VMState;

impl NeoSystem {
    /// Persists a block through the minimal smart-contract pipeline, returning
    /// the list of execution summaries produced during processing.
    pub fn persist_block(&self, block: Block) -> CoreResult<Vec<ApplicationExecuted>> {
        let ledger_block = convert_payload_block(&block);
        let mut tx = StoreTransaction::from_snapshot(self.store().get_snapshot());
        let base_snapshot = Arc::new(tx.cache().data_cache().clone());

        let mut on_persist_engine = ApplicationEngine::new(
            TriggerType::OnPersist,
            None,
            Arc::clone(&base_snapshot),
            Some(ledger_block.clone()),
            self.settings().clone(),
            TEST_MODE_GAS,
            None,
        )?;

        on_persist_engine.native_on_persist()?;
        let on_persist_exec = ApplicationExecuted::new(&mut on_persist_engine);
        self.actor_system()
            .event_stream()
            .publish(on_persist_exec.clone());

        let mut executed = Vec::with_capacity(ledger_block.transactions.len() + 2);
        executed.push(on_persist_exec);

        let mut tx_states = on_persist_engine
            .take_state::<LedgerTransactionStates>()
            .unwrap_or_else(|| {
                LedgerTransactionStates::new(Vec::<PersistedTransactionState>::new())
            });

        for tx in &ledger_block.transactions {
            let tx_snapshot = Arc::new(base_snapshot.as_ref().clone_cache());
            let container: Arc<dyn crate::IVerifiable> = Arc::new(tx.clone());
            let mut tx_engine = ApplicationEngine::new(
                TriggerType::Application,
                Some(container),
                Arc::clone(&tx_snapshot),
                Some(ledger_block.clone()),
                self.settings().clone(),
                tx.system_fee(),
                None,
            )?;

            tx_engine.set_state(tx_states);
            tx_engine.load_script(tx.script().to_vec(), CallFlags::ALL, None)?;
            tx_engine.execute()?;

            let vm_state = tx_engine.state();
            let tx_hash = tx.hash();

            let executed_tx = ApplicationExecuted::new(&mut tx_engine);
            self.actor_system()
                .event_stream()
                .publish(executed_tx.clone());
            tx_states = tx_engine
                .take_state::<LedgerTransactionStates>()
                .unwrap_or_else(|| {
                    LedgerTransactionStates::new(Vec::<PersistedTransactionState>::new())
                });
            executed.push(executed_tx);

            if vm_state != VMState::HALT {
                return Err(CoreError::system(format!(
                    "transaction execution halted in state {:?} for hash {}",
                    vm_state, tx_hash
                )));
            }

            let tracked = tx_snapshot.tracked_items();
            base_snapshot.merge_tracked_items(&tracked);
        }

        let mut post_persist_engine = ApplicationEngine::new(
            TriggerType::PostPersist,
            None,
            Arc::clone(&base_snapshot),
            Some(ledger_block.clone()),
            self.settings().clone(),
            TEST_MODE_GAS,
            None,
        )?;
        post_persist_engine.set_state(tx_states);
        post_persist_engine.native_post_persist()?;
        let post_persist_exec = ApplicationExecuted::new(&mut post_persist_engine);
        self.actor_system()
            .event_stream()
            .publish(post_persist_exec.clone());
        executed.push(post_persist_exec);

        self.invoke_committing(&ledger_block, base_snapshot.as_ref(), &executed);

        crate::persistence::transaction::apply_tracked_items(
            tx.cache_mut(),
            base_snapshot.tracked_items(),
        );

        tx.commit().map_err(|err| {
            CoreError::system(format!(
                "failed to commit store cache for block {}: {err}",
                ledger_block.index()
            ))
        })?;

        // Update in-memory caches with the payload block so networking queries can respond immediately.
        self.context().record_block(block.clone());

        // Notify plugins that a block has been persisted, matching the C# event ordering.
        let block_hash = ledger_block.hash().to_string();
        let block_height = ledger_block.index();
        self.context()
            .broadcast_plugin_event(PluginEvent::BlockReceived {
                block_hash,
                block_height,
            });

        self.invoke_committed(&ledger_block);

        Ok(executed)
    }

    fn invoke_committing(
        &self,
        block: &LedgerBlock,
        snapshot: &DataCache,
        application_executed: &[ApplicationExecuted],
    ) {
        let handlers = { self.context().committing_handlers().read().clone() };
        for handler in handlers {
            handler.blockchain_committing_handler(self, block, snapshot, application_executed);
        }
    }

    fn invoke_committed(&self, block: &LedgerBlock) {
        let handlers = { self.context().committed_handlers().read().clone() };
        for handler in handlers {
            handler.blockchain_committed_handler(self, block);
        }
    }
}
