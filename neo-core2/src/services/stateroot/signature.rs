use std::sync::{RwLock, Arc};
use std::collections::HashMap;

use neo_core2::config::netmode::Magic;
use neo_core2::core::state::MPTRoot;
use neo_core2::core::transaction::Witness;
use neo_core2::crypto::keys::{PublicKey, PublicKeys};
use neo_core2::io::BufBinWriter;
use neo_core2::network::payload::Extensible;
use neo_core2::smartcontract;
use neo_core2::vm::emit;

pub struct IncompleteRoot {
    sv_list: PublicKeys,
    is_sent: bool,
    root: Option<Arc<MPTRoot>>,
    sigs: RwLock<HashMap<Vec<u8>, RootSig>>,
    my_index: usize,
    my_vote: Option<Extensible>,
    retries: i32,
}

struct RootSig {
    pub: PublicKey,
    ok: bool,
    sig: Vec<u8>,
}

impl IncompleteRoot {
    pub fn reverify(&self, net: Magic) {
        let mut sigs = self.sigs.write().unwrap();
        for sig in sigs.values_mut() {
            if !sig.ok {
                sig.ok = sig.pub.verify_hashable(&sig.sig, net as u32, self.root.as_ref().unwrap());
            }
        }
    }

    pub fn add_signature(&self, pub_key: &PublicKey, sig: Vec<u8>) {
        let mut sigs = self.sigs.write().unwrap();
        sigs.insert(pub_key.to_bytes(), RootSig {
            pub: pub_key.clone(),
            ok: self.root.is_some(),
            sig,
        });
    }

    pub fn is_sender_now(&self) -> bool {
        if self.root.is_none() || self.is_sent || self.sv_list.is_empty() {
            return false;
        }
        let retries = self.retries.max(0) as usize;
        let ind = (self.root.as_ref().unwrap().index as usize - retries) % self.sv_list.len();
        ind == self.my_index
    }

    pub fn finalize(&self) -> Option<(Arc<MPTRoot>, bool)> {
        let root = self.root.as_ref()?;

        let m = smartcontract::get_default_honest_node_count(self.sv_list.len());
        let mut sigs = Vec::with_capacity(m);
        let sigs_map = self.sigs.read().unwrap();

        for pub_key in &self.sv_list {
            if let Some(sig) = sigs_map.get(&pub_key.to_bytes()) {
                if sig.ok {
                    sigs.push(sig.sig.clone());
                    if sigs.len() == m {
                        break;
                    }
                }
            }
        }

        if sigs.len() != m {
            return None;
        }

        let verif = smartcontract::create_default_multi_sig_redeem_script(&self.sv_list).ok()?;
        let mut w = BufBinWriter::new();
        for sig in &sigs {
            emit::bytes(&mut w, sig);
        }

        let mut root = (*root).clone();
        root.witness = vec![Witness {
            invocation_script: w.to_bytes(),
            verification_script: verif,
        }];

        Some((Arc::new(root), true))
    }
}
