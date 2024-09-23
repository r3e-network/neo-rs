use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::error::Error;

use neo_config::StateRoot as StateRootConfig;
use neo_core::state::MPTRoot;
use neo_core::transaction::Witness;
use neo_io::{BufBinWriter, BinWriter};
use neo_network::payload::Extensible;
use neo_vm::emit;
use neo_wallet::Account;

use crate::config;
use crate::core::state;
use crate::core::transaction;
use crate::io;
use crate::network::payload;
use crate::vm::emit;
use crate::wallet;

const ROOT_VALID_END_INC: u32 = 100;

type RelayCallback = Box<dyn Fn(&Extensible) + Send + Sync>;

pub struct Service {
    main_cfg: StateRootConfig,
    network: u32,
    incomplete_roots: Arc<Mutex<HashMap<u32, IncompleteRoot>>>,
    relay_extensible: RelayCallback,
    log: slog::Logger,
}

impl Service {
    pub fn add_signature(&self, height: u32, validator_index: i32, sig: Vec<u8>) -> Result<(), Box<dyn Error>> {
        if !self.main_cfg.enabled {
            return Ok(());
        }
        let (my_index, acc) = self.get_account();
        let acc = match acc {
            Some(a) => a,
            None => return Ok(()),
        };

        let inc_root = self.get_incomplete_root(height, my_index);
        let mut inc_root = inc_root.lock().unwrap();

        if validator_index < 0 || validator_index as usize >= inc_root.sv_list.len() {
            return Err("invalid validator index".into());
        }

        let pub_key = &inc_root.sv_list[validator_index as usize];
        if let Some(root) = &inc_root.root {
            if !pub_key.verify_hashable(&sig, self.network, root) {
                return Err(format!("invalid state root signature for {}", validator_index).into());
            }
        }
        inc_root.add_signature(pub_key, sig);
        self.try_send_root(&mut inc_root, &acc);
        Ok(())
    }

    pub fn get_config(&self) -> StateRootConfig {
        self.main_cfg.clone()
    }

    fn get_incomplete_root(&self, height: u32, my_index: u8) -> Arc<Mutex<IncompleteRoot>> {
        let mut roots = self.incomplete_roots.lock().unwrap();
        roots.entry(height).or_insert_with(|| {
            Arc::new(Mutex::new(IncompleteRoot {
                my_index: my_index as i32,
                sv_list: self.get_state_validators(height),
                sigs: HashMap::new(),
                root: None,
                is_sent: false,
            }))
        }).clone()
    }

    fn try_send_root(&self, ir: &mut IncompleteRoot, acc: &Account) {
        if !ir.is_sender_now() {
            return;
        }
        if let Some((sr, ready)) = ir.finalize() {
            if ready {
                if let Err(e) = self.add_state_root(&sr) {
                    self.log.error("can't add validated state root", slog::o!("error" => e.to_string()));
                }
                self.send_validated_root(&sr, acc);
                ir.is_sent = true;
            }
        }
    }

    fn send_validated_root(&self, r: &MPTRoot, acc: &Account) {
        let mut w = BufBinWriter::new();
        let m = Message::new(MessageType::Root, r);
        m.encode_binary(&mut w);
        let ep = Extensible {
            category: Category::StateRoot,
            valid_block_start: r.index,
            valid_block_end: r.index + ROOT_VALID_END_INC,
            sender: acc.script_hash(),
            data: w.to_vec(),
            witness: Witness {
                verification_script: acc.get_verification_script(),
                invocation_script: Vec::new(),
            },
        };
        let sig = acc.sign_hashable(self.network, &ep);
        let mut buf = BufBinWriter::new();
        emit::bytes(&mut buf, &sig);
        ep.witness.invocation_script = buf.to_vec();
        (self.relay_extensible)(&ep);
    }
}
