use crate::crypto::keys::PublicKey;
use crate::smartcontract::Parameter;
use std::collections::HashMap;

/// Item represents a transaction context item.
#[derive(Debug, Clone)]
pub struct Item {
    pub script: Vec<u8>,
    pub parameters: Vec<Parameter>,
    pub signatures: HashMap<String, Vec<u8>>,
}

impl Item {
    /// GetSignature returns a signature for the pub if present.
    pub fn get_signature(&self, pub_key: &PublicKey) -> Option<&Vec<u8>> {
        self.signatures.get(&pub_key.string_compressed())
    }

    /// AddSignature adds a signature for the pub.
    pub fn add_signature(&mut self, pub_key: &PublicKey, sig: Vec<u8>) {
        let pub_hex = pub_key.string_compressed();
        self.signatures.insert(pub_hex, sig);
    }
}
