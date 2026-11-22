//! dBFT consensus wiring for neo-node.

use crate::config::DbftSettings;
use neo_actors::ScheduleHandle;
use neo_consensus::{BlockData, ConsensusService, ValidatorInfo};
use neo_native_contracts::native::{NeoToken, helpers::NativeHelpers};
use neo_primitives::UInt256;
use neo_protocol::payloads::Transaction;
use neo_smart_contract::contract::Contract;
use neo_storage::persistence::Store;
use neo_wallets::Wallet;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::task::JoinHandle;

mod actor;
mod controller;
mod policy;
mod proposal;
mod recovery;

#[cfg(test)]
mod tests;

pub use controller::DbftConsensusController;

const CONSENSUS_STATE_KEY: [u8; 1] = [0xf4];

fn load_validators(system: &neo_system::NeoSystem, next_index: u32) -> Vec<neo_crypto::ECPoint> {
    let store_cache = system.store_cache();
    let snapshot = store_cache.data_cache();
    let settings = system.settings();
    let validators_count = settings.validators_count.max(0) as usize;
    let refresh =
        NativeHelpers::should_refresh_committee(next_index, settings.committee_members_count());
    let result = if refresh {
        NeoToken::new().compute_next_block_validators_snapshot(snapshot, settings)
    } else {
        NeoToken::new().get_next_block_validators_snapshot(snapshot, validators_count, settings)
    };
    result.unwrap_or_else(|_| settings.standby_validators())
}

fn find_signing_key(wallet: &Arc<dyn Wallet>, validators: &[neo_crypto::ECPoint]) -> Option<u8> {
    validators
        .iter()
        .enumerate()
        .find_map(|(index, public_key)| {
            let script_hash = Contract::create_signature_contract(public_key.clone()).script_hash();
            let account = wallet.get_account(&script_hash)?;
            (index <= u8::MAX as usize
                && !account.is_locked()
                && account.has_key()
                && account.get_key().is_some())
            .then_some(index as u8)
        })
}

pub(crate) fn ensure_wallet_can_validate(
    system: &Arc<neo_system::NeoSystem>,
    wallet: &Arc<dyn Wallet>,
) -> Result<(), String> {
    let next_index = system.current_block_index().checked_add(1).ok_or_else(|| {
        "dBFT cannot start because the next block height overflows u32".to_string()
    })?;
    let validators = load_validators(system, next_index);
    if validators.is_empty() {
        return Err(format!(
            "dBFT validator mode has no validators at next block height {next_index}"
        ));
    }
    if find_signing_key(wallet, &validators).is_none() {
        return Err(format!(
            "dBFT validator mode requires an unlocked validator key at next block height {next_index}; refusing to start a node that cannot produce blocks"
        ));
    }
    Ok(())
}

/// Wallet-changed handler that starts/stops the consensus actor.

struct ConsensusActor {
    system: Arc<neo_system::NeoSystem>,
    settings: DbftSettings,
    wallet: Arc<dyn Wallet>,
    service: Option<ConsensusService>,
    event_task: Option<JoinHandle<()>>,
    timer: Option<ScheduleHandle>,
    proposal_transactions: HashMap<UInt256, Transaction>,
    pending_block: Option<BlockData>,
    missing_transactions: Option<HashSet<UInt256>>,
    current_block_index: Option<u32>,
    current_prev_hash: Option<UInt256>,
    prev_timestamp: Option<u64>,
    validators: Vec<neo_crypto::ECPoint>,
    validator_infos: Vec<ValidatorInfo>,
    my_index: Option<u8>,
    recovery_path: PathBuf,
    recovery_store: Option<Arc<dyn Store>>,
    recovery_requested: bool,
}
