use crate::p2p_service::BroadcastMessage;
use neo_core::cryptography::ECPoint;
use neo_core::neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_core::network::p2p::payloads::{ExtensiblePayload, Witness};
use neo_core::network::p2p::ProtocolMessage;
use neo_core::smart_contract::helper::Helper as ContractHelper;
use neo_core::smart_contract::Contract;
use neo_core::state_service::{
    StateRoot, Vote, STATE_SERVICE_CATEGORY, STATE_SERVICE_MESSAGE_STATE_ROOT,
    STATE_SERVICE_MESSAGE_VOTE,
};
use neo_core::wallets::KeyPair;
use neo_core::{UInt160, UInt256};
use neo_crypto::Crypto;
use std::collections::{BTreeMap, HashMap, HashSet};
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

const MAX_VALID_UNTIL_BLOCK_INCREMENT: u32 = 100;
const MAX_CACHED_VERIFICATION_PROCESS_COUNT: u32 = 10;

#[derive(Clone)]
pub(crate) struct StateServiceReactorConfig {
    pub network_magic: u32,
    pub verifiers: Vec<ECPoint>,
    pub signer: Option<StateServiceSigner>,
    pub p2p_broadcast_tx: Option<broadcast::Sender<BroadcastMessage>>,
}

#[derive(Clone)]
pub(crate) struct StateServiceSigner {
    pub my_index: i32,
    pub key_pair: KeyPair,
    pub sender: UInt160,
    pub verification_script: Vec<u8>,
}

pub(crate) struct StateServiceReactor {
    network_magic: u32,
    verifiers: Vec<ECPoint>,
    allowed_senders: HashSet<UInt160>,
    signer: Option<StateServiceSigner>,
    p2p_broadcast_tx: Option<broadcast::Sender<BroadcastMessage>>,
    contexts: BTreeMap<u32, VerificationContext>,
}

struct VerificationContext {
    root_index: u32,
    retries: i32,
    state_root: StateRoot,
    signatures: HashMap<i32, Vec<u8>>,
    root_broadcasted: bool,
}

impl StateServiceReactor {
    pub(crate) fn new(config: StateServiceReactorConfig) -> Self {
        let allowed_senders = config
            .verifiers
            .iter()
            .map(|key| Contract::create_signature_contract(key.clone()).script_hash())
            .collect();
        Self {
            network_magic: config.network_magic,
            verifiers: config.verifiers,
            allowed_senders,
            signer: config.signer,
            p2p_broadcast_tx: config.p2p_broadcast_tx,
            contexts: BTreeMap::new(),
        }
    }

    pub(crate) fn on_local_state_root(&mut self, index: u32, root_hash: UInt256) {
        let Some(signer) = self.signer.clone() else {
            return;
        };
        if self.verifiers.is_empty() {
            return;
        }

        self.evict_old_contexts();
        let my_signature = {
            let context = self.contexts.entry(index).or_insert_with(|| VerificationContext {
                root_index: index,
                retries: 0,
                state_root: StateRoot::new_current(index, root_hash),
                signatures: HashMap::new(),
                root_broadcasted: false,
            });

            // If the local state root changed (e.g. reorg), reset cached signatures.
            if context.state_root.root_hash != root_hash {
                context.state_root = StateRoot::new_current(index, root_hash);
                context.signatures.clear();
                context.root_broadcasted = false;
                context.retries = 0;
            }

            // Ensure we have our own vote signature cached.
            if !context.signatures.contains_key(&signer.my_index) {
                if let Some(sig) = sign_state_root(&context.state_root, &signer.key_pair, self.network_magic) {
                    context.signatures.insert(signer.my_index, sig);
                }
            }

            context.signatures.get(&signer.my_index).cloned()
        };

        // Broadcast our vote message immediately (C# uses a short delay).
        if let Some(signature) = my_signature {
            if let Some(vote_payload) = self.build_vote_payload_for(&signer, index, signature) {
                self.broadcast(vote_payload);
            }
        }

        // If we're the designated sender and already have enough signatures (e.g. fast sync),
        // broadcast the state root payload.
        self.maybe_broadcast_state_root(index, &signer);
    }

    pub(crate) fn handle_incoming_payload(
        &mut self,
        mut payload: ExtensiblePayload,
        current_height: u32,
        from: &str,
    ) -> Option<StateRoot> {
        if payload.category != STATE_SERVICE_CATEGORY && payload.category != "StateRoot" {
            return None;
        }

        if payload.category == "StateRoot" {
            // Legacy compatibility: some peers still relay raw StateRoot without a MessageType prefix.
            let mut reader = MemoryReader::new(&payload.data);
            if let Ok(root) = StateRoot::deserialize(&mut reader) {
                return Some(root);
            }
        }

        if payload.category == STATE_SERVICE_CATEGORY {
            if !self.verify_envelope(&mut payload, current_height) {
                debug!(
                    target: "neo::runtime",
                    from,
                    sender = %payload.sender,
                    "invalid StateService extensible payload envelope"
                );
                return None;
            }
        }

        if payload.data.is_empty() {
            return None;
        }

        match payload.data[0] {
            STATE_SERVICE_MESSAGE_VOTE => {
                let mut reader = MemoryReader::new(&payload.data[1..]);
                let vote = match Vote::deserialize(&mut reader) {
                    Ok(v) => v,
                    Err(err) => {
                        debug!(
                            target: "neo::runtime",
                            from,
                            %err,
                            "failed to deserialize StateService vote"
                        );
                        return None;
                    }
                };
                self.on_vote(vote, from);
                None
            }
            STATE_SERVICE_MESSAGE_STATE_ROOT => {
                let mut reader = MemoryReader::new(&payload.data[1..]);
                match StateRoot::deserialize(&mut reader) {
                    Ok(root) => Some(root),
                    Err(err) => {
                        debug!(
                            target: "neo::runtime",
                            from,
                            %err,
                            "failed to deserialize StateService state root"
                        );
                        None
                    }
                }
            }
            other => {
                debug!(
                    target: "neo::runtime",
                    from,
                    message_type = other,
                    "unknown StateService message type"
                );
                None
            }
        }
    }

    fn on_vote(&mut self, vote: Vote, from: &str) {
        let Some(signer) = self.signer.clone() else {
            // Non-validator nodes do not aggregate signatures.
            return;
        };
        if self.verifiers.is_empty() {
            return;
        }

        let validator_index = vote.validator_index;
        if validator_index < 0 || validator_index as usize >= self.verifiers.len() {
            return;
        }

        let ctx = match self.contexts.get_mut(&vote.root_index) {
            Some(ctx) => ctx,
            None => return,
        };

        if ctx.signatures.len() >= required_signatures(self.verifiers.len()) {
            return;
        }
        if ctx.signatures.contains_key(&validator_index) {
            return;
        }

        let validator_key = self.verifiers[validator_index as usize].clone();
        let Ok(encoded_key) = validator_key.encode_point(true) else {
            return;
        };

        let Some(sign_data) = state_root_sign_data(&ctx.state_root, self.network_magic) else {
            return;
        };

        if vote.signature.len() != 64
            || !Crypto::verify_signature_bytes(&sign_data, &vote.signature, &encoded_key)
        {
            debug!(
                target: "neo::runtime",
                from,
                root_index = vote.root_index,
                validator_index,
                "invalid StateService vote signature"
            );
            return;
        }

        ctx.signatures.insert(validator_index, vote.signature);
        info!(
            target: "neo::runtime",
            from,
            root_index = vote.root_index,
            validator_index,
            votes = ctx.signatures.len(),
            "state service vote accepted"
        );

        self.maybe_broadcast_state_root(vote.root_index, &signer);
    }

    fn maybe_broadcast_state_root(&mut self, root_index: u32, signer: &StateServiceSigner) {
        let (state_root, signatures) = {
            let Some(ctx) = self.contexts.get_mut(&root_index) else {
                return;
            };

            if ctx.root_broadcasted {
                return;
            }

            let required = required_signatures(self.verifiers.len());
            if ctx.signatures.len() < required {
                return;
            }

            let sender = sender_index(ctx.root_index, ctx.retries, self.verifiers.len());
            if signer.my_index != sender {
                return;
            }

            ctx.root_broadcasted = true;
            (ctx.state_root.clone(), ctx.signatures.clone())
        };

        if let Some(state_root_payload) = self.build_state_root_payload_for(signer, state_root, signatures) {
            self.broadcast(state_root_payload);
        }
    }

    fn build_vote_payload_for(
        &self,
        signer: &StateServiceSigner,
        root_index: u32,
        signature: Vec<u8>,
    ) -> Option<ExtensiblePayload> {
        let vote = Vote {
            validator_index: signer.my_index,
            root_index,
            signature,
        };

        let mut writer = BinaryWriter::new();
        writer.write_u8(STATE_SERVICE_MESSAGE_VOTE).ok()?;
        vote.serialize(&mut writer).ok()?;
        let data = writer.into_bytes();

        self.build_extensible_payload(
            signer,
            root_index,
            root_index.saturating_add(MAX_CACHED_VERIFICATION_PROCESS_COUNT),
            data,
        )
    }

    fn build_state_root_payload_for(
        &self,
        signer: &StateServiceSigner,
        mut root: StateRoot,
        signatures: HashMap<i32, Vec<u8>>,
    ) -> Option<ExtensiblePayload> {
        root.witness = Some(build_state_root_witness(&self.verifiers, &signatures)?);

        let mut writer = BinaryWriter::new();
        writer.write_u8(STATE_SERVICE_MESSAGE_STATE_ROOT).ok()?;
        root.serialize(&mut writer).ok()?;
        let data = writer.into_bytes();

        self.build_extensible_payload(
            signer,
            root.index,
            root.index.saturating_add(MAX_VALID_UNTIL_BLOCK_INCREMENT),
            data,
        )
    }

    fn build_extensible_payload(
        &self,
        signer: &StateServiceSigner,
        valid_block_start: u32,
        valid_block_end: u32,
        data: Vec<u8>,
    ) -> Option<ExtensiblePayload> {
        let mut payload = ExtensiblePayload::new();
        payload.category = STATE_SERVICE_CATEGORY.to_string();
        payload.valid_block_start = valid_block_start;
        payload.valid_block_end = valid_block_end;
        payload.sender = signer.sender;
        payload.data = data;

        let signature = sign_extensible_payload(&mut payload, self.network_magic, &signer.key_pair)?;
        let mut invocation = Vec::with_capacity(signature.len() + 2);
        invocation.push(neo_vm::op_code::OpCode::PUSHDATA1 as u8);
        invocation.push(signature.len() as u8);
        invocation.extend_from_slice(&signature);
        payload.witness = Witness::new_with_scripts(invocation, signer.verification_script.clone());
        Some(payload)
    }

    fn verify_envelope(&self, payload: &mut ExtensiblePayload, current_height: u32) -> bool {
        if current_height < payload.valid_block_start || current_height >= payload.valid_block_end {
            return false;
        }

        if !self.allowed_senders.contains(&payload.sender) {
            return false;
        }

        // Signature-contract witness verification (fast path).
        if !ContractHelper::is_signature_contract(payload.witness.verification_script()) {
            return false;
        }
        if payload.witness.script_hash() != payload.sender {
            return false;
        }

        let signature = extract_signature_from_invocation_script(payload.witness.invocation_script());
        let Some(signature) = signature else {
            return false;
        };

        let pubkey = payload.witness.verification_script()[2..35].to_vec();
        let hash = payload.hash();
        let mut sign_data = Vec::with_capacity(36);
        sign_data.extend_from_slice(&self.network_magic.to_le_bytes());
        sign_data.extend_from_slice(&hash.as_bytes());

        Crypto::verify_signature_bytes(&sign_data, &signature, &pubkey)
    }

    fn broadcast(&self, payload: ExtensiblePayload) {
        let Some(tx) = &self.p2p_broadcast_tx else {
            return;
        };
        let _ = tx.send(BroadcastMessage {
            message: ProtocolMessage::Extensible(payload),
        });
    }

    fn evict_old_contexts(&mut self) {
        while self.contexts.len() > MAX_CACHED_VERIFICATION_PROCESS_COUNT as usize {
            let Some((&first, _)) = self.contexts.iter().next() else {
                break;
            };
            self.contexts.remove(&first);
        }
    }
}

fn sender_index(root_index: u32, retries: i32, count: usize) -> i32 {
    if count == 0 {
        return -1;
    }
    let raw = (root_index as i64) - (retries as i64);
    let n = count as i64;
    let idx = ((raw % n) + n) % n;
    idx as i32
}

fn required_signatures(n: usize) -> usize {
    n.saturating_sub((n.saturating_sub(1)) / 3)
}

fn sign_state_root(root: &StateRoot, key_pair: &KeyPair, network_magic: u32) -> Option<Vec<u8>> {
    let sign_data = state_root_sign_data(root, network_magic)?;
    key_pair.sign(&sign_data).ok()
}

fn state_root_sign_data(root: &StateRoot, network_magic: u32) -> Option<Vec<u8>> {
    let mut hashable = root.clone();
    let hash = hashable.hash();
    let mut data = Vec::with_capacity(36);
    data.extend_from_slice(&network_magic.to_le_bytes());
    data.extend_from_slice(&hash.as_bytes());
    Some(data)
}

fn sign_extensible_payload(
    payload: &mut ExtensiblePayload,
    network_magic: u32,
    key_pair: &KeyPair,
) -> Option<Vec<u8>> {
    let hash = payload.hash();
    let mut sign_data = Vec::with_capacity(36);
    sign_data.extend_from_slice(&network_magic.to_le_bytes());
    sign_data.extend_from_slice(&hash.as_bytes());
    key_pair.sign(&sign_data).ok()
}

fn extract_signature_from_invocation_script(script: &[u8]) -> Option<Vec<u8>> {
    if script.len() == 66 && script[0] == neo_vm::op_code::OpCode::PUSHDATA1 as u8 && script[1] == 64
    {
        return Some(script[2..66].to_vec());
    }
    None
}

fn build_state_root_witness(
    verifiers: &[ECPoint],
    signatures: &HashMap<i32, Vec<u8>>,
) -> Option<Witness> {
    if verifiers.is_empty() {
        return None;
    }

    let required = required_signatures(verifiers.len());
    if signatures.len() < required {
        return None;
    }

    // Build verification script (Contract.CreateMultiSigRedeemScript sorts keys internally).
    let verification_script = Contract::create_multi_sig_redeem_script(required, verifiers);

    // Build invocation script with exactly `required` signatures, in sorted key order.
    let mut sorted_keys = verifiers.to_vec();
    sorted_keys.sort();

    let mut signature_by_key: HashMap<ECPoint, Vec<u8>> = HashMap::new();
    for (idx, sig) in signatures {
        if *idx < 0 {
            continue;
        }
        let idx = *idx as usize;
        if idx >= verifiers.len() {
            continue;
        }
        signature_by_key.insert(verifiers[idx].clone(), sig.clone());
    }

    let mut invocation = Vec::new();
    let mut added = 0usize;
    for key in sorted_keys {
        if added >= required {
            break;
        }
        let Some(sig) = signature_by_key.get(&key) else {
            continue;
        };
        if sig.len() != 64 {
            continue;
        }
        invocation.push(neo_vm::op_code::OpCode::PUSHDATA1 as u8);
        invocation.push(64u8);
        invocation.extend_from_slice(sig);
        added += 1;
    }

    if added != required {
        return None;
    }

    Some(Witness::new_with_scripts(invocation, verification_script))
}

pub(crate) fn build_state_service_signer(
    validator_index: Option<u8>,
    private_key: &[u8],
    verifiers: &[ECPoint],
) -> Option<StateServiceSigner> {
    if private_key.is_empty() || verifiers.is_empty() {
        return None;
    }

    let key_pair = KeyPair::from_private_key(private_key).ok()?;
    let public_key = key_pair.get_public_key_point().ok()?;

    let my_index = match verifiers.iter().position(|k| *k == public_key) {
        Some(i) => i,
        None => {
            warn!(
                target: "neo::runtime",
                "provided private key is not a designated state validator; state service signing disabled"
            );
            return None;
        }
    };

    if let Some(expected) = validator_index {
        if expected as usize != my_index {
            warn!(
                target: "neo::runtime",
                expected_index = expected,
                actual_index = my_index,
                "consensus validator index differs from state validator index; using state validator index"
            );
        }
    }

    let verification_script = Contract::create_signature_redeem_script(public_key.clone());
    let sender = Contract::create_signature_contract(public_key.clone()).script_hash();

    Some(StateServiceSigner {
        my_index: my_index as i32,
        key_pair,
        sender,
        verification_script,
    })
}
