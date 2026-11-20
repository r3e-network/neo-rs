//! NEP-11 tracker implementation.
//!
//! Tracks NEP-11 (non-fungible token) ownership and transfer history.

use super::super::token_balance::TokenBalance;
use super::super::tracker_base::{ContractCall, Tracker, TrackerBase, TransferRecord};
use super::nep11_balance_key::Nep11BalanceKey;
use super::nep11_transfer_key::Nep11TransferKey;
use neo_ledger::ApplicationExecuted;
use neo_native_contracts::native::contract_management::ContractManagement;
use neo_primitives::UInt160;
use neo_primitives::log_level::LogLevel;
use neo_protocol::execution::Block;
use neo_storage::persistence::PersistenceDataCache;
use neo_system::NeoSystem;
use neo_vm::VmState as VMState;
use num_bigint::BigInt;
use num_traits::One;
use std::collections::HashMap;
use std::sync::Arc;

const NEP11_BALANCE_PREFIX: u8 = 0xf8;
const NEP11_TRANSFER_SENT_PREFIX: u8 = 0xf9;
const NEP11_TRANSFER_RECEIVED_PREFIX: u8 = 0xfa;

/// Gas limit for the `balanceOf` query engine.
const NEP11_BALANCE_OF_GAS: i64 = 34_000_000;

/// NEP-11 token tracker.
pub struct Nep11Tracker {
    base: TrackerBase,
    current_height: u32,
    current_block: Option<Block>,
}

impl Nep11Tracker {
    /// Creates a new NEP-11 tracker.
    pub fn new(
        db: Arc<dyn neo_storage::persistence::Store>,
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
        record: &TransferRecord,
        container: Option<&Arc<dyn neo_primitives::Verifiable>>,
        transfers: &mut Vec<TransferRecord>,
        transfer_index: &mut u32,
    ) {
        let Some(token_id) = record.token_id.clone() else {
            return;
        };

        transfers.push(record.clone());
        if let Some(container) = container
            && let Some(tx) = container
                .as_any()
                .downcast_ref::<neo_protocol::payloads::Transaction>()
        {
            self.record_transfer_history(record, &token_id, &tx.hash(), transfer_index);
        }
    }

    fn record_transfer_history(
        &mut self,
        record: &TransferRecord,
        token_id: &[u8],
        tx_hash: &neo_primitives::UInt256,
        transfer_index: &mut u32,
    ) {
        if !self.base.should_track_history {
            return;
        }
        let Some(ref block) = self.current_block else {
            return;
        };

        let timestamp_ms = block.header.timestamp;
        let block_index = self.current_height;
        let index = *transfer_index;
        self.base.record_transfer_history(
            NEP11_TRANSFER_SENT_PREFIX,
            NEP11_TRANSFER_RECEIVED_PREFIX,
            record,
            block_index,
            *tx_hash,
            index,
            |user, xfer_index| {
                Nep11TransferKey::new(
                    user,
                    timestamp_ms,
                    record.asset,
                    token_id.to_vec(),
                    xfer_index,
                )
            },
        );
        *transfer_index += 1;
    }

    fn save_divisible_nft_balance(
        &mut self,
        record: &TransferRecord,
        snapshot: &PersistenceDataCache,
    ) {
        let Some(token_id) = record.token_id.clone() else {
            TrackerBase::log(
                self.track_name(),
                "Divisible NEP-11 transfer missing tokenId",
                LogLevel::Warning,
            );
            return;
        };

        // Both balances come from a single script, so the transfer costs one
        // engine setup: the answer for `from` is pushed first and therefore sits
        // deeper on the result stack.
        let Some(engine) = self.base.call_contract(
            snapshot,
            self.current_block.as_ref(),
            record.asset,
            NEP11_BALANCE_OF_GAS,
            &[
                ContractCall {
                    method: "balanceOf",
                    args: vec![record.from.to_bytes(), token_id.clone()],
                },
                ContractCall {
                    method: "balanceOf",
                    args: vec![record.to.to_bytes(), token_id.clone()],
                },
            ],
        ) else {
            TrackerBase::log(
                self.track_name(),
                "Divisible NEP-11 balanceOf fault",
                LogLevel::Warning,
            );
            return;
        };

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
        let Ok(to_balance) = to_item.as_integer() else {
            return;
        };
        let Ok(from_balance) = from_item.as_integer() else {
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
        snapshot: &PersistenceDataCache,
        executed_list: &[ApplicationExecuted],
    ) {
        self.current_block = Some(block.clone());
        self.current_height = block.index();

        let mut transfers: Vec<TransferRecord> = Vec::new();
        TrackerBase::for_each_transfer(
            snapshot,
            executed_list,
            "NEP-11",
            4,
            |record, container, transfer_index| {
                self.handle_notification(record, container, &mut transfers, transfer_index);
            },
        );

        let mut divisibility: HashMap<UInt160, bool> = HashMap::new();
        for record in &transfers {
            use std::collections::hash_map::Entry;

            if let Entry::Vacant(entry) = divisibility.entry(record.asset) {
                let Ok(Some(contract_state)) =
                    ContractManagement::get_contract_from_snapshot(snapshot, &record.asset)
                else {
                    continue;
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

    fn commit(&mut self) -> Result<(), String> {
        self.base.commit()
    }
}
