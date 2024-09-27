use std::collections::HashMap;
use std::sync::Arc;

use crate::config::ProtocolConfiguration;
use crate::core::interop::{Contract, InteropInterface};
use crate::core::interop::interopnames;
use crate::io::BufBinWriter;
use crate::util::Uint160;
use crate::vm::emit;

// Contracts is a set of registered native contracts.
pub struct Contracts {
    management: Arc<Management>,
    ledger: Arc<Ledger>,
    neo: Arc<NEO>,
    gas: Arc<GAS>,
    policy: Arc<Policy>,
    oracle: Arc<Oracle>,
    designate: Arc<Designate>,
    notary: Option<Arc<Notary>>,
    crypto: Arc<Crypto>,
    std: Arc<Std>,
    contracts: Vec<Arc<dyn Contract>>,
    // persistScript is a vm script which executes "onPersist" method of every native contract.
    persist_script: Option<Vec<u8>>,
    // postPersistScript is a vm script which executes "postPersist" method of every native contract.
    post_persist_script: Option<Vec<u8>>,
}

impl Contracts {
    // ByHash returns a native contract with the specified hash.
    pub fn by_hash(&self, h: &Uint160) -> Option<Arc<dyn Contract>> {
        self.contracts.iter().find(|ctr| ctr.metadata().hash == *h).cloned()
    }

    // ByName returns a native contract with the specified name.
    pub fn by_name(&self, name: &str) -> Option<Arc<dyn Contract>> {
        let name = name.to_lowercase();
        self.contracts.iter().find(|ctr| ctr.metadata().name.to_lowercase() == name).cloned()
    }

    // NewContracts returns a new set of native contracts with new GAS, NEO, Policy, Oracle,
    // Designate and (optional) Notary contracts.
    pub fn new(cfg: &ProtocolConfiguration) -> Self {
        let mut cs = Contracts {
            management: Arc::new(Management::new()),
            ledger: Arc::new(Ledger::new()),
            neo: Arc::new(NEO::new(cfg)),
            gas: Arc::new(GAS::new(cfg.initial_gas_supply as i64, cfg.p2p_sig_extensions)),
            policy: Arc::new(Policy::new(cfg.p2p_sig_extensions)),
            oracle: Arc::new(Oracle::new()),
            designate: Arc::new(Designate::new(&cfg.genesis.roles)),
            notary: None,
            crypto: Arc::new(Crypto::new()),
            std: Arc::new(Std::new()),
            contracts: Vec::new(),
            persist_script: None,
            post_persist_script: None,
        };

        // Set up cross-references
        Arc::get_mut(&mut cs.neo).unwrap().gas = Arc::clone(&cs.gas);
        Arc::get_mut(&mut cs.neo).unwrap().policy = Arc::clone(&cs.policy);
        Arc::get_mut(&mut cs.gas).unwrap().neo = Arc::clone(&cs.neo);
        Arc::get_mut(&mut cs.gas).unwrap().policy = Arc::clone(&cs.policy);
        Arc::get_mut(&mut cs.management).unwrap().neo = Arc::clone(&cs.neo);
        Arc::get_mut(&mut cs.management).unwrap().policy = Arc::clone(&cs.policy);
        Arc::get_mut(&mut cs.policy).unwrap().neo = Arc::clone(&cs.neo);

        Arc::get_mut(&mut cs.designate).unwrap().neo = Arc::clone(&cs.neo);
        Arc::get_mut(&mut cs.oracle).unwrap().gas = Arc::clone(&cs.gas);
        Arc::get_mut(&mut cs.oracle).unwrap().neo = Arc::clone(&cs.neo);
        Arc::get_mut(&mut cs.oracle).unwrap().designate = Arc::clone(&cs.designate);

        if cfg.p2p_sig_extensions {
            let notary = Arc::new(Notary::new());
            Arc::get_mut(&mut notary).unwrap().gas = Arc::clone(&cs.gas);
            Arc::get_mut(&mut notary).unwrap().neo = Arc::clone(&cs.neo);
            Arc::get_mut(&mut notary).unwrap().designate = Arc::clone(&cs.designate);
            Arc::get_mut(&mut notary).unwrap().policy = Arc::clone(&cs.policy);
            cs.notary = Some(notary.clone());
            cs.contracts.push(notary);
        }

        cs.contracts.extend_from_slice(&[
            cs.management.clone(),
            cs.std.clone(),
            cs.crypto.clone(),
            cs.ledger.clone(),
            cs.gas.clone(),
            cs.neo.clone(),
            cs.policy.clone(),
            cs.designate.clone(),
            cs.oracle.clone(),
        ]);

        cs
    }

    // GetPersistScript returns a VM script calling "onPersist" syscall for native contracts.
    pub fn get_persist_script(&mut self) -> Vec<u8> {
        if let Some(script) = &self.persist_script {
            script.clone()
        } else {
            let mut w = BufBinWriter::new();
            emit::syscall(&mut w, interopnames::SYSTEM_CONTRACT_NATIVE_ON_PERSIST);
            let script = w.to_vec();
            self.persist_script = Some(script.clone());
            script
        }
    }

    // GetPostPersistScript returns a VM script calling "postPersist" syscall for native contracts.
    pub fn get_post_persist_script(&mut self) -> Vec<u8> {
        if let Some(script) = &self.post_persist_script {
            script.clone()
        } else {
            let mut w = BufBinWriter::new();
            emit::syscall(&mut w, interopnames::SYSTEM_CONTRACT_NATIVE_POST_PERSIST);
            let script = w.to_vec();
            self.post_persist_script = Some(script.clone());
            script
        }
    }
}
