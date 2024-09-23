use hex;
use log::{info, error};
use std::time::Duration;
use tokio::runtime::Runtime;
use serde::{Serialize, Deserialize};
use neon::config::{BlockchainConfig, ProtocolConfiguration, NetMode};
use neon::core::{Blockchain, MemoryStore};
use neon::crypto::keys::{PublicKey, PrivateKey};
use neon::wallet::Account;
use neon::smartcontract;
use neon::neotest::Signer;

const MAX_TRACEABLE_BLOCKS: u32 = 1000;
const TIME_PER_BLOCK: Duration = Duration::from_secs(1);
const SINGLE_VALIDATOR_WIF: &str = "KxyjQ8eUa4FHt3Gvioyt1Wz29cTUrE4eTqX3yFSk1YFCsPL8uNsY";

const COMMITTEE_WIFS: [&str; 6] = [
    "KzfPUYDC9n2yf4fK5ro4C8KMcdeXtFuEnStycbZgX3GomiUsvX6W",
    "KzgWE3u3EDp13XPXXuTKZxeJ3Gi8Bsm8f9ijY3ZsCKKRvZUo1Cdn",
    SINGLE_VALIDATOR_WIF,
    "L2oEXKRAAMiPEZukwR5ho2S6SMeQLhcK9mF71ZnF7GvT8dU4Kkgz",
    "L1Tr1iq5oz1jaFaMXP21sHDkJYDDkuLtpvQ4wRf1cjKvJYvnvpAb",
    "Kz6XTUrExy78q8f4MjDHnwz8fYYyUE8iPXwPRAkHa3qN2JcHYm7e",
];

struct Options {
    logger: Option<log::Logger>,
    blockchain_config_hook: Option<Box<dyn Fn(&mut BlockchainConfig)>>,
    store: Option<MemoryStore>,
    skip_run: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            logger: None,
            blockchain_config_hook: None,
            store: None,
            skip_run: false,
        }
    }
}

fn init() {
    let committee_acc = Account::from_wif(SINGLE_VALIDATOR_WIF).unwrap();
    let pubs = vec![committee_acc.public_key()];
    committee_acc.convert_multisig(1, &pubs).unwrap();

    let mc = smartcontract::get_majority_honest_node_count(COMMITTEE_WIFS.len());
    let mv = smartcontract::get_default_honest_node_count(4);
    let mut accs: Vec<Account> = Vec::with_capacity(COMMITTEE_WIFS.len());
    let mut pubs: Vec<PublicKey> = Vec::with_capacity(COMMITTEE_WIFS.len());

    for wif in COMMITTEE_WIFS.iter() {
        let acc = Account::from_wif(wif).unwrap();
        pubs.push(acc.public_key());
        accs.push(acc);
    }

    // Config entry must contain validators first in a specific order.
    let mut stand_by_committee: Vec<String> = pubs.iter().map(|pk| pk.to_string_compressed()).collect();
    stand_by_committee.sort();

    let mut multi_validator_acc: Vec<Account> = Vec::with_capacity(4);
    pubs.sort_by(|a, b| a.cmp(b));
    accs.sort_by(|a, b| a.public_key().cmp(&b.public_key()));

    for i in 0..4 {
        let acc = Account::from_private_key(accs[i].private_key());
        acc.convert_multisig(mv, &pubs[..4]).unwrap();
        multi_validator_acc.push(acc);
    }

    let mut multi_committee_acc: Vec<Account> = Vec::with_capacity(COMMITTEE_WIFS.len());
    pubs.sort_by(|a, b| a.cmp(b));
    accs.sort_by(|a, b| a.public_key().cmp(&b.public_key()));

    for acc in accs.iter() {
        let acc = Account::from_private_key(acc.private_key());
        acc.convert_multisig(mc, &pubs).unwrap();
        multi_committee_acc.push(acc);
    }
}

fn new_single(t: &mut dyn std::fmt::Debug) -> (Blockchain, Signer) {
    new_single_with_custom_config(t, None)
}

fn new_single_with_custom_config(t: &mut dyn std::fmt::Debug, f: Option<Box<dyn Fn(&mut BlockchainConfig)>>) -> (Blockchain, Signer) {
    new_single_with_custom_config_and_store(t, f, None, true)
}

fn new_single_with_custom_config_and_store(t: &mut dyn std::fmt::Debug, f: Option<Box<dyn Fn(&mut BlockchainConfig)>>, st: Option<MemoryStore>, run: bool) -> (Blockchain, Signer) {
    new_single_with_options(t, Options {
        blockchain_config_hook: f,
        store: st,
        skip_run: !run,
        ..Default::default()
    })
}

fn new_single_with_options(t: &mut dyn std::fmt::Debug, options: Options) -> (Blockchain, Signer) {
    let mut cfg = BlockchainConfig {
        protocol_configuration: ProtocolConfiguration {
            magic: NetMode::UnitTestNet,
            max_traceable_blocks: MAX_TRACEABLE_BLOCKS,
            time_per_block: TIME_PER_BLOCK,
            standby_committee: vec![hex::encode(committee_acc.public_key().bytes())],
            validators_count: 1,
            verify_transactions: true,
        },
    };

    if let Some(hook) = options.blockchain_config_hook {
        hook(&mut cfg);
    }

    let store = options.store.unwrap_or_else(|| MemoryStore::new());
    let logger = options.logger.unwrap_or_else(|| log::Logger::new(t));

    let bc = Blockchain::new(store, cfg, logger).unwrap();
    if !options.skip_run {
        let rt = Runtime::new().unwrap();
        rt.spawn(async move {
            bc.run().await;
        });
        t.cleanup(|| bc.close());
    }
    (bc, Signer::new_multi(committee_acc))
}

// Similar functions for NewMulti, NewMultiWithCustomConfig, etc. would follow the same pattern.
