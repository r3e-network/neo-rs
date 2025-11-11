use alloc::vec::Vec;

use neo_base::hash::Hash160;
use neo_store::Store;
use neo_vm::Trigger;

use crate::runtime::gas::GasMeter;

use super::ExecutionContext;

impl<'a> ExecutionContext<'a> {
    pub fn new(store: &'a mut dyn Store, gas_limit: u64, signer: Option<Hash160>) -> Self {
        Self {
            store,
            gas: GasMeter::new(gas_limit),
            legacy_signer: signer,
            signers: Vec::new(),
            log: Vec::new(),
            notifications: Vec::new(),
            timestamp: 0,
            invocation_counter: 0,
            storage_context: Default::default(),
            script: Default::default(),
            current_script_hash: None,
            entry_script_hash: None,
            calling_script_hash: None,
            current_contract_groups: Vec::new(),
            calling_contract_groups: Vec::new(),
            trigger: Trigger::Application,
            platform: "NEO".to_string(),
        }
    }

    pub fn with_timestamp(
        store: &'a mut dyn Store,
        gas_limit: u64,
        signer: Option<Hash160>,
        timestamp: i64,
    ) -> Self {
        let mut ctx = Self::new(store, gas_limit, signer);
        ctx.timestamp = timestamp;
        ctx.invocation_counter = 0;
        ctx
    }
}
