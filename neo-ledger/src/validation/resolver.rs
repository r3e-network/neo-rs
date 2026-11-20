use neo_primitives::{UInt160, Verifiable};
use neo_protocol::payloads::{Block, Header, Transaction};
use neo_protocol::witness::WitnessData;
use neo_smart_contract::application_engine::WitnessHashResolver;
use neo_storage::persistence::PersistenceDataCache;

pub trait VerifiableExt: WitnessData {
    fn script_hashes_for_verifying(&self, snapshot: &PersistenceDataCache) -> Vec<UInt160>;

    fn as_transaction(&self) -> Option<&Transaction> {
        self.as_any().downcast_ref()
    }
}

/// Delegates to the canonical header verifying-hash policy in
/// `neo-native-contracts`.
///
/// Consensus-critical witness selection must resolve identically for every
/// caller, so the genesis/`next_consensus` rule has a single owner and this
/// entry point only forwards to it.
pub fn header_script_hashes_for_verifying(
    header: &Header,
    snapshot: &PersistenceDataCache,
) -> Vec<UInt160> {
    neo_native_contracts::header_verifying_script_hashes(header, snapshot)
}

pub fn block_script_hashes_for_verifying(
    block: &Block,
    snapshot: &PersistenceDataCache,
) -> Vec<UInt160> {
    header_script_hashes_for_verifying(&block.header, snapshot)
}

pub fn transaction_script_hashes_for_verifying(transaction: &Transaction) -> Vec<UInt160> {
    transaction.signer_accounts()
}

impl VerifiableExt for Transaction {
    fn script_hashes_for_verifying(&self, _snapshot: &PersistenceDataCache) -> Vec<UInt160> {
        transaction_script_hashes_for_verifying(self)
    }
}

impl VerifiableExt for Header {
    fn script_hashes_for_verifying(&self, snapshot: &PersistenceDataCache) -> Vec<UInt160> {
        header_script_hashes_for_verifying(self, snapshot)
    }
}

impl VerifiableExt for Block {
    fn script_hashes_for_verifying(&self, snapshot: &PersistenceDataCache) -> Vec<UInt160> {
        block_script_hashes_for_verifying(self, snapshot)
    }
}

pub struct LedgerWitnessHashResolver;

impl LedgerWitnessHashResolver {
    // None distinguishes an unowned container from a known container with no verifying hashes.
    pub fn resolve_known(
        verifiable: &dyn Verifiable,
        snapshot: &PersistenceDataCache,
    ) -> Option<Vec<UInt160>> {
        if let Some(transaction) = verifiable.as_any().downcast_ref::<Transaction>() {
            Some(transaction_script_hashes_for_verifying(transaction))
        } else if let Some(header) = verifiable.as_any().downcast_ref::<Header>() {
            Some(header_script_hashes_for_verifying(header, snapshot))
        } else {
            verifiable
                .as_any()
                .downcast_ref::<Block>()
                .map(|block| block_script_hashes_for_verifying(block, snapshot))
        }
    }
}

impl WitnessHashResolver for LedgerWitnessHashResolver {
    fn resolve(
        &self,
        verifiable: &dyn Verifiable,
        snapshot: &PersistenceDataCache,
    ) -> Vec<UInt160> {
        Self::resolve_known(verifiable, snapshot).unwrap_or_default()
    }
}
