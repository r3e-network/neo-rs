use alloc::vec::Vec;

use neo_base::hash::Hash160;
use neo_core::{h160::H160, tx::Signer};

use super::ExecutionContext;

impl<'a> ExecutionContext<'a> {
    pub fn signer(&self) -> Option<Hash160> {
        self.legacy_signer
    }

    pub fn set_signers(&mut self, signers: Vec<Signer>) {
        self.signers = signers;
    }

    pub fn set_signers_from(&mut self, signers: &[Signer]) {
        self.signers = signers.to_vec();
        if self.legacy_signer.is_none() {
            if let Some(first) = self.signers.first() {
                self.legacy_signer = Some(hash_from_h160(&first.account));
            }
        }
    }

    pub fn add_signer(&mut self, signer: Signer) {
        self.signers.push(signer);
    }

    pub fn signers(&self) -> &[Signer] {
        &self.signers
    }

    pub fn signers_mut(&mut self) -> &mut Vec<Signer> {
        &mut self.signers
    }
}

fn hash_from_h160(value: &H160) -> Hash160 {
    let mut buf = [0u8; 20];
    buf.copy_from_slice(value.as_le_bytes());
    Hash160::new(buf)
}
