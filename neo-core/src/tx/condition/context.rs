use neo_base::hash::Hash160;
use neo_crypto::ecc256::PublicKey;

#[derive(Debug, Clone)]
pub struct WitnessConditionContext<'a> {
    pub current_script_hash: Hash160,
    pub current_contract_groups: Option<&'a [PublicKey]>,
    pub calling_script_hash: Option<Hash160>,
    pub calling_contract_groups: Option<&'a [PublicKey]>,
    pub is_called_by_entry: bool,
}

impl<'a> WitnessConditionContext<'a> {
    pub fn new(current_script_hash: Hash160) -> Self {
        Self {
            current_script_hash,
            current_contract_groups: None,
            calling_script_hash: None,
            calling_contract_groups: None,
            is_called_by_entry: false,
        }
    }

    pub fn with_current_groups(mut self, groups: &'a [PublicKey]) -> Self {
        self.current_contract_groups = Some(groups);
        self
    }

    pub fn with_calling_script(mut self, hash: Hash160) -> Self {
        self.calling_script_hash = Some(hash);
        self
    }

    pub fn with_calling_groups(mut self, groups: &'a [PublicKey]) -> Self {
        self.calling_contract_groups = Some(groups);
        self
    }

    pub fn called_by_entry(mut self, value: bool) -> Self {
        self.is_called_by_entry = value;
        self
    }
}
