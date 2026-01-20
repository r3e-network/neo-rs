use crate::messages::ConsensusPayload;
use crate::{ConsensusEvent, ConsensusService};
use crate::{ConsensusMessageType, ConsensusResult, ValidatorInfo};
use neo_crypto::{Crypto, ECCurve, ECPoint, Secp256r1Crypto};
use neo_primitives::{UInt160, UInt256};
use tokio::sync::mpsc;

pub(super) fn create_test_validators(count: usize) -> Vec<ValidatorInfo> {
    (0..count)
        .map(|i| ValidatorInfo {
            index: i as u8,
            public_key: ECPoint::infinity(neo_crypto::ECCurve::Secp256r1),
            script_hash: neo_primitives::UInt160::zero(),
        })
        .collect()
}

pub(super) fn script_hash_from_pubkey(pubkey: &[u8]) -> UInt160 {
    let mut script = Vec::with_capacity(pubkey.len() + 6);
    script.push(0x0C);
    script.push(pubkey.len() as u8);
    script.extend_from_slice(pubkey);
    script.push(0x41);
    let syscall_hash = Crypto::sha256(b"System.Crypto.CheckSig");
    script.extend_from_slice(&syscall_hash[..4]);
    UInt160::from_bytes(&Crypto::hash160(&script)).expect("script hash")
}

pub(super) fn create_validators_with_keys(count: usize) -> (Vec<ValidatorInfo>, Vec<[u8; 32]>) {
    let mut validators = Vec::with_capacity(count);
    let mut private_keys = Vec::with_capacity(count);
    for i in 0..count {
        let key = [i as u8 + 1; 32];
        let pubkey = Secp256r1Crypto::derive_public_key(&key).expect("pubkey");
        let point = ECPoint::new(ECCurve::Secp256r1, pubkey.clone()).expect("ecpoint");
        let script_hash = script_hash_from_pubkey(&pubkey);
        validators.push(ValidatorInfo {
            index: i as u8,
            public_key: point,
            script_hash,
        });
        private_keys.push(key);
    }
    (validators, private_keys)
}

pub(super) fn sign_payload(
    service: &ConsensusService,
    payload: &mut ConsensusPayload,
    private_key: &[u8; 32],
) {
    let sign_data = service.dbft_sign_data(payload).expect("sign data");
    let signature = Secp256r1Crypto::sign(&sign_data, private_key).expect("sign");
    payload.set_witness(signature.to_vec());
}

pub(super) fn sign_commit(network: u32, block_hash: &UInt256, private_key: &[u8; 32]) -> Vec<u8> {
    let mut sign_data = Vec::with_capacity(4 + 32);
    sign_data.extend_from_slice(&network.to_le_bytes());
    sign_data.extend_from_slice(&block_hash.as_bytes());
    Secp256r1Crypto::sign(&sign_data, private_key)
        .expect("sign")
        .to_vec()
}

pub(super) struct PersistCompletedHarness {
    services: Vec<ConsensusService>,
    receivers: Vec<mpsc::Receiver<ConsensusEvent>>,
    events: Vec<ConsensusEvent>,
}

impl PersistCompletedHarness {
    pub(super) fn new(network: u32, validator_count: usize) -> Self {
        let (validators, keys) = create_validators_with_keys(validator_count);
        let mut services = Vec::with_capacity(validator_count);
        let mut receivers = Vec::with_capacity(validator_count);

        for i in 0..validator_count {
            let (tx, rx) = mpsc::channel(256);
            let service = ConsensusService::new(
                network,
                validators.clone(),
                Some(i as u8),
                keys[i].to_vec(),
                tx,
            );
            services.push(service);
            receivers.push(rx);
        }

        Self {
            services,
            receivers,
            events: Vec::new(),
        }
    }

    pub(super) fn persist_completed_all(
        &mut self,
        block_index: u32,
        prev_hash: UInt256,
        timestamp: u64,
    ) -> ConsensusResult<()> {
        let next_index = block_index + 1;
        for service in &mut self.services {
            service.start(next_index, timestamp, prev_hash, 0)?;
        }
        Ok(())
    }

    pub(super) fn take_events(&mut self) -> Vec<ConsensusEvent> {
        std::mem::take(&mut self.events)
    }

    pub(super) async fn drive_until_idle(&mut self, max_iters: usize) -> ConsensusResult<()> {
        for _ in 0..max_iters {
            if !self.drive_once()? {
                break;
            }
        }
        Ok(())
    }

    pub(super) fn drive_once(&mut self) -> ConsensusResult<bool> {
        let mut progressed = false;
        for idx in 0..self.services.len() {
            while let Ok(event) = self.receivers[idx].try_recv() {
                progressed = true;
                self.handle_event(idx, event)?;
            }
        }
        Ok(progressed)
    }

    pub(super) fn handle_event(
        &mut self,
        sender_index: usize,
        event: ConsensusEvent,
    ) -> ConsensusResult<()> {
        match &event {
            ConsensusEvent::RequestTransactions { .. } => {
                self.services[sender_index].on_transactions_received(Vec::new())?;
            }
            ConsensusEvent::BroadcastMessage(payload) => {
                for (idx, service) in self.services.iter_mut().enumerate() {
                    if idx == sender_index {
                        continue;
                    }
                    service.process_message(payload.clone())?;
                }
            }
            ConsensusEvent::BlockCommitted { .. } | ConsensusEvent::ViewChanged { .. } => {}
        }

        self.events.push(event);
        Ok(())
    }

    pub(super) fn saw_prepare_request(&self, block_index: u32) -> bool {
        self.events.iter().any(|event| match event {
            ConsensusEvent::BroadcastMessage(payload) => {
                payload.message_type == ConsensusMessageType::PrepareRequest
                    && payload.block_index == block_index
            }
            _ => false,
        })
    }

    pub(super) fn saw_block_committed(&self, block_index: u32) -> bool {
        self.events.iter().any(|event| match event {
            ConsensusEvent::BlockCommitted {
                block_index: committed_index,
                ..
            } => *committed_index == block_index,
            _ => false,
        })
    }
}
