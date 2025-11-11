use alloc::vec::Vec;

use neo_crypto::ecc256::PublicKey;

use crate::{
    h160::H160,
    tx::{WitnessRule, WitnessScopes},
};

#[derive(Debug, Clone)]
pub struct Signer {
    pub account: H160,
    pub scopes: WitnessScopes,
    pub allowed_contract: Vec<H160>,
    pub allowed_groups: Vec<PublicKey>,
    pub rules: Vec<WitnessRule>,
}

impl Signer {
    pub fn rules(&self) -> &[WitnessRule] {
        &self.rules
    }
}
