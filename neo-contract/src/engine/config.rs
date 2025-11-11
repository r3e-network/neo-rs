use neo_base::hash::Hash160;
use neo_core::tx::Signer;
use neo_vm::Trigger;

/// Configuration for a single script execution.
pub struct EngineConfig<'a> {
    pub trigger: Trigger,
    pub platform: &'a str,
    pub gas_limit: u64,
    pub timestamp: i64,
    pub signers: &'a [Signer],
    pub calling_script_hash: Option<Hash160>,
    pub current_contract_groups: &'a [neo_crypto::ecc256::PublicKey],
    pub calling_contract_groups: &'a [neo_crypto::ecc256::PublicKey],
}

impl<'a> Default for EngineConfig<'a> {
    fn default() -> Self {
        Self {
            trigger: Trigger::Application,
            platform: "NEO",
            gas_limit: 20_000_000,
            timestamp: 0,
            signers: &[],
            calling_script_hash: None,
            current_contract_groups: &[],
            calling_contract_groups: &[],
        }
    }
}
