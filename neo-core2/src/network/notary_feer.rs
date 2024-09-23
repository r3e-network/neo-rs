use std::sync::Arc;
use num_bigint::BigInt;
use crate::util::Uint160;
use crate::ledger::Ledger;

// NotaryFeer implements mempool::Feer trait for Notary balance handling.
pub struct NotaryFeer {
    bc: Arc<Ledger>,
}

impl NotaryFeer {
    // FeePerByte implements mempool::Feer trait.
    pub fn fee_per_byte(&self) -> i64 {
        self.bc.fee_per_byte()
    }

    // GetUtilityTokenBalance implements mempool::Feer trait.
    pub fn get_utility_token_balance(&self, acc: Uint160) -> BigInt {
        self.bc.get_notary_balance(acc)
    }

    // BlockHeight implements mempool::Feer trait.
    pub fn block_height(&self) -> u32 {
        self.bc.block_height()
    }

    // NewNotaryFeer returns new NotaryFeer instance.
    pub fn new(bc: Arc<Ledger>) -> NotaryFeer {
        NotaryFeer { bc }
    }
}
