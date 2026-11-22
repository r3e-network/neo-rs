use super::*;

use super::actor::ConsensusActorMessage;

use neo_actors::ActorRef;
use neo_consensus::ConsensusSigner;
use neo_io::{MemoryReader, Serializable};
use neo_p2p::p2p::{
    MessageCommand, MessageHandlerSubscription,
    events::{MessageReceivedHandler, NetworkMessageRef},
    register_message_received_handler,
};
use neo_primitives::UInt160;
use parking_lot::Mutex;
use std::any::Any;
use tracing::warn;

pub struct DbftConsensusController {
    system: Arc<neo_system::NeoSystem>,
    settings: DbftSettings,
    actor: Mutex<Option<ActorRef>>,
    _message_subscription: MessageHandlerSubscription,
}

impl DbftConsensusController {
    pub fn new(system: Arc<neo_system::NeoSystem>, settings: DbftSettings) -> Self {
        let filter = Arc::new(DbftMessageFilter::new(settings.max_block_system_fee));
        let subscription = register_message_received_handler(filter);
        install_mempool_filter(&system, settings.max_block_system_fee);
        Self {
            system,
            settings,
            actor: Mutex::new(None),
            _message_subscription: subscription,
        }
    }

    pub fn start_with_wallet(&self, wallet: Arc<dyn Wallet>) -> bool {
        let mut guard = self.actor.lock();
        if guard.is_some() {
            return false;
        }

        let props =
            ConsensusActor::props(self.system.clone(), self.settings.clone(), wallet.clone());
        match self.system.actor_system().actor_of(props, "dbft-consensus") {
            Ok(actor) => {
                if !self.settings.auto_start {
                    let _ = actor.tell(ConsensusActorMessage::ManualStart);
                }
                *guard = Some(actor);
                true
            }
            Err(err) => {
                warn!(target: "neo", %err, "failed to start dBFT consensus actor");
                false
            }
        }
    }

    pub fn stop(&self) -> bool {
        let mut guard = self.actor.lock();
        if let Some(actor) = guard.take() {
            if let Err(err) = self.system.actor_system().stop(&actor) {
                warn!(target: "neo", %err, "failed to stop dBFT consensus actor");
                return false;
            }
            return true;
        }
        false
    }

    pub fn is_running(&self) -> bool {
        self.actor.lock().is_some()
    }
}

impl neo_system::i_event_handlers::WalletChangedHandler for DbftConsensusController {
    fn wallet_provider_wallet_changed_handler(
        &self,
        _sender: &dyn std::any::Any,
        wallet: Option<Arc<dyn Wallet>>,
    ) {
        if !self.settings.auto_start {
            return;
        }

        self.stop();
        if let Some(wallet) = wallet {
            self.start_with_wallet(wallet);
        }
    }
}

struct DbftMessageFilter {
    max_block_system_fee: i64,
}

impl DbftMessageFilter {
    fn new(max_block_system_fee: i64) -> Self {
        Self {
            max_block_system_fee,
        }
    }
}

impl MessageReceivedHandler for DbftMessageFilter {
    fn remote_node_message_received_handler(
        &self,
        _system: &dyn Any,
        message: &NetworkMessageRef,
    ) -> bool {
        if message.command != MessageCommand::Transaction {
            return true;
        }

        let mut reader = MemoryReader::new(&message.payload_raw);
        let Ok(tx) = Transaction::deserialize(&mut reader) else {
            return true;
        };

        tx.system_fee() <= self.max_block_system_fee
    }
}

fn install_mempool_filter(system: &Arc<neo_system::NeoSystem>, max_block_system_fee: i64) {
    let mempool = system.mempool();
    let mut guard = mempool.lock();
    let existing = guard.new_transaction.take();
    guard.new_transaction = Some(Box::new(move |pool, args| {
        if let Some(handler) = &existing {
            handler(pool, args);
        }
        if !args.cancel && args.transaction.system_fee() > max_block_system_fee {
            args.cancel = true;
        }
    }));
}

pub(super) struct WalletConsensusSigner {
    wallet: Arc<dyn Wallet>,
}

impl WalletConsensusSigner {
    pub(super) fn new(wallet: Arc<dyn Wallet>) -> Self {
        Self { wallet }
    }
}

impl ConsensusSigner for WalletConsensusSigner {
    fn can_sign(&self, script_hash: &UInt160) -> bool {
        self.wallet.get_account(script_hash).is_some_and(|account| {
            !account.is_locked() && account.has_key() && account.get_key().is_some()
        })
    }

    fn sign(
        &self,
        data: &[u8],
        script_hash: &UInt160,
        _context: &neo_consensus::ConsensusSignContext,
    ) -> neo_consensus::ConsensusResult<Vec<u8>> {
        let account = self.wallet.get_account(script_hash).ok_or_else(|| {
            neo_consensus::ConsensusError::state_error(format!(
                "Consensus validator account {script_hash} is not present in the active wallet"
            ))
        })?;
        if account.is_locked() {
            return Err(neo_consensus::ConsensusError::state_error(format!(
                "Consensus validator account {script_hash} is locked"
            )));
        }
        let key = account.get_key().ok_or_else(|| {
            neo_consensus::ConsensusError::state_error(format!(
                "Consensus validator account {script_hash} has no decrypted private key"
            ))
        })?;
        key.sign(data).map_err(|error| {
            neo_consensus::ConsensusError::state_error(format!(
                "Consensus wallet signing failed for {script_hash}: {error}"
            ))
        })
    }
}
