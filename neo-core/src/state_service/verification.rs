//! State root verification service.
//!
//! Matches the behaviour of `Neo.Plugins.StateService.Verification`.

use crate::akka::{Actor, ActorContext, ActorRef, ActorResult, Cancelable, Props};
use crate::ledger::{BlockchainCommand, PersistCompleted, RelayResult, VerifyResult};
use crate::neo_io::{BinaryWriter, MemoryReader, Serializable};
use crate::network::p2p::helper::get_sign_data_vec;
use crate::network::p2p::payloads::extensible_payload::ExtensiblePayload;
use crate::smart_contract::contract::Contract;
use crate::smart_contract::contract_parameters_context::ContractParametersContext;
use crate::smart_contract::native::role_management::RoleManagement;
use crate::smart_contract::native::Role;
use crate::state_service::{MessageType, StateRoot, StateStore, ValidatedRootPersisted, Vote};
use crate::wallets::Wallet;
use crate::{cryptography::Crypto, neo_system::NeoSystem, UInt160};
use async_trait::async_trait;
use parking_lot::Mutex;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

use super::STATE_SERVICE_CATEGORY;

const MAX_CACHED_VERIFICATION_PROCESS_COUNT: usize = 10;
const INITIAL_DELAY_MS: u64 = 3_000;
const MAX_VALID_UNTIL_BLOCK_INCREMENT: u32 = 100;

#[derive(Clone, Debug)]
struct VerificationTimer {
    index: u32,
}

struct VerificationContext {
    root_index: u32,
    validators: Vec<crate::cryptography::ECPoint>,
    my_index: usize,
    signatures: HashMap<usize, Vec<u8>>,
    retries: u32,
    state_root: Option<StateRoot>,
    vote_payload: Option<ExtensiblePayload>,
    state_root_payload: Option<ExtensiblePayload>,
    timer: Option<Cancelable>,
}

impl VerificationContext {
    fn new(
        root_index: u32,
        validators: Vec<crate::cryptography::ECPoint>,
        my_index: usize,
    ) -> Self {
        Self {
            root_index,
            validators,
            my_index,
            signatures: HashMap::new(),
            retries: 0,
            state_root: None,
            vote_payload: None,
            state_root_payload: None,
            timer: None,
        }
    }

    fn required_signatures(&self) -> usize {
        let n = self.validators.len();
        n.saturating_sub((n.saturating_sub(1)) / 3)
    }

    fn sender_index(&self) -> usize {
        let n = self.validators.len() as i64;
        if n == 0 {
            return 0;
        }
        let mut sender = (self.root_index as i64 - self.retries as i64) % n;
        if sender < 0 {
            sender += n;
        }
        sender as usize
    }

    fn is_sender(&self) -> bool {
        self.my_index == self.sender_index()
    }

    fn cancel_timer(&mut self) {
        if let Some(timer) = self.timer.take() {
            timer.cancel();
        }
    }

    fn sender_script_hash(&self) -> UInt160 {
        let key = self.validators[self.my_index].clone();
        Contract::create_signature_contract(key).script_hash()
    }

    fn sender_verification_script(&self) -> Vec<u8> {
        let key = self.validators[self.my_index].clone();
        Contract::create_signature_redeem_script(key)
    }

    fn load_state_root(&mut self, store: &StateStore) -> Option<&mut StateRoot> {
        if self.state_root.is_none() {
            self.state_root = store.get_state_root(self.root_index);
        }
        self.state_root.as_mut()
    }

    fn add_signature(
        &mut self,
        index: usize,
        signature: Vec<u8>,
        store: &StateStore,
        network: u32,
    ) -> bool {
        let required = self.required_signatures();
        if self.signatures.len() >= required {
            return false;
        }
        if index >= self.validators.len() {
            return false;
        }
        if self.signatures.contains_key(&index) {
            return false;
        }

        let state_root = match self.load_state_root(store) {
            Some(root) => root,
            None => return false,
        };

        let Ok(sign_data) = get_sign_data_vec(state_root, network) else {
            return false;
        };

        if !Crypto::verify_signature_bytes(
            &sign_data,
            &signature,
            self.validators[index].as_bytes(),
        ) {
            return false;
        }

        self.signatures.insert(index, signature);
        true
    }

    fn check_signatures(
        &mut self,
        store: &StateStore,
        snapshot: Arc<crate::persistence::DataCache>,
        network: u32,
    ) -> bool {
        let required = self.required_signatures();
        if self.signatures.len() < required {
            return false;
        }

        let validators = self.validators.clone();
        let signatures = self.signatures.clone();
        let state_root = match self.load_state_root(store) {
            Some(root) => root,
            None => return false,
        };

        if state_root.witness.is_none() {
            let contract = Contract::create_multi_sig_contract(required, &validators);
            let mut context = ContractParametersContext::new(snapshot, state_root.clone(), network);
            let mut added = 0usize;
            for (idx, validator) in validators.iter().enumerate() {
                let Some(sig) = signatures.get(&idx) else {
                    continue;
                };
                if let Ok(true) =
                    context.add_signature(contract.clone(), validator.clone(), sig.clone())
                {
                    added = added.saturating_add(1);
                }
                if added >= required {
                    break;
                }
            }
            if !context.completed() {
                return false;
            }
            let Some(witnesses) = context.get_witnesses() else {
                return false;
            };
            state_root.witness = Some(witnesses[0].clone());
        }

        true
    }
}

struct StateVerificationActor {
    system: Arc<NeoSystem>,
    wallet: Arc<dyn Wallet>,
    state_store: Option<Arc<StateStore>>,
    contexts: BTreeMap<u32, VerificationContext>,
}

impl StateVerificationActor {
    fn new(system: Arc<NeoSystem>, wallet: Arc<dyn Wallet>) -> Self {
        let state_store = system.state_store().ok().flatten();
        Self {
            system,
            wallet,
            state_store,
            contexts: BTreeMap::new(),
        }
    }

    fn props(system: Arc<NeoSystem>, wallet: Arc<dyn Wallet>) -> Props {
        Props::new(move || Self::new(system.clone(), wallet.clone()))
    }

    fn prune_old_contexts(&mut self) {
        while self.contexts.len() >= MAX_CACHED_VERIFICATION_PROCESS_COUNT {
            if let Some((&index, _)) = self.contexts.iter().next() {
                if let Some(mut context) = self.contexts.remove(&index) {
                    context.cancel_timer();
                }
            } else {
                break;
            }
        }
    }

    fn find_validator_index(
        wallet: &Arc<dyn Wallet>,
        validators: &[crate::cryptography::ECPoint],
    ) -> Option<usize> {
        let accounts = wallet.get_accounts();
        for (idx, validator) in validators.iter().enumerate() {
            let script_hash = Contract::create_signature_contract(validator.clone()).script_hash();
            if accounts
                .iter()
                .any(|account| account.script_hash() == script_hash && account.has_key())
            {
                return Some(idx);
            }
        }
        None
    }

    async fn on_block_persisted(&mut self, index: u32, ctx: &ActorContext) {
        if self.state_store.is_none() {
            return;
        }

        self.prune_old_contexts();

        let store_cache = self.system.store_cache();
        let snapshot = store_cache.data_cache();
        let validators = RoleManagement::new()
            .get_designated_by_role_at(snapshot, Role::StateValidator, index)
            .unwrap_or_default();
        if validators.is_empty() {
            return;
        }

        let Some(my_index) = Self::find_validator_index(&self.wallet, &validators) else {
            return;
        };

        let mut context = VerificationContext::new(index, validators, my_index);
        let timer = ctx.schedule_once(
            Duration::from_millis(INITIAL_DELAY_MS),
            &ctx.self_ref(),
            VerificationTimer { index },
        );
        context.timer = Some(timer);
        self.contexts.insert(index, context);
        info!(
            target: "state",
            index,
            my_index,
            active = self.contexts.len(),
            "state verification context started"
        );
    }

    fn on_validated_root_persisted(&mut self, index: u32) {
        let keys: Vec<u32> = self
            .contexts
            .keys()
            .cloned()
            .filter(|key| *key <= index)
            .collect();
        for key in keys {
            if let Some(mut context) = self.contexts.remove(&key) {
                context.cancel_timer();
            }
        }
    }

    async fn on_timer(&mut self, index: u32, ctx: &ActorContext) {
        let Some(mut context) = self.contexts.remove(&index) else {
            return;
        };

        self.send_vote(&mut context).await;
        self.check_votes(&mut context).await;

        context.cancel_timer();
        let delay_ms = self.next_delay_ms(context.retries);
        let timer = ctx.schedule_once(
            Duration::from_millis(delay_ms),
            &ctx.self_ref(),
            VerificationTimer { index },
        );
        context.timer = Some(timer);
        context.retries = context.retries.saturating_add(1);
        self.contexts.insert(index, context);
    }

    async fn on_relay_result(&mut self, result: RelayResult) {
        if result.result != VerifyResult::Succeed
            || result.inventory_type != crate::InventoryType::Extensible
        {
            return;
        }

        let context = self.system.context();
        let payload = context
            .try_get_extensible(&result.hash)
            .or_else(|| context.try_get_relay_extensible(&result.hash));
        let Some(payload) = payload else {
            return;
        };
        if payload.category != STATE_SERVICE_CATEGORY {
            return;
        }
        if payload.data.is_empty() {
            return;
        }

        let message_type = MessageType::from_byte(payload.data[0]);
        if message_type != Some(MessageType::Vote) {
            return;
        }

        let mut reader = MemoryReader::new(&payload.data[1..]);
        let vote = match Vote::deserialize(&mut reader) {
            Ok(vote) => vote,
            Err(_) => return,
        };

        self.on_vote(vote).await;
    }

    async fn on_vote(&mut self, vote: Vote) {
        let Some(state_store) = &self.state_store else {
            return;
        };

        if vote.validator_index < 0 {
            return;
        }
        let index = vote.root_index;
        let Some(mut context) = self.contexts.remove(&index) else {
            return;
        };

        let added = context.add_signature(
            vote.validator_index as usize,
            vote.signature,
            state_store,
            self.system.settings().network,
        );
        if !added {
            self.contexts.insert(index, context);
            return;
        }
        self.check_votes(&mut context).await;
        self.contexts.insert(index, context);
    }

    async fn send_vote(&self, context: &mut VerificationContext) {
        if context.vote_payload.is_none() {
            let payload = self.build_vote_payload(context).await;
            context.vote_payload = payload;
        }

        if let Some(payload) = context.vote_payload.clone() {
            self.relay_payload(payload);
        }
    }

    async fn check_votes(&self, context: &mut VerificationContext) {
        if !context.is_sender() {
            return;
        }

        let Some(state_store) = &self.state_store else {
            return;
        };

        let snapshot = Arc::new(self.system.store_cache().data_cache().clone());
        let ok = context.check_signatures(state_store, snapshot, self.system.settings().network);
        if !ok {
            return;
        }

        if context.state_root_payload.is_none() {
            let payload = self.build_state_root_payload(context).await;
            context.state_root_payload = payload;
        }

        if let Some(payload) = context.state_root_payload.clone() {
            self.relay_payload(payload);
        }
    }

    fn relay_payload(&self, payload: ExtensiblePayload) {
        if let Err(error) =
            self.system
                .blockchain_actor()
                .tell(BlockchainCommand::InventoryExtensible {
                    payload,
                    relay: true,
                })
        {
            warn!(target: "state", %error, "failed to relay state service payload");
        }
    }

    async fn build_vote_payload(
        &self,
        context: &mut VerificationContext,
    ) -> Option<ExtensiblePayload> {
        let state_store = self.state_store.as_ref()?;
        let sign_data = {
            let state_root = context.load_state_root(state_store)?;
            get_sign_data_vec(state_root, self.system.settings().network).ok()?
        };

        let signature = if let Some(sig) = context.signatures.get(&context.my_index) {
            sig.clone()
        } else {
            let sender = context.sender_script_hash();
            let sig = self.wallet.sign(&sign_data, &sender).await.ok()?;
            context.signatures.insert(context.my_index, sig.clone());
            sig
        };

        let vote = Vote {
            validator_index: context.my_index as i32,
            root_index: context.root_index,
            signature,
        };
        let mut writer = BinaryWriter::new();
        vote.serialize(&mut writer).ok()?;
        self.build_extensible_payload(
            context,
            MessageType::Vote,
            writer.into_bytes(),
            MAX_CACHED_VERIFICATION_PROCESS_COUNT as u32,
        )
        .await
    }

    async fn build_state_root_payload(
        &self,
        context: &mut VerificationContext,
    ) -> Option<ExtensiblePayload> {
        let state_store = self.state_store.as_ref()?;
        let state_root = context.load_state_root(state_store)?;
        if state_root.witness.is_none() {
            return None;
        }

        let mut writer = BinaryWriter::new();
        state_root.serialize(&mut writer).ok()?;
        self.build_extensible_payload(
            context,
            MessageType::StateRoot,
            writer.into_bytes(),
            MAX_VALID_UNTIL_BLOCK_INCREMENT,
        )
        .await
    }

    async fn build_extensible_payload(
        &self,
        context: &VerificationContext,
        message_type: MessageType,
        payload_bytes: Vec<u8>,
        valid_block_end_increment: u32,
    ) -> Option<ExtensiblePayload> {
        let mut data = Vec::with_capacity(1 + payload_bytes.len());
        data.push(message_type as u8);
        data.extend_from_slice(&payload_bytes);

        let sender = context.sender_script_hash();
        let mut payload = ExtensiblePayload::new();
        payload.category = STATE_SERVICE_CATEGORY.to_string();
        payload.valid_block_start = context.root_index;
        payload.valid_block_end = context.root_index.saturating_add(valid_block_end_increment);
        payload.sender = sender;
        payload.data = data;

        let sign_data = get_sign_data_vec(&payload, self.system.settings().network).ok()?;
        let signature = self.wallet.sign(&sign_data, &sender).await.ok()?;

        let mut invocation = Vec::with_capacity(signature.len() + 2);
        invocation.push(neo_vm::op_code::OpCode::PUSHDATA1 as u8);
        invocation.push(signature.len() as u8);
        invocation.extend_from_slice(&signature);
        payload.witness = crate::network::p2p::payloads::witness::Witness::new_with_scripts(
            invocation,
            context.sender_verification_script(),
        );

        Some(payload)
    }

    fn next_delay_ms(&self, retries: u32) -> u64 {
        let base_ms = self.system.time_per_block().as_millis() as u64;
        let shift = retries.min(32);
        base_ms.saturating_mul(1u64 << shift)
    }
}

#[async_trait]
impl Actor for StateVerificationActor {
    async fn pre_start(&mut self, ctx: &mut ActorContext) -> ActorResult {
        let stream = self.system.event_stream();
        stream.subscribe::<PersistCompleted>(ctx.self_ref());
        stream.subscribe::<RelayResult>(ctx.self_ref());
        stream.subscribe::<ValidatedRootPersisted>(ctx.self_ref());
        Ok(())
    }

    async fn post_stop(&mut self, ctx: &mut ActorContext) -> ActorResult {
        self.system.event_stream().unsubscribe_all(&ctx.self_ref());
        while let Some((_index, mut context)) = self.contexts.pop_first() {
            context.cancel_timer();
        }
        Ok(())
    }

    async fn handle(
        &mut self,
        envelope: Box<dyn std::any::Any + Send>,
        ctx: &mut ActorContext,
    ) -> ActorResult {
        let envelope = match envelope.downcast::<PersistCompleted>() {
            Ok(message) => {
                self.on_block_persisted(message.block.index(), ctx).await;
                return Ok(());
            }
            Err(envelope) => envelope,
        };

        let envelope = match envelope.downcast::<RelayResult>() {
            Ok(message) => {
                self.on_relay_result(*message).await;
                return Ok(());
            }
            Err(envelope) => envelope,
        };

        let envelope = match envelope.downcast::<ValidatedRootPersisted>() {
            Ok(message) => {
                self.on_validated_root_persisted(message.index);
                return Ok(());
            }
            Err(envelope) => envelope,
        };

        if let Ok(message) = envelope.downcast::<VerificationTimer>() {
            self.on_timer(message.index, ctx).await;
        }

        Ok(())
    }
}

/// Wallet-changed handler that starts/stops the state verification actor.
pub struct StateServiceVerification {
    system: Arc<NeoSystem>,
    actor: Mutex<Option<ActorRef>>,
}

impl StateServiceVerification {
    /// Creates a new state verification handler bound to the given system.
    pub fn new(system: Arc<NeoSystem>) -> Self {
        Self {
            system,
            actor: Mutex::new(None),
        }
    }

    fn start(&self, wallet: Arc<dyn Wallet>) {
        let mut guard = self.actor.lock();
        if guard.is_some() {
            return;
        }

        let props = StateVerificationActor::props(self.system.clone(), wallet);
        match self.system.actor_system().actor_of(props, "state-verifier") {
            Ok(actor) => {
                *guard = Some(actor);
            }
            Err(err) => {
                warn!(target: "state", %err, "failed to start state verification actor");
            }
        }
    }

    fn stop(&self) {
        let mut guard = self.actor.lock();
        if let Some(actor) = guard.take() {
            if let Err(err) = self.system.actor_system().stop(&actor) {
                warn!(target: "state", %err, "failed to stop state verification actor");
            }
        }
    }
}

impl crate::i_event_handlers::IWalletChangedHandler for StateServiceVerification {
    fn i_wallet_provider_wallet_changed_handler(
        &self,
        _sender: &dyn std::any::Any,
        wallet: Option<Arc<dyn Wallet>>,
    ) {
        match wallet {
            Some(wallet) => self.start(wallet),
            None => self.stop(),
        }
    }
}
