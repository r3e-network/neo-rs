//
// methods.rs - NeoToken constructor and method registration
//

use super::*;

impl NeoToken {
    pub fn new() -> Self {
        Self {
            methods: Self::native_methods(),
        }
    }

    pub(super) fn invoke_method(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if let Some(result) = self.ft_invoke_standard_read(engine, method, args) {
            return result;
        }

        match method {
            // NEP-17 standard methods
            "transfer" => self.transfer(engine, args),
            // Governance query methods
            "unclaimedGas" => self.unclaimed_gas_invoke(engine, args),
            "getAccountState" => self.get_account_state_invoke(engine, args),
            "getCandidates" => self.get_candidates(engine),
            "getAllCandidates" => self.get_all_candidates(engine),
            "getCandidateVote" => self.get_candidate_vote(engine, args),
            "getCommittee" => self.get_committee(engine),
            "getCommitteeAddress" => self.get_committee_address(engine),
            "getNextBlockValidators" => self.get_next_block_validators(engine),
            "getGasPerBlock" => self.get_gas_per_block(engine),
            "getRegisterPrice" => self.get_register_price(engine),
            "onNEP17Payment" => self.on_nep17_payment(engine, args),
            // Governance write methods
            "registerCandidate" => self.register_candidate(engine, args),
            "unregisterCandidate" => self.unregister_candidate(engine, args),
            "vote" => self.vote(engine, args),
            "setGasPerBlock" => self.set_gas_per_block(engine, args),
            "setRegisterPrice" => self.set_register_price(engine, args),
            _ => Err(CoreError::native_contract(format!(
                "Unknown method: {}",
                method
            ))),
        }
    }

    pub fn symbol(&self) -> &'static str {
        Self::SYMBOL
    }

    pub fn decimals(&self) -> u8 {
        Self::DECIMALS
    }

    pub fn total_supply(&self) -> BigInt {
        BigInt::from(Self::TOTAL_SUPPLY)
    }
}
