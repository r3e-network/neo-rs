use super::Transaction;
use crate::persistence::DataCache;
use neo_primitives::UInt160;

impl crate::VerifiableExt for Transaction {
    fn script_hashes_for_verifying(&self, _snapshot: &DataCache) -> Vec<UInt160> {
        self.signer_accounts()
    }
}
