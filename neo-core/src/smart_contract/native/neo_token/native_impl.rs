//
// native_impl.rs - NativeContract trait implementation
//

use super::*;
use crate::smart_contract::native::security_fixes::{SafeArithmetic, StateValidator};

/// NeoAccountState helper methods
impl NeoAccountState {
    /// Converts account state to a StackItem for serialization
    pub(super) fn to_stack_item(&self) -> StackItem {
        StackItem::from_struct(vec![
            StackItem::from_int(self.balance.clone()),
            StackItem::from_int(self.balance_height),
            match &self.vote_to {
                Some(pk) => StackItem::from_byte_string(pk.as_bytes().to_vec()),
                None => StackItem::Null,
            },
            StackItem::from_int(self.last_gas_per_vote.clone()),
        ])
    }
}

impl NativeContract for NeoToken {
    fn id(&self) -> i32 {
        Self::ID
    }

    fn hash(&self) -> UInt160 {
        *NEO_HASH
    }

    fn name(&self) -> &str {
        Self::NAME
    }

    fn methods(&self) -> &[NativeMethod] {
        &self.methods
    }

    fn is_active(&self, _settings: &ProtocolSettings, _block_height: u32) -> bool {
        true
    }

    fn supported_standards(&self, settings: &ProtocolSettings, block_height: u32) -> Vec<String> {
        if settings.is_hardfork_enabled(crate::hardfork::Hardfork::HfEchidna, block_height) {
            vec!["NEP-17".to_string(), "NEP-27".to_string()]
        } else {
            vec!["NEP-17".to_string()]
        }
    }

    fn events(
        &self,
        settings: &ProtocolSettings,
        block_height: u32,
    ) -> Vec<ContractEventDescriptor> {
        let mut events = vec![
            ContractEventDescriptor::new(
                "Transfer".to_string(),
                vec![
                    ContractParameterDefinition::new(
                        "from".to_string(),
                        ContractParameterType::Hash160,
                    )
                    .expect("Transfer.from"),
                    ContractParameterDefinition::new(
                        "to".to_string(),
                        ContractParameterType::Hash160,
                    )
                    .expect("Transfer.to"),
                    ContractParameterDefinition::new(
                        "amount".to_string(),
                        ContractParameterType::Integer,
                    )
                    .expect("Transfer.amount"),
                ],
            )
            .expect("Transfer event descriptor"),
            ContractEventDescriptor::new(
                "CandidateStateChanged".to_string(),
                vec![
                    ContractParameterDefinition::new(
                        "pubkey".to_string(),
                        ContractParameterType::PublicKey,
                    )
                    .expect("CandidateStateChanged.pubkey"),
                    ContractParameterDefinition::new(
                        "registered".to_string(),
                        ContractParameterType::Boolean,
                    )
                    .expect("CandidateStateChanged.registered"),
                    ContractParameterDefinition::new(
                        "votes".to_string(),
                        ContractParameterType::Integer,
                    )
                    .expect("CandidateStateChanged.votes"),
                ],
            )
            .expect("CandidateStateChanged event descriptor"),
            ContractEventDescriptor::new(
                "Vote".to_string(),
                vec![
                    ContractParameterDefinition::new(
                        "account".to_string(),
                        ContractParameterType::Hash160,
                    )
                    .expect("Vote.account"),
                    ContractParameterDefinition::new(
                        "from".to_string(),
                        ContractParameterType::PublicKey,
                    )
                    .expect("Vote.from"),
                    ContractParameterDefinition::new(
                        "to".to_string(),
                        ContractParameterType::PublicKey,
                    )
                    .expect("Vote.to"),
                    ContractParameterDefinition::new(
                        "amount".to_string(),
                        ContractParameterType::Integer,
                    )
                    .expect("Vote.amount"),
                ],
            )
            .expect("Vote event descriptor"),
        ];

        if settings.is_hardfork_enabled(crate::hardfork::Hardfork::HfCockatrice, block_height) {
            events.push(
                ContractEventDescriptor::new(
                    "CommitteeChanged".to_string(),
                    vec![
                        ContractParameterDefinition::new(
                            "old".to_string(),
                            ContractParameterType::Array,
                        )
                        .expect("CommitteeChanged.old"),
                        ContractParameterDefinition::new(
                            "new".to_string(),
                            ContractParameterType::Array,
                        )
                        .expect("CommitteeChanged.new"),
                    ],
                )
                .expect("CommitteeChanged event descriptor"),
            );
        }

        events
    }

    fn initialize(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        let snapshot = engine.snapshot_cache();
        let committee_key = StorageKey::create(Self::ID, Self::PREFIX_COMMITTEE);
        if snapshot.as_ref().try_get(&committee_key).is_some() {
            return Ok(());
        }

        // Native contract initialization must not consume GAS. Use direct snapshot updates.
        let committee_pairs: Vec<(ECPoint, BigInt)> = engine
            .protocol_settings()
            .standby_committee
            .iter()
            .cloned()
            .map(|pk| (pk, BigInt::zero()))
            .collect();
        let committee_bytes = Self::encode_committee_with_votes(&committee_pairs)?;
        engine.set_storage(
            committee_key.clone(),
            StorageItem::from_bytes(committee_bytes),
        )?;

        let voters_key = StorageKey::create(Self::ID, Self::PREFIX_VOTERS_COUNT);
        engine.set_storage(voters_key, StorageItem::from_bytes(Vec::new()))?;

        let gas_key = StorageKey::create_with_bytes(
            Self::ID,
            Self::PREFIX_GAS_PER_BLOCK,
            &0u32.to_be_bytes(),
        );
        let initial_gas = BigInt::from(5i64 * Self::DATOSHI_FACTOR);
        engine.set_storage(
            gas_key,
            StorageItem::from_bytes(Self::encode_amount(&initial_gas)),
        )?;

        let register_key = StorageKey::create(Self::ID, Self::PREFIX_REGISTER_PRICE);
        let register_price = BigInt::from(Self::DEFAULT_REGISTER_PRICE);
        engine.set_storage(
            register_key,
            StorageItem::from_bytes(Self::encode_amount(&register_price)),
        )?;

        let validators = engine.protocol_settings().standby_validators();
        if !validators.is_empty() {
            let account = NativeHelpers::get_bft_address(&validators);
            let account_key = StorageKey::create_with_uint160(Self::ID, PREFIX_ACCOUNT, &account);
            let state = NeoAccountState {
                balance: BigInt::from(Self::TOTAL_SUPPLY),
                balance_height: 0,
                vote_to: None,
                last_gas_per_vote: BigInt::zero(),
            };
            let bytes = BinarySerializer::serialize(
                &state.to_stack_item(),
                &ExecutionEngineLimits::default(),
            )
            .map_err(CoreError::native_contract)?;
            engine.set_storage(account_key, StorageItem::from_bytes(bytes))?;
        }

        Ok(())
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        self.invoke_method(engine, method, args)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    /// OnPersist: Refresh committee if required.
    /// Matches C# NeoToken.OnPersistAsync.
    fn on_persist(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        let block = engine
            .persisting_block()
            .cloned()
            .ok_or_else(|| CoreError::native_contract("No persisting block available"))?;

        let committee_count = engine.protocol_settings().committee_members_count();
        if !Self::should_refresh_committee(block.index(), committee_count) {
            return Ok(());
        }

        let snapshot = engine.snapshot_cache();
        let old_committee =
            self.committee_from_cache_with_votes(snapshot.as_ref(), engine.protocol_settings())?;
        let old_keys: Vec<ECPoint> = old_committee.iter().map(|(pk, _)| pk.clone()).collect();

        let new_committee =
            self.compute_committee_members(snapshot.as_ref(), engine.protocol_settings())?;

        let context = engine.get_native_storage_context(&self.hash())?;
        let committee_bytes = Self::encode_committee_with_votes(&new_committee)?;
        let committee_suffix = StorageKey::create(Self::ID, Self::PREFIX_COMMITTEE)
            .suffix()
            .to_vec();
        engine.put_storage_item(&context, &committee_suffix, &committee_bytes)?;

        if engine.is_hardfork_enabled(crate::hardfork::Hardfork::HfCockatrice) {
            let new_keys: Vec<ECPoint> = new_committee.iter().map(|(pk, _)| pk.clone()).collect();
            if new_keys != old_keys {
                let old_array = StackItem::from_array(
                    old_keys
                        .iter()
                        .map(|pk| StackItem::from_byte_string(pk.as_bytes().to_vec()))
                        .collect::<Vec<_>>(),
                );
                let new_array = StackItem::from_array(
                    new_keys
                        .iter()
                        .map(|pk| StackItem::from_byte_string(pk.as_bytes().to_vec()))
                        .collect::<Vec<_>>(),
                );
                engine
                    .send_notification(
                        self.hash(),
                        "CommitteeChanged".to_string(),
                        vec![old_array, new_array],
                    )
                    .map_err(CoreError::native_contract)?;
            }
        }

        Ok(())
    }

    /// PostPersist: Distribute GAS rewards to committee members.
    /// Matches C# NeoToken.PostPersistAsync.
    fn post_persist(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        let block = engine
            .persisting_block()
            .cloned()
            .ok_or_else(|| CoreError::native_contract("No persisting block available"))?;

        let committee_count = engine.protocol_settings().committee_members_count();
        let validators_count = engine.protocol_settings().validators_count as usize;
        let snapshot = engine.snapshot_cache();

        let committee =
            self.committee_from_cache_with_votes(snapshot.as_ref(), engine.protocol_settings())?;
        if committee.is_empty() {
            return Ok(());
        }

        let reward_index = (block.index() % committee_count as u32) as usize;
        if reward_index >= committee.len() {
            return Ok(());
        }

        let ledger = LedgerContract::new();
        let current_index = ledger
            .current_index(snapshot.as_ref())
            .unwrap_or(block.index());
        let gas_per_block =
            self.get_gas_per_block_internal(snapshot.as_ref(), current_index.saturating_add(1));

        // Use safe arithmetic for committee reward calculation
        let committee_reward =
            SafeArithmetic::safe_mul(&gas_per_block, &BigInt::from(Self::COMMITTEE_REWARD_RATIO))?;
        let committee_reward = SafeArithmetic::safe_div(&committee_reward, &BigInt::from(100i64))?;

        if !committee_reward.is_zero() {
            let pubkey = &committee[reward_index].0;
            let account = Contract::create_signature_contract(pubkey.clone()).script_hash();
            GasToken::new().mint(engine, &account, &committee_reward, false)?;
        }

        if Self::should_refresh_committee(block.index(), committee_count) {
            let m = committee_count as i64;
            let n = validators_count as i64;

            // Use safe arithmetic for reward calculation
            let mut voter_reward_each =
                SafeArithmetic::safe_mul(&gas_per_block, &BigInt::from(Self::VOTER_REWARD_RATIO))?;
            voter_reward_each =
                SafeArithmetic::safe_mul(&voter_reward_each, &BigInt::from(Self::DATOSHI_FACTOR))?;
            voter_reward_each = SafeArithmetic::safe_mul(&voter_reward_each, &BigInt::from(m))?;
            voter_reward_each = SafeArithmetic::safe_div(&voter_reward_each, &BigInt::from(m + n))?;
            voter_reward_each =
                SafeArithmetic::safe_div(&voter_reward_each, &BigInt::from(100i64))?;

            let context = engine.get_native_storage_context(&self.hash())?;

            for (idx, (pubkey, votes)) in committee.iter().enumerate() {
                // Validate votes
                if votes.is_zero() || votes.is_negative() {
                    continue;
                }

                let factor = if idx < validators_count { 2i64 } else { 1i64 };

                // Use safe arithmetic
                let mut voter_sum_reward_per_neo =
                    SafeArithmetic::safe_mul(&BigInt::from(factor), &voter_reward_each)?;
                voter_sum_reward_per_neo =
                    SafeArithmetic::safe_div(&voter_sum_reward_per_neo, votes)?;

                let reward_key = StorageKey::create_with_bytes(
                    Self::ID,
                    Self::PREFIX_VOTER_REWARD_PER_COMMITTEE,
                    pubkey.as_bytes(),
                );
                let current_reward = snapshot
                    .as_ref()
                    .try_get(&reward_key)
                    .map(|item| item.to_bigint())
                    .unwrap_or_else(BigInt::zero);

                // Use safe arithmetic
                let updated = SafeArithmetic::safe_add(&current_reward, &voter_sum_reward_per_neo)?;

                // Validate the reward amount
                StateValidator::validate_account_state(&updated, 0, u32::MAX)?;

                engine.put_storage_item(
                    &context,
                    reward_key.suffix(),
                    &Self::encode_amount(&updated),
                )?;
            }
        }

        Ok(())
    }
}
