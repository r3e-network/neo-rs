use alloc::vec;
use alloc::vec::Vec;

use neo_crypto::ecc256::PublicKey;

use super::{ContractParameter, ContractParameterType};

#[derive(Clone, Debug)]
pub struct Contract {
    script: Vec<u8>,
    parameters: Vec<ContractParameter>,
    deployed: bool,
}

impl Contract {
    pub fn new(script: Vec<u8>, parameters: Vec<ContractParameter>, deployed: bool) -> Self {
        Self {
            script,
            parameters,
            deployed,
        }
    }

    pub fn signature(public_key: &PublicKey) -> Self {
        Self {
            script: public_key.signature_redeem_script().to_vec(),
            parameters: vec![ContractParameter::new(
                "signature",
                ContractParameterType::Signature,
            )],
            deployed: false,
        }
    }

    pub fn script(&self) -> &[u8] {
        &self.script
    }

    pub fn parameters(&self) -> &[ContractParameter] {
        &self.parameters
    }

    pub fn deployed(&self) -> bool {
        self.deployed
    }
}
