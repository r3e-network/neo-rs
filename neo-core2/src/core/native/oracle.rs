use std::collections::HashMap;
use std::sync::atomic::AtomicPtr;
use std::sync::Arc;

use neo_core2::config::Hardfork;
use neo_core2::core::dao::{self, NativeContractCache};
use neo_core2::core::interop::{self, Contract, ContractMD};
use neo_core2::core::native::{gas::GAS, neo::NEO, designate::Designate};
use neo_core2::core::state::OracleRequest;
use neo_core2::crypto::keys::PublicKey;
use neo_core2::util::Uint160;

const ORACLE_CONTRACT_ID: i32 = -9;
const MAX_URL_LENGTH: usize = 256;
const MAX_FILTER_LENGTH: usize = 128;
const MAX_CALLBACK_LENGTH: usize = 32;
const MAX_USER_DATA_LENGTH: usize = 512;
const MAX_REQUESTS_COUNT: usize = 256;

const DEFAULT_ORACLE_REQUEST_PRICE: i64 = 5000_0000;
const MINIMUM_RESPONSE_GAS: i64 = 10_000_000;

pub struct Oracle {
    contract_md: ContractMD,
    gas: Arc<GAS>,
    neo: Arc<NEO>,
    desig: Arc<Designate>,
    oracle_script: Vec<u8>,
    module: AtomicPtr<dyn OracleService>,
    new_requests: HashMap<u64, OracleRequest>,
}

pub struct OracleCache {
    request_price: i64,
}

pub trait OracleService: Send + Sync {
    fn add_requests(&mut self, requests: HashMap<u64, OracleRequest>);
    fn remove_requests(&mut self, ids: Vec<u64>);
    fn update_oracle_nodes(&mut self, keys: Vec<PublicKey>);
    fn update_native_contract(&mut self, script: Vec<u8>, response_script: Vec<u8>, hash: Uint160, offset: i32);
    fn start(&mut self);
    fn shutdown(&mut self);
}

impl NativeContractCache for OracleCache {
    fn copy(&self) -> Box<dyn NativeContractCache> {
        Box::new(OracleCache {
            request_price: self.request_price,
        })
    }
}

impl Oracle {
    pub fn new() -> Self {
        let contract_md = ContractMD::new("Oracle", ORACLE_CONTRACT_ID);
        let oracle_script = Self::create_oracle_response_script(&contract_md.hash());

        let mut oracle = Oracle {
            contract_md,
            gas: Arc::new(GAS::new()),
            neo: Arc::new(NEO::new()),
            desig: Arc::new(Designate::new()),
            oracle_script,
            module: AtomicPtr::default(),
            new_requests: HashMap::new(),
        };

        oracle.build_methods();
        oracle
    }

    fn build_methods(&mut self) {
        // TODO: Implement method building logic
    }

    pub fn get_oracle_response_script(&self) -> Vec<u8> {
        self.oracle_script.clone()
    }

    // TODO: Implement other methods

    fn create_oracle_response_script(native_oracle_hash: &Uint160) -> Vec<u8> {
        // TODO: Implement script creation
        vec![]
    }
}

impl Contract for Oracle {
    fn metadata(&self) -> &ContractMD {
        &self.contract_md
    }

    fn initialize(&mut self, _ic: &mut interop::Context, _hf: &Hardfork, _new_md: &interop::HFSpecificContractMD) -> Result<(), Box<dyn std::error::Error>> {
        // TODO: Implement initialization
        Ok(())
    }

    fn on_persist(&self, _ic: &interop::Context) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn post_persist(&mut self, _ic: &mut interop::Context) -> Result<(), Box<dyn std::error::Error>> {
        // TODO: Implement post-persist logic
        Ok(())
    }

    fn active_in(&self) -> Option<Hardfork> {
        None
    }
}
