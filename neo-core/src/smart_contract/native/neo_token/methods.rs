//
// methods.rs - NeoToken constructor and method registration
//

use super::*;

impl NeoToken {
    pub fn new() -> Self {
        // Method registrations matching C# NeoToken exactly
        let methods = vec![
            // NEP-17 standard methods
            NativeMethod::safe(
                "symbol".to_string(),
                0,
                Vec::new(),
                ContractParameterType::String,
            ),
            NativeMethod::safe(
                "decimals".to_string(),
                0,
                Vec::new(),
                ContractParameterType::Integer,
            ),
            NativeMethod::safe(
                "totalSupply".to_string(),
                1 << 15,
                Vec::new(),
                ContractParameterType::Integer,
            )
            .with_required_call_flags(CallFlags::READ_STATES),
            NativeMethod::safe(
                "balanceOf".to_string(),
                1 << 15,
                vec![ContractParameterType::Hash160],
                ContractParameterType::Integer,
            )
            .with_required_call_flags(CallFlags::READ_STATES)
            .with_parameter_names(vec!["account".to_string()]),
            NativeMethod::unsafe_method(
                "transfer".to_string(),
                1 << 17,
                CallFlags::ALL.bits(),
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::Hash160,
                    ContractParameterType::Integer,
                    ContractParameterType::Any,
                ],
                ContractParameterType::Boolean,
            )
            .with_storage_fee(50)
            .with_parameter_names(vec![
                "from".to_string(),
                "to".to_string(),
                "amount".to_string(),
                "data".to_string(),
            ]),
            // Governance query methods (safe)
            NativeMethod::safe(
                "unclaimedGas".to_string(),
                1 << 4,
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::Integer,
                ],
                ContractParameterType::Integer,
            )
            .with_parameter_names(vec!["account".to_string(), "end".to_string()]),
            NativeMethod::safe(
                "getAccountState".to_string(),
                1 << 4,
                vec![ContractParameterType::Hash160],
                ContractParameterType::Array,
            )
            .with_parameter_names(vec!["account".to_string()]),
            NativeMethod::safe(
                "getCandidates".to_string(),
                1 << 22,
                Vec::new(),
                ContractParameterType::Array,
            ),
            NativeMethod::safe(
                "getAllCandidates".to_string(),
                1 << 22,
                Vec::new(),
                ContractParameterType::Array,
            ),
            NativeMethod::safe(
                "getCandidateVote".to_string(),
                1 << 4,
                vec![ContractParameterType::PublicKey],
                ContractParameterType::Integer,
            )
            .with_parameter_names(vec!["pubkey".to_string()]),
            NativeMethod::safe(
                "getCommittee".to_string(),
                1 << 4,
                Vec::new(),
                ContractParameterType::Array,
            ),
            NativeMethod::safe(
                "getCommitteeAddress".to_string(),
                1 << 4,
                Vec::new(),
                ContractParameterType::Hash160,
            )
            .with_active_in(Hardfork::HfCockatrice)
            .with_required_call_flags(CallFlags::READ_STATES),
            NativeMethod::safe(
                "getNextBlockValidators".to_string(),
                1 << 4,
                Vec::new(),
                ContractParameterType::Array,
            ),
            NativeMethod::safe(
                "getGasPerBlock".to_string(),
                1 << 4,
                Vec::new(),
                ContractParameterType::Integer,
            ),
            NativeMethod::safe(
                "getRegisterPrice".to_string(),
                1 << 4,
                Vec::new(),
                ContractParameterType::Integer,
            ),
            // NEP-17 callbacks (Echidna+).
            NativeMethod::unsafe_method(
                "onNEP17Payment".to_string(),
                1 << 15,
                (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::Integer,
                    ContractParameterType::Any,
                ],
                ContractParameterType::Void,
            )
            .with_active_in(Hardfork::HfEchidna)
            .with_parameter_names(vec![
                "from".to_string(),
                "amount".to_string(),
                "data".to_string(),
            ]),
            // Governance write methods (unsafe - require witness/committee)
            NativeMethod::unsafe_method(
                "registerCandidate".to_string(),
                1 << SECONDS_PER_BLOCK,
                CallFlags::STATES.bits(),
                vec![ContractParameterType::PublicKey],
                ContractParameterType::Boolean,
            )
            .with_deprecated_in(Hardfork::HfEchidna)
            .with_parameter_names(vec!["pubkey".to_string()]),
            NativeMethod::unsafe_method(
                "registerCandidate".to_string(),
                1 << SECONDS_PER_BLOCK,
                (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
                vec![ContractParameterType::PublicKey],
                ContractParameterType::Boolean,
            )
            .with_active_in(Hardfork::HfEchidna)
            .with_parameter_names(vec!["pubkey".to_string()]),
            NativeMethod::unsafe_method(
                "unregisterCandidate".to_string(),
                1 << SECONDS_PER_BLOCK,
                CallFlags::STATES.bits(),
                vec![ContractParameterType::PublicKey],
                ContractParameterType::Boolean,
            )
            .with_deprecated_in(Hardfork::HfEchidna)
            .with_parameter_names(vec!["pubkey".to_string()]),
            NativeMethod::unsafe_method(
                "unregisterCandidate".to_string(),
                1 << SECONDS_PER_BLOCK,
                (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
                vec![ContractParameterType::PublicKey],
                ContractParameterType::Boolean,
            )
            .with_active_in(Hardfork::HfEchidna)
            .with_parameter_names(vec!["pubkey".to_string()]),
            NativeMethod::unsafe_method(
                "vote".to_string(),
                1 << SECONDS_PER_BLOCK,
                CallFlags::STATES.bits(),
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::PublicKey,
                ],
                ContractParameterType::Boolean,
            )
            .with_deprecated_in(Hardfork::HfEchidna)
            .with_parameter_names(vec!["account".to_string(), "voteTo".to_string()]),
            NativeMethod::unsafe_method(
                "vote".to_string(),
                1 << SECONDS_PER_BLOCK,
                (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::PublicKey,
                ],
                ContractParameterType::Boolean,
            )
            .with_active_in(Hardfork::HfEchidna)
            .with_parameter_names(vec!["account".to_string(), "voteTo".to_string()]),
            NativeMethod::unsafe_method(
                "setGasPerBlock".to_string(),
                1 << 4,
                CallFlags::STATES.bits(),
                vec![ContractParameterType::Integer],
                ContractParameterType::Void,
            ),
            NativeMethod::unsafe_method(
                "setRegisterPrice".to_string(),
                1 << 4,
                CallFlags::STATES.bits(),
                vec![ContractParameterType::Integer],
                ContractParameterType::Void,
            ),
        ];

        Self { methods }
    }

    pub(super) fn total_supply_bytes() -> Vec<u8> {
        let mut bytes = BigInt::from(Self::TOTAL_SUPPLY).to_signed_bytes_le();
        if bytes.is_empty() {
            bytes.push(0);
        }
        bytes
    }

    pub(super) fn invoke_method(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        match method {
            // NEP-17 standard methods
            "symbol" => Ok(Self::SYMBOL.as_bytes().to_vec()),
            "decimals" => Ok(vec![Self::DECIMALS]),
            "totalSupply" => Ok(Self::total_supply_bytes()),
            "balanceOf" => self.balance_of(engine, args),
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
                "Method not implemented: {}",
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
