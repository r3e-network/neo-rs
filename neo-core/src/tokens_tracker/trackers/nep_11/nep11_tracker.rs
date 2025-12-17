//! NEP-11 tracker implementation.
//!
//! Tracks NEP-11 (non-fungible token) ownership and transfer history.

use super::super::token_balance::TokenBalance;
use super::super::token_transfer::TokenTransfer;
use super::super::tracker_base::{Tracker, TrackerBase, TransferRecord};
use super::nep11_balance_key::Nep11BalanceKey;
use super::nep11_transfer_key::Nep11TransferKey;
use crate::extensions::log_level::LogLevel;
use crate::neo_ledger::{ApplicationExecuted, Block};
use crate::neo_vm::{CallFlags, OpCode, ScriptBuilder, VMState};
use crate::persistence::DataCache;
use crate::smart_contract::native::contract_management::ContractManagement;
use crate::smart_contract::ApplicationEngine;
use crate::smart_contract::TriggerType;
use crate::{NeoSystem, UInt160};
use num_bigint::BigInt;
use num_traits::One;
use std::collections::HashMap;
use std::sync::Arc;

const NEP11_BALANCE_PREFIX: u8 = 0xf8;
const NEP11_TRANSFER_SENT_PREFIX: u8 = 0xf9;
const NEP11_TRANSFER_RECEIVED_PREFIX: u8 = 0xfa;

/// NEP-11 token tracker.
pub struct Nep11Tracker {
    base: TrackerBase,
    current_height: u32,
    current_block: Option<Block>,
}

impl Nep11Tracker {
    /// Creates a new NEP-11 tracker.
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
        transfers: &mut Vec<TransferRecord>,
        transfer_index: &mut u32,
    ) {
        if state_items.len() != 4 {
            return;
        }
        let Some(record) = TrackerBase::get_transfer_record(asset, state_items) else {
            return;
        };
        let Some(token_id) = record.token_id.clone() else {
            return;
        };

        transfers.push(record.clone());
        if let Some(container) = container {
            if let Some(tx) = container.as_transaction() {
                self.record_transfer_history(&record, &token_id, &tx.hash(), transfer_index);
            }
        }
    }

    fn record_transfer_history(
        &mut self,
        record: &TransferRecord,
        token_id: &[u8],
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
            let key = Nep11TransferKey::new(
                record.from,
                block.header.timestamp,
                record.asset,
                token_id.to_vec(),
                *transfer_index,
            );
            let value = TokenTransfer {
                amount: record.amount.clone(),
                user_script_hash: record.to,
                block_index: self.current_height,
                tx_hash: *tx_hash,
            };
            let _ = self.base.put(NEP11_TRANSFER_SENT_PREFIX, &key, &value);
        }

        if record.to != UInt160::zero() {
            let key = Nep11TransferKey::new(
                record.to,
                block.header.timestamp,
                record.asset,
                token_id.to_vec(),
                *transfer_index,
            );
            let value = TokenTransfer {
                amount: record.amount.clone(),
                user_script_hash: record.from,
                block_index: self.current_height,
                tx_hash: *tx_hash,
            };
            let _ = self.base.put(NEP11_TRANSFER_RECEIVED_PREFIX, &key, &value);
        }

        *transfer_index += 1;
    }

    fn save_divisible_nft_balance(&mut self, record: &TransferRecord, snapshot: &DataCache) {
        let Some(token_id) = record.token_id.clone() else {
            TrackerBase::log(
                self.track_name(),
                "Divisible NEP-11 transfer missing tokenId",
                LogLevel::Warning,
            );
            return;
        };

        let mut sb = ScriptBuilder::new();
        // balanceOf(from, tokenId)
        sb.emit_push(&record.from.to_bytes());
        sb.emit_push(&token_id);
        sb.emit_push_int(2);
        sb.emit_opcode(OpCode::PACK);
        sb.emit_push_int(CallFlags::ALL.bits() as i64);
        sb.emit_push("balanceOf".as_bytes());
        sb.emit_push(&record.asset.to_bytes());
        if sb.emit_syscall("System.Contract.Call").is_err() {
            return;
        }

        // balanceOf(to, tokenId)
        sb.emit_push(&record.to.to_bytes());
        sb.emit_push(&token_id);
        sb.emit_push_int(2);
        sb.emit_opcode(OpCode::PACK);
        sb.emit_push_int(CallFlags::ALL.bits() as i64);
        sb.emit_push("balanceOf".as_bytes());
        sb.emit_push(&record.asset.to_bytes());
        if sb.emit_syscall("System.Contract.Call").is_err() {
            return;
        }

        let mut engine = match ApplicationEngine::new(
            TriggerType::Application,
            None,
            Arc::new(snapshot.clone()),
            self.current_block.clone(),
            self.base.neo_system.settings().clone(),
            34_000_000,
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
            TrackerBase::log(
                self.track_name(),
                "Divisible NEP-11 balanceOf fault",
                LogLevel::Warning,
            );
            return;
        }

        if engine.state().contains(VMState::FAULT) || engine.result_stack().len() != 2 {
            TrackerBase::log(
                self.track_name(),
                "Divisible NEP-11 balanceOf returned unexpected stack size",
                LogLevel::Warning,
            );
            return;
        }

        let Ok(to_item) = engine.result_stack().peek(0) else {
            return;
        };
        let Ok(from_item) = engine.result_stack().peek(1) else {
            return;
        };
        let Ok(to_balance) = to_item.get_integer() else {
            return;
        };
        let Ok(from_balance) = from_item.get_integer() else {
            return;
        };

        let key_to = Nep11BalanceKey::new(record.to, record.asset, token_id.clone());
        let key_from = Nep11BalanceKey::new(record.from, record.asset, token_id);

        let value_to = TokenBalance {
            balance: to_balance,
            last_updated_block: self.current_height,
        };
        let value_from = TokenBalance {
            balance: from_balance,
            last_updated_block: self.current_height,
        };

        let _ = self.base.put(NEP11_BALANCE_PREFIX, &key_to, &value_to);
        let _ = self.base.put(NEP11_BALANCE_PREFIX, &key_from, &value_from);
    }

    fn save_nft_balance(&mut self, record: &TransferRecord) {
        let Some(token_id) = record.token_id.clone() else {
            TrackerBase::log(
                self.track_name(),
                "Indivisible NEP-11 transfer missing tokenId",
                LogLevel::Warning,
            );
            return;
        };

        if record.from != UInt160::zero() {
            let key_from = Nep11BalanceKey::new(record.from, record.asset, token_id.clone());
            let _ = self.base.delete(NEP11_BALANCE_PREFIX, &key_from);
        }

        if record.to != UInt160::zero() {
            let key_to = Nep11BalanceKey::new(record.to, record.asset, token_id);
            let value = TokenBalance {
                balance: BigInt::one(),
                last_updated_block: self.current_height,
            };
            let _ = self.base.put(NEP11_BALANCE_PREFIX, &key_to, &value);
        }
    }

    /// Returns the database prefixes for RPC queries.
    pub fn rpc_prefixes() -> (u8, u8, u8) {
        (
            NEP11_BALANCE_PREFIX,
            NEP11_TRANSFER_SENT_PREFIX,
            NEP11_TRANSFER_RECEIVED_PREFIX,
        )
    }
}

impl Tracker for Nep11Tracker {
    fn track_name(&self) -> &str {
        "Nep11Tracker"
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
        let mut transfers: Vec<TransferRecord> = Vec::new();

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

                if !contract.manifest.supports_standard("NEP-11") {
                    continue;
                }

                self.handle_notification(
                    notify.script_container.as_ref(),
                    &notify.script_hash,
                    &notify.state,
                    &mut transfers,
                    &mut transfer_index,
                );
            }
        }

        let mut divisibility: HashMap<UInt160, bool> = HashMap::new();
        for record in &transfers {
            use std::collections::hash_map::Entry;

            if let Entry::Vacant(entry) = divisibility.entry(record.asset) {
                let contract_state =
                    match ContractManagement::get_contract_from_snapshot(snapshot, &record.asset) {
                        Ok(Some(state)) => state,
                        _ => continue,
                    };
                let mut abi = contract_state.manifest.abi.clone();
                let has_balance1 = abi.get_method("balanceOf", 1).is_some();
                let has_balance2 = abi.get_method("balanceOf", 2).is_some();
                if !has_balance1 && !has_balance2 {
                    TrackerBase::log(
                        self.track_name(),
                        "Contract does not expose balanceOf for NEP-11",
                        LogLevel::Warning,
                    );
                    continue;
                }
                entry.insert(has_balance2);
            }

            if divisibility.get(&record.asset).copied().unwrap_or(false) {
                self.save_divisible_nft_balance(record, snapshot);
            } else {
                self.save_nft_balance(record);
            }
        }
    }

    fn reset_batch(&mut self) {
        self.base.reset_batch();
    }

    fn commit(&mut self) {
        self.base.commit();
    }
}
