//! NEP-17 tracker implementation.
//!
//! Tracks NEP-17 (fungible token) balances and transfer history.

use super::super::token_balance::TokenBalance;
use super::super::tracker_base::{ContractCall, Tracker, TrackerBase, TransferRecord};
use super::nep17_balance_key::Nep17BalanceKey;
use super::nep17_transfer_key::Nep17TransferKey;
use neo_ledger::ApplicationExecuted;
use neo_primitives::UInt160;
use neo_primitives::log_level::LogLevel;
use neo_protocol::execution::Block;
use neo_storage::persistence::DataCache;
use neo_system::NeoSystem;
use num_traits::Zero;
use std::collections::HashSet;
use std::sync::Arc;

const NEP17_BALANCE_PREFIX: u8 = 0xe8;
const NEP17_TRANSFER_SENT_PREFIX: u8 = 0xe9;
const NEP17_TRANSFER_RECEIVED_PREFIX: u8 = 0xea;

/// Gas limit for the `balanceOf` query engine.
const NEP17_BALANCE_OF_GAS: i64 = 17_000_000;

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
        balance_records: &mut HashSet<BalanceChangeRecord>,
        transfer_index: &mut u32,
    ) {
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

        if let Some(container) = container
            && let Some(tx) = container
                .as_any()
                .downcast_ref::<neo_protocol::payloads::Transaction>()
        {
            self.record_transfer_history(record, &tx.hash(), transfer_index);
        }
    }

    fn record_transfer_history(
        &mut self,
        record: &TransferRecord,
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
            NEP17_TRANSFER_SENT_PREFIX,
            NEP17_TRANSFER_RECEIVED_PREFIX,
            record,
            block_index,
            *tx_hash,
            index,
            |user, xfer_index| Nep17TransferKey::new(user, timestamp_ms, record.asset, xfer_index),
        );
        *transfer_index += 1;
    }

    fn save_nep17_balance(&mut self, record: &BalanceChangeRecord, snapshot: &DataCache) {
        let key = Nep17BalanceKey::new(record.user, record.asset);

        let Some(engine) = self.base.call_contract(
            snapshot,
            self.current_block.as_ref(),
            record.asset,
            NEP17_BALANCE_OF_GAS,
            &[ContractCall {
                method: "balanceOf",
                args: vec![record.user.to_bytes()],
            }],
        ) else {
            return;
        };

        let Ok(balance_item) = engine.peek(0) else {
            return;
        };
        let Ok(balance) = balance_item.as_integer() else {
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

        let mut balance_records: HashSet<BalanceChangeRecord> = HashSet::new();
        TrackerBase::for_each_transfer(
            snapshot,
            executed_list,
            "NEP-17",
            3,
            |record, container, transfer_index| {
                self.handle_notification(record, container, &mut balance_records, transfer_index);
            },
        );

        for record in balance_records {
            self.save_nep17_balance(&record, snapshot);
        }
    }

    fn reset_batch(&mut self) {
        self.base.reset_batch();
    }

    fn commit(&mut self) -> Result<(), String> {
        self.base.commit()
    }
}
