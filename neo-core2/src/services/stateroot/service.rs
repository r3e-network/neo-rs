use std::collections::HashMap;
use std::sync::{Arc, RwLock, Mutex};
use std::time::Duration;
use std::sync::atomic::{AtomicBool, Ordering};

use neo_config::{Config, StateRootConfig, BlockchainConfig};
use neo_core::{block::Block, state::MPTRoot};
use neo_crypto::keys::PublicKey;
use neo_io::BinReader;
use neo_network::payload::Extensible;
use neo_wallet::Wallet;

use crate::core::native::noderoles::Role;
use crate::core::state::stateroot::{Module, Error as StateRootError};
use crate::network::payload::Message;

pub const CATEGORY: &str = "StateService";

pub trait Ledger: Send + Sync {
    fn get_config(&self) -> BlockchainConfig;
    fn get_designated_by_role(&self, role: Role) -> Result<(Vec<PublicKey>, u32), Box<dyn std::error::Error>>;
    fn header_height(&self) -> u32;
    fn subscribe_for_blocks(&self, tx: tokio::sync::mpsc::Sender<Arc<Block>>);
    fn unsubscribe_from_blocks(&self, tx: tokio::sync::mpsc::Sender<Arc<Block>>);
}

pub trait Service: Send + Sync {
    fn name(&self) -> String;
    fn on_payload(&self, p: &Extensible) -> Result<(), Box<dyn std::error::Error>>;
    fn add_signature(&self, height: u32, validator_index: i32, sig: Vec<u8>) -> Result<(), Box<dyn std::error::Error>>;
    fn get_config(&self) -> StateRootConfig;
    fn start(&self);
    fn shutdown(&self);
    fn is_authorized(&self) -> bool;
}

pub struct StateRootService {
    module: Arc<Module>,
    chain: Arc<dyn Ledger>,
    main_cfg: StateRootConfig,
    network: u32,
    log: slog::Logger,
    started: AtomicBool,
    acc_data: RwLock<(u32, Option<u8>)>,
    wallet: Arc<Wallet>,
    incomplete_roots: Mutex<HashMap<u32, IncompleteRoot>>,
    time_per_block: Duration,
    max_retries: i32,
    relay_extensible: Box<dyn Fn(Extensible) -> Result<(), Box<dyn std::error::Error>> + Send + Sync>,
    block_rx: tokio::sync::mpsc::Receiver<Arc<Block>>,
    stop_tx: tokio::sync::mpsc::Sender<()>,
}

impl StateRootService {
    pub fn new(
        cfg: StateRootConfig,
        sm: Arc<Module>,
        log: slog::Logger,
        bc: Arc<dyn Ledger>,
        cb: impl Fn(Extensible) -> Result<(), Box<dyn std::error::Error>> + Send + Sync + 'static,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let bc_conf = bc.get_config();
        
        if cfg.enabled && bc_conf.state_root_in_header {
            return Err("StateRootInHeader should be disabled when state service is enabled".into());
        }

        let wallet = if cfg.enabled {
            let w = Wallet::new_from_file(&cfg.unlock_wallet.path)?;
            if !w.accounts().iter().any(|acc| acc.decrypt(&cfg.unlock_wallet.password, &w.scrypt()).is_ok()) {
                return Err("no wallet account could be unlocked".into());
            }
            Arc::new(w)
        } else {
            Arc::new(Wallet::default())
        };

        let (block_tx, block_rx) = tokio::sync::mpsc::channel(1);
        let (stop_tx, _) = tokio::sync::mpsc::channel(1);

        let mut service = Self {
            module: sm,
            network: bc_conf.magic,
            chain: bc,
            log,
            main_cfg: cfg,
            started: AtomicBool::new(false),
            acc_data: RwLock::new((0, None)),
            wallet,
            incomplete_roots: Mutex::new(HashMap::new()),
            time_per_block: bc_conf.time_per_block,
            max_retries: 20, // Assuming voteValidEndInc is 20
            relay_extensible: Box::new(cb),
            block_rx,
            stop_tx,
        };

        if service.main_cfg.enabled {
            let (keys, height) = service.chain.get_designated_by_role(Role::StateValidator)?;
            service.update_validators(height, keys);
            service.module.set_update_validators_callback(Box::new(move |h, k| service.update_validators(h, k)));
        }

        Ok(service)
    }

    fn update_validators(&self, height: u32, pubs: Vec<PublicKey>) {
        let mut acc_data = self.acc_data.write().unwrap();
        *acc_data = (height, None);

        for (i, pub_key) in pubs.iter().enumerate() {
            if let Some(acc) = self.wallet.get_account(&pub_key.get_script_hash()) {
                if acc.decrypt(&self.main_cfg.unlock_wallet.password, &self.wallet.scrypt()).is_ok() {
                    *acc_data = (height, Some(i as u8));
                    break;
                }
            }
        }
    }
}

impl Service for StateRootService {
    fn name(&self) -> String {
        "StateRootService".to_string()
    }

    fn on_payload(&self, ep: &Extensible) -> Result<(), Box<dyn std::error::Error>> {
        let mut reader = BinReader::new(&ep.data);
        let message = Message::decode_binary(&mut reader)?;

        match message {
            Message::Root(sr) if sr.index != 0 => {
                self.module.add_state_root(&sr).map_err(|e| {
                    if let StateRootError::StateMismatch = e {
                        slog::error!(self.log, "can't add SV-signed state root"; "error" => %e);
                    }
                    e
                })?;

                let mut incomplete_roots = self.incomplete_roots.lock().unwrap();
                if let Some(ir) = incomplete_roots.get_mut(&sr.index) {
                    ir.is_sent = true;
                }
                Ok(())
            }
            Message::Vote(v) => self.module.add_signature(v.height, v.validator_index, v.signature),
            _ => Ok(()),
        }
    }

    fn add_signature(&self, height: u32, validator_index: i32, sig: Vec<u8>) -> Result<(), Box<dyn std::error::Error>> {
        self.module.add_signature(height, validator_index, sig)
    }

    fn get_config(&self) -> StateRootConfig {
        self.main_cfg.clone()
    }

    fn start(&self) {
        let log = self.log.clone();
        let module = self.module.clone();
        let main_cfg = self.main_cfg.clone();
        let wallet = self.wallet.clone();
        let acc_data = self.acc_data.clone();
        let incomplete_roots = self.incomplete_roots.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(main_cfg.interval_ms));
            loop {
                interval.tick().await;
                if let Err(e) = Self::process_state_root(&module, &main_cfg, &wallet, &acc_data, &incomplete_roots, &log).await {
                    slog::error!(log, "Error processing state root"; "error" => %e);
                }
            }
        });

        slog::info!(self.log, "StateRootService started");
    }

    fn shutdown(&self) {
        // Signal the background task to stop
        if let Some(sender) = &self.shutdown_sender {
            let _ = sender.send(());
        }

        slog::info!(self.log, "StateRootService shutting down");
    }

    fn is_authorized(&self) -> bool {
        self.acc_data.read().unwrap().1.is_some()
    }
}
