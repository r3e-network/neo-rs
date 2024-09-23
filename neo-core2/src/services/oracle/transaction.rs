use std::collections::HashMap;
use std::sync::RwLock;
use std::time::SystemTime;

use crate::config::netmode::Magic;
use crate::core::state::OracleRequest;
use crate::core::transaction::Transaction;
use crate::crypto::hash;
use crate::crypto::keys::PublicKey;
use crate::io;
use crate::smartcontract;
use crate::vm::emit;

struct IncompleteTx {
    lock: RwLock<()>,
    // isSent is true if tx has been already broadcasted.
    is_sent: bool,
    // attempts is how many times the request was processed.
    attempts: i32,
    // time is the time when the request was last processed.
    time: SystemTime,
    // request is an oracle request.
    request: Option<OracleRequest>,
    // tx is an oracle response transaction.
    tx: Option<Transaction>,
    // sigs contains a signature from every oracle node.
    sigs: HashMap<String, TxSignature>,
    // backupTx is a backup transaction.
    backup_tx: Option<Transaction>,
    // backupSigs contains signatures of backup tx.
    backup_sigs: HashMap<String, TxSignature>,
}

struct TxSignature {
    // pub is a cached public key.
    pub: PublicKey,
    // ok is true if the signature was verified.
    ok: bool,
    // sig is tx signature.
    sig: Vec<u8>,
}

impl IncompleteTx {
    fn new() -> Self {
        IncompleteTx {
            lock: RwLock::new(()),
            is_sent: false,
            attempts: 0,
            time: SystemTime::now(),
            request: None,
            tx: None,
            sigs: HashMap::new(),
            backup_tx: None,
            backup_sigs: HashMap::new(),
        }
    }

    fn reverify_tx(&self, net: Magic) {
        let tx_hash = hash::net_sha256(net as u32, self.tx.as_ref().unwrap());
        let backup_hash = hash::net_sha256(net as u32, self.backup_tx.as_ref().unwrap());
        for (pub_key, sig) in &self.sigs {
            if !sig.ok {
                sig.ok = sig.pub.verify(&sig.sig, &tx_hash.bytes_be());
                if !sig.ok && sig.pub.verify(&sig.sig, &backup_hash.bytes_be()) {
                    self.backup_sigs.insert(pub_key.clone(), TxSignature {
                        pub: sig.pub.clone(),
                        ok: true,
                        sig: sig.sig.clone(),
                    });
                }
            }
        }
    }

    fn add_response(&mut self, pub: PublicKey, sig: Vec<u8>, is_backup: bool) {
        let (tx, sigs) = if is_backup {
            (&self.backup_tx, &mut self.backup_sigs)
        } else {
            (&self.tx, &mut self.sigs)
        };
        sigs.insert(pub.bytes().to_vec(), TxSignature {
            pub,
            ok: tx.is_some(),
            sig,
        });
    }

    // finalize checks if either main or backup tx has sufficient number of signatures and returns
    // tx and bool value indicating if it is ready to be broadcasted.
    fn finalize(&self, oracle_nodes: Vec<PublicKey>, backup_only: bool) -> (Option<Transaction>, bool) {
        if !backup_only && finalize_tx(&oracle_nodes, self.tx.as_ref(), &self.sigs) {
            return (self.tx.clone(), true);
        }
        (self.backup_tx.clone(), finalize_tx(&oracle_nodes, self.backup_tx.as_ref(), &self.backup_sigs))
    }
}

fn finalize_tx(oracle_nodes: &[PublicKey], tx: Option<&Transaction>, tx_sigs: &HashMap<String, TxSignature>) -> bool {
    if tx.is_none() {
        return false;
    }
    let tx = tx.unwrap();
    let m = smartcontract::get_default_honest_node_count(oracle_nodes.len());
    let mut sigs = Vec::with_capacity(m);
    for pub_key in oracle_nodes {
        if let Some(sig) = tx_sigs.get(&pub_key.bytes().to_vec()) {
            if sig.ok {
                sigs.push(sig.sig.clone());
                if sigs.len() == m {
                    break;
                }
            }
        }
    }
    if sigs.len() != m {
        return false;
    }

    let mut w = io::BufBinWriter::new();
    for sig in sigs {
        emit::bytes(&mut w, &sig);
    }
    tx.scripts[1].invocation_script = w.bytes();
    true
}
