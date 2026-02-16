//! NEP-17 tracker implementation.
//!
//! Tracks NEP-17 (fungible token) balances and transfer history.

use super::super::token_balance::TokenBalance;
use super::super::token_transfer::TokenTransfer;
use super::super::tracker_base::{Tracker, TrackerBase, TransferRecord};
use super::nep17_balance_key::Nep17BalanceKey;
use super::nep17_transfer_key::Nep17TransferKey;
use crate::extensions::log_level::LogLevel;
use crate::neo_ledger::{ApplicationExecuted, Block};
use crate::neo_vm::{CallFlags, OpCode, ScriptBuilder, VMState};
use crate::persistence::DataCache;
use crate::smart_contract::ApplicationEngine;
use crate::smart_contract::native::contract_management::ContractManagement;
use crate::{NeoSystem, UInt160};
use num_traits::Zero;
use std::collections::HashSet;
use std::sync::Arc;

const NEP17_BALANCE_PREFIX: u8 = 0xe8;
const NEP17_TRANSFER_SENT_PREFIX: u8 = 0xe9;
const NEP17_TRANSFER_RECEIVED_PREFIX: u8 = 0xea;

#[derive(Hash, Eq, PartialEq)]
struct BalanceChangeRecord {
    user: UInt160,
    asset: UInt160,
}

/// NEP-17 token tracker.
pub struct Nep17Tracker {
    base: TrackerBase,
    current_height: u32,
    current_block: Option<Block>,
}

impl Nep17Tracker {
    /// Creates a new NEP-17 tracker.
    pub fn new(
        db: Arc<dyn crate::persistence::IStore>,
        max_results: u32,
        should_track_history: bool,
        neo_system: Arc<NeoSystem>,
    ) -> Self {
        Self {
            base: TrackerBase::new(db, max_results, should_track_history, neo_system),
            current_height: 0,
            current_block: None,
        }
    }

    fn handle_notification(
        &mut self,
        container: Option<&Arc<dyn crate::IVerifiable>>,
        asset: &UInt160,
        state_items: &[crate::neo_vm::StackItem],
        balance_records: &mut HashSet<BalanceChangeRecord>,
        transfer_index: &mut u32,
    ) {
        if state_items.len() != 3 {
            return;
        }
        let Some(record) = TrackerBase::get_transfer_record(asset, state_items) else {
            return;
        };

        if record.from != UInt160::zero() {
            balance_records.insert(BalanceChangeRecord {
                user: record.from,
                asset: record.asset,
            });
        }
        if record.to != UInt160::zero() {
            balance_records.insert(BalanceChangeRecord {
                user: record.to,
                asset: record.asset,
            });
        }

        if let Some(container) = container {
            if let Some(tx) = container.as_transaction() {
                self.record_transfer_history(&record, &tx.hash(), transfer_index);
            }
        }
    }

    fn record_transfer_history(
        &mut self,
        record: &TransferRecord,
        tx_hash: &crate::UInt256,
        transfer_index: &mut u32,
    ) {
        if !self.base.should_track_history {
            return;
        }
        let Some(ref block) = self.current_block else {
            return;
        };

        if record.from != UInt160::zero() {
            let key = Nep17TransferKey::new(
                record.from,
                block.header.timestamp,
                record.asset,
                *transfer_index,
            );
            let value = TokenTransfer {
                amount: record.amount.clone(),
                user_script_hash: record.to,
                block_index: self.current_height,
                tx_hash: *tx_hash,
            };
            let _ = self.base.put(NEP17_TRANSFER_SENT_PREFIX, &key, &value);
        }

        if record.to != UInt160::zero() {
            let key = Nep17TransferKey::new(
                record.to,
                block.header.timestamp,
                record.asset,
                *transfer_index,
            );
            let value = TokenTransfer {
                amount: record.amount.clone(),
                user_script_hash: record.from,
                block_index: self.current_height,
                tx_hash: *tx_hash,
            };
            let _ = self.base.put(NEP17_TRANSFER_RECEIVED_PREFIX, &key, &value);
        }

        *transfer_index += 1;
    }

    fn save_nep17_balance(&mut self, record: &BalanceChangeRecord, snapshot: &DataCache) {
        let key = Nep17BalanceKey::new(record.user, record.asset);

        let mut sb = ScriptBuilder::new();
        sb.emit_push(&record.user.to_bytes());
        sb.emit_push_int(1);
        sb.emit_opcode(OpCode::PACK);
        sb.emit_push_int(CallFlags::ALL.bits() as i64);
        sb.emit_push("balanceOf".as_bytes());
        sb.emit_push(&record.asset.to_bytes());
        if sb.emit_syscall("System.Contract.Call").is_err() {
            return;
        }

        let mut engine = match ApplicationEngine::new(
            crate::smart_contract::TriggerType::Application,
            None,
            Arc::new(snapshot.clone()),
            self.current_block.clone(),
            self.base.neo_system.settings().clone(),
            17_000_000,
            None,
        ) {
            Ok(engine) => engine,
            Err(_) => return,
        };
        if engine
            .load_script(sb.to_array(), CallFlags::ALL, Some(record.asset))
            .and_then(|_| engine.execute())
            .is_err()
        {
            return;
        }

        let Ok(balance_item) = engine.peek(0) else {
            return;
        };
        let Ok(balance) = balance_item.get_integer() else {
            TrackerBase::log(
                self.track_name(),
                "balanceOf returned non-integer",
                LogLevel::Warning,
            );
            return;
        };

        if balance.is_zero() {
            let _ = self.base.delete(NEP17_BALANCE_PREFIX, &key);
            return;
        }

        let value = TokenBalance {
            balance,
            last_updated_block: self.current_height,
        };
        let _ = self.base.put(NEP17_BALANCE_PREFIX, &key, &value);
    }

    /// Returns the database prefixes for RPC queries.
    pub fn rpc_prefixes() -> (u8, u8, u8) {
        (
            NEP17_BALANCE_PREFIX,
            NEP17_TRANSFER_SENT_PREFIX,
            NEP17_TRANSFER_RECEIVED_PREFIX,
        )
    }
}

impl Tracker for Nep17Tracker {
    fn track_name(&self) -> &str {
        "Nep17Tracker"
    }

    fn on_persist(
        &mut self,
        _system: &NeoSystem,
        block: &Block,
        snapshot: &DataCache,
        executed_list: &[ApplicationExecuted],
    ) {
        self.current_block = Some(block.clone());
        self.current_height = block.index();

        let mut transfer_index: u32 = 0;
        let mut balance_records: HashSet<BalanceChangeRecord> = HashSet::new();

        for app in executed_list {
            if app.vm_state.contains(VMState::FAULT) {
                continue;
            }
            for notify in &app.notifications {
                if notify.event_name != "Transfer" || notify.state.is_empty() {
                    continue;
                }

                let contract = match ContractManagement::get_contract_from_snapshot(
                    snapshot,
                    &notify.script_hash,
                ) {
                    Ok(Some(contract)) => contract,
                    _ => continue,
                };

                if !contract.manifest.supports_standard("NEP-17") {
                    continue;
                }

                self.handle_notification(
                    notify.script_container.as_ref(),
                    &notify.script_hash,
                    &notify.state,
                    &mut balance_records,
                    &mut transfer_index,
                );
            }
        }

        for record in balance_records {
            self.save_nep17_balance(&record, snapshot);
        }
    }

    fn reset_batch(&mut self) {
        self.base.reset_batch();
    }

    fn commit(&mut self) {
        self.base.commit();
    }
}
