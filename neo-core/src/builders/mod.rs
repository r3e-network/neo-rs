//! Minimal builder helpers mirroring the C# `Neo.Builder` utilities used by
//! the test suite. These builders intentionally expose only the functionality
//! currently required by the tests while keeping the API ergonomic for future
//! extensions.

use crate::network::p2p::payloads::{
    Conflicts, NotValidBefore, OracleResponse, OracleResponseCode, Signer, Transaction,
    TransactionAttribute, Witness, WitnessCondition, WitnessRule, WitnessRuleAction,
};
use crate::{cryptography::ECPoint, UInt160, UInt256, WitnessScope};
use neo_vm::{op_code::OpCode, script_builder::ScriptBuilder};

/// Convenience builder for constructing transactions in tests.
#[derive(Default)]
pub struct TransactionBuilder {
    inner: Transaction,
}

impl TransactionBuilder {
    /// Creates a builder seeded with an empty transaction.
    pub fn create_empty() -> Self {
        let mut tx = Transaction::new();
        tx.set_script(vec![OpCode::RET as u8]);
        Self { inner: tx }
    }

    /// Sets the version for the transaction being built.
    pub fn version(mut self, version: u8) -> Self {
        self.inner.set_version(version);
        self
    }

    /// Sets the script for the transaction being built.
    pub fn script(mut self, script: Vec<u8>) -> Self {
        self.inner.set_script(script);
        self
    }

    /// Builds the script using a script builder (C# AttachSystem parity).
    pub fn attach_system<F>(mut self, config: F) -> Self
    where
        F: FnOnce(&mut ScriptBuilder),
    {
        let mut builder = ScriptBuilder::new();
        config(&mut builder);
        self.inner.set_script(builder.to_array());
        self
    }

    /// Assigns the script bytes directly (C# AttachSystem overload).
    pub fn attach_system_script(mut self, script: Vec<u8>) -> Self {
        self.inner.set_script(script);
        self
    }

    /// Sets the nonce for the transaction being built.
    pub fn nonce(mut self, nonce: u32) -> Self {
        self.inner.set_nonce(nonce);
        self
    }

    /// Sets the system fee for the transaction being built.
    pub fn system_fee(mut self, system_fee: i64) -> Self {
        self.inner.set_system_fee(system_fee);
        self
    }

    /// Sets the network fee for the transaction being built.
    pub fn network_fee(mut self, network_fee: i64) -> Self {
        self.inner.set_network_fee(network_fee);
        self
    }

    /// Sets the valid-until block height for the transaction being built.
    pub fn valid_until(mut self, valid_until: u32) -> Self {
        self.inner.set_valid_until_block(valid_until);
        self
    }

    /// Configures transaction attributes using a builder.
    pub fn add_attributes<F>(mut self, config: F) -> Self
    where
        F: FnOnce(&mut TransactionAttributesBuilder),
    {
        let mut builder = TransactionAttributesBuilder::create_empty();
        config(&mut builder);
        self.inner.set_attributes(builder.build());
        self
    }

    /// Adds a witness using a builder.
    pub fn add_witness<F>(mut self, config: F) -> Self
    where
        F: FnOnce(&mut WitnessBuilder),
    {
        let mut builder = WitnessBuilder::create_empty();
        config(&mut builder);
        self.inner.add_witness(builder.build());
        self
    }

    /// Adds a witness using a builder with access to the transaction.
    pub fn add_witness_with_tx<F>(mut self, config: F) -> Self
    where
        F: FnOnce(&mut WitnessBuilder, &Transaction),
    {
        let mut builder = WitnessBuilder::create_empty();
        config(&mut builder, &self.inner);
        self.inner.add_witness(builder.build());
        self
    }

    /// Adds a signer using a builder with access to the transaction.
    pub fn add_signer<F>(mut self, config: F) -> Self
    where
        F: FnOnce(&mut SignerBuilder, &Transaction),
    {
        let mut builder = SignerBuilder::create_empty();
        config(&mut builder, &self.inner);
        self.inner.add_signer(builder.build());
        self
    }

    /// Assigns signers to the transaction.
    pub fn signers(mut self, signers: Vec<Signer>) -> Self {
        self.inner.set_signers(signers);
        self
    }

    /// Assigns witnesses to the transaction.
    pub fn witnesses(mut self, witnesses: Vec<Witness>) -> Self {
        self.inner.set_witnesses(witnesses);
        self
    }

    /// Finalises the builder and returns the transaction.
    pub fn build(self) -> Transaction {
        self.inner
    }
}

/// Builder for `Signer` instances. Provides a fluent API matching the
/// expectations of the converted C# tests.
#[derive(Clone)]
pub struct SignerBuilder {
    account: UInt160,
    scopes: WitnessScope,
    allowed_contracts: Vec<UInt160>,
    allowed_groups: Vec<ECPoint>,
    rules: Vec<WitnessRule>,
}

impl SignerBuilder {
    /// Creates a builder with default signer settings (zero account,
    /// `None` scope).
    pub fn create_empty() -> Self {
        Self {
            account: UInt160::zero(),
            scopes: WitnessScope::NONE,
            allowed_contracts: Vec::new(),
            allowed_groups: Vec::new(),
            rules: Vec::new(),
        }
    }

    pub fn account(&mut self, account: UInt160) -> &mut Self {
        self.account = account;
        self
    }

    pub fn scope(&mut self, scope: WitnessScope) -> &mut Self {
        self.scopes = scope;
        self
    }

    pub fn with_allowed_contract(&mut self, contract: UInt160) -> &mut Self {
        self.allowed_contracts.push(contract);
        self
    }

    pub fn with_allowed_group(&mut self, group: ECPoint) -> &mut Self {
        self.allowed_groups.push(group);
        self
    }

    pub fn allow_contract(&mut self, contract: UInt160) -> &mut Self {
        self.with_allowed_contract(contract)
    }

    pub fn allow_group(&mut self, group: ECPoint) -> &mut Self {
        self.with_allowed_group(group)
    }

    pub fn add_witness_scope(&mut self, scope: WitnessScope) -> &mut Self {
        self.scopes |= scope;
        self
    }

    pub fn add_witness_rule<F>(&mut self, action: WitnessRuleAction, config: F) -> &mut Self
    where
        F: FnOnce(&mut WitnessRuleBuilder),
    {
        let mut builder = WitnessRuleBuilder::create(action);
        config(&mut builder);
        self.rules.push(builder.build());
        self
    }

    pub fn build(&self) -> Signer {
        let mut signer = Signer::new(self.account, self.scopes);
        signer.allowed_contracts = self.allowed_contracts.clone();
        signer.allowed_groups = self.allowed_groups.clone();
        signer.rules = self.rules.clone();
        signer
    }
}

/// Builder for transaction attributes.
pub struct TransactionAttributesBuilder {
    attributes: Vec<TransactionAttribute>,
}

impl TransactionAttributesBuilder {
    pub fn create_empty() -> Self {
        Self {
            attributes: Vec::new(),
        }
    }

    pub fn add_high_priority(&mut self) -> &mut Self {
        if self
            .attributes
            .iter()
            .any(|attr| matches!(attr, TransactionAttribute::HighPriority))
        {
            panic!("HighPriority attribute already exists in the transaction attributes. Only one HighPriority attribute is allowed per transaction.");
        }
        self.attributes.push(TransactionAttribute::HighPriority);
        self
    }

    pub fn add_conflict<F>(&mut self, config: F) -> &mut Self
    where
        F: FnOnce(&mut Conflicts),
    {
        let mut conflicts = Conflicts::new(UInt256::zero());
        config(&mut conflicts);
        self.attributes
            .push(TransactionAttribute::Conflicts(conflicts));
        self
    }

    pub fn add_oracle_response<F>(&mut self, config: F) -> &mut Self
    where
        F: FnOnce(&mut OracleResponse),
    {
        let mut response = OracleResponse::new(0, OracleResponseCode::Success, Vec::new());
        config(&mut response);
        self.attributes
            .push(TransactionAttribute::OracleResponse(response));
        self
    }

    pub fn add_not_valid_before(&mut self, height: u32) -> &mut Self {
        if self.attributes.iter().any(|attr| match attr {
            TransactionAttribute::NotValidBefore(existing) => existing.height == height,
            _ => false,
        }) {
            panic!(
                "NotValidBefore attribute for block {} already exists in the transaction attributes. Each block height can only be specified once.",
                height
            );
        }
        self.attributes
            .push(TransactionAttribute::NotValidBefore(NotValidBefore::new(
                height,
            )));
        self
    }

    pub fn build(&self) -> Vec<TransactionAttribute> {
        self.attributes.clone()
    }
}

/// Builder for witness conditions.
pub struct WitnessConditionBuilder {
    condition: Option<WitnessCondition>,
}

impl WitnessConditionBuilder {
    pub fn create() -> Self {
        Self { condition: None }
    }

    pub fn create_empty() -> Self {
        Self::create()
    }

    pub fn and<F>(&mut self, config: F) -> &mut Self
    where
        F: FnOnce(&mut AndConditionBuilder),
    {
        let mut builder = AndConditionBuilder::create_empty();
        config(&mut builder);
        self.condition = Some(builder.build());
        self
    }

    pub fn or<F>(&mut self, config: F) -> &mut Self
    where
        F: FnOnce(&mut OrConditionBuilder),
    {
        let mut builder = OrConditionBuilder::create_empty();
        config(&mut builder);
        self.condition = Some(builder.build());
        self
    }

    pub fn boolean(&mut self, value: bool) -> &mut Self {
        self.condition = Some(WitnessCondition::Boolean { value });
        self
    }

    pub fn called_by_contract(&mut self, hash: UInt160) -> &mut Self {
        self.condition = Some(WitnessCondition::CalledByContract { hash });
        self
    }

    pub fn called_by_entry(&mut self) -> &mut Self {
        self.condition = Some(WitnessCondition::CalledByEntry);
        self
    }

    pub fn called_by_group(&mut self, group: ECPoint) -> &mut Self {
        self.condition = Some(WitnessCondition::CalledByGroup {
            group: group.as_bytes().to_vec(),
        });
        self
    }

    pub fn group(&mut self, group: ECPoint) -> &mut Self {
        self.condition = Some(WitnessCondition::Group {
            group: group.as_bytes().to_vec(),
        });
        self
    }

    pub fn not<F>(&mut self, config: F) -> &mut Self
    where
        F: FnOnce(&mut WitnessConditionBuilder),
    {
        let mut builder = WitnessConditionBuilder::create();
        config(&mut builder);
        self.condition = Some(WitnessCondition::Not {
            condition: Box::new(builder.build()),
        });
        self
    }

    pub fn script_hash(&mut self, hash: UInt160) -> &mut Self {
        self.condition = Some(WitnessCondition::ScriptHash { hash });
        self
    }

    pub fn build(&self) -> WitnessCondition {
        self.condition
            .clone()
            .unwrap_or(WitnessCondition::Boolean { value: true })
    }
}

/// Builder for witness rules.
pub struct WitnessRuleBuilder {
    action: WitnessRuleAction,
    condition: Option<WitnessCondition>,
}

impl WitnessRuleBuilder {
    pub fn create(action: WitnessRuleAction) -> Self {
        Self {
            action,
            condition: None,
        }
    }

    pub fn add_condition<F>(&mut self, config: F) -> &mut Self
    where
        F: FnOnce(&mut WitnessConditionBuilder),
    {
        let mut builder = WitnessConditionBuilder::create();
        config(&mut builder);
        self.condition = Some(builder.build());
        self
    }

    pub fn build(&self) -> WitnessRule {
        WitnessRule::new(
            self.action,
            self.condition
                .clone()
                .expect("Witness rule condition must be set"),
        )
    }
}

/// Builder for `And` witness conditions.
pub struct AndConditionBuilder {
    conditions: Vec<WitnessCondition>,
}

impl AndConditionBuilder {
    pub fn create_empty() -> Self {
        Self {
            conditions: Vec::new(),
        }
    }

    pub fn and<F>(&mut self, config: F) -> &mut Self
    where
        F: FnOnce(&mut AndConditionBuilder),
    {
        let mut builder = AndConditionBuilder::create_empty();
        config(&mut builder);
        self.conditions.push(builder.build());
        self
    }

    pub fn or<F>(&mut self, config: F) -> &mut Self
    where
        F: FnOnce(&mut OrConditionBuilder),
    {
        let mut builder = OrConditionBuilder::create_empty();
        config(&mut builder);
        self.conditions.push(builder.build());
        self
    }

    pub fn boolean(&mut self, value: bool) -> &mut Self {
        self.conditions.push(WitnessCondition::Boolean { value });
        self
    }

    pub fn called_by_contract(&mut self, hash: UInt160) -> &mut Self {
        self.conditions
            .push(WitnessCondition::CalledByContract { hash });
        self
    }

    pub fn called_by_entry(&mut self) -> &mut Self {
        self.conditions.push(WitnessCondition::CalledByEntry);
        self
    }

    pub fn called_by_group(&mut self, group: ECPoint) -> &mut Self {
        self.conditions.push(WitnessCondition::CalledByGroup {
            group: group.as_bytes().to_vec(),
        });
        self
    }

    pub fn group(&mut self, group: ECPoint) -> &mut Self {
        self.conditions.push(WitnessCondition::Group {
            group: group.as_bytes().to_vec(),
        });
        self
    }

    pub fn script_hash(&mut self, hash: UInt160) -> &mut Self {
        self.conditions.push(WitnessCondition::ScriptHash { hash });
        self
    }

    pub fn build(self) -> WitnessCondition {
        WitnessCondition::And {
            conditions: self.conditions,
        }
    }
}

/// Builder for `Or` witness conditions.
pub struct OrConditionBuilder {
    conditions: Vec<WitnessCondition>,
}

impl OrConditionBuilder {
    pub fn create_empty() -> Self {
        Self {
            conditions: Vec::new(),
        }
    }

    pub fn and<F>(&mut self, config: F) -> &mut Self
    where
        F: FnOnce(&mut AndConditionBuilder),
    {
        let mut builder = AndConditionBuilder::create_empty();
        config(&mut builder);
        self.conditions.push(builder.build());
        self
    }

    pub fn or<F>(&mut self, config: F) -> &mut Self
    where
        F: FnOnce(&mut OrConditionBuilder),
    {
        let mut builder = OrConditionBuilder::create_empty();
        config(&mut builder);
        self.conditions.push(builder.build());
        self
    }

    pub fn boolean(&mut self, value: bool) -> &mut Self {
        self.conditions.push(WitnessCondition::Boolean { value });
        self
    }

    pub fn called_by_contract(&mut self, hash: UInt160) -> &mut Self {
        self.conditions
            .push(WitnessCondition::CalledByContract { hash });
        self
    }

    pub fn called_by_entry(&mut self) -> &mut Self {
        self.conditions.push(WitnessCondition::CalledByEntry);
        self
    }

    pub fn called_by_group(&mut self, group: ECPoint) -> &mut Self {
        self.conditions.push(WitnessCondition::CalledByGroup {
            group: group.as_bytes().to_vec(),
        });
        self
    }

    pub fn group(&mut self, group: ECPoint) -> &mut Self {
        self.conditions.push(WitnessCondition::Group {
            group: group.as_bytes().to_vec(),
        });
        self
    }

    pub fn script_hash(&mut self, hash: UInt160) -> &mut Self {
        self.conditions.push(WitnessCondition::ScriptHash { hash });
        self
    }

    pub fn build(self) -> WitnessCondition {
        WitnessCondition::Or {
            conditions: self.conditions,
        }
    }
}

/// Builder for `Witness` instances.
#[derive(Default)]
pub struct WitnessBuilder {
    invocation: Vec<u8>,
    verification: Vec<u8>,
}

impl WitnessBuilder {
    pub fn create_empty() -> Self {
        Self::default()
    }

    pub fn invocation_script(mut self, script: Vec<u8>) -> Self {
        self.invocation = script;
        self
    }

    pub fn verification_script(mut self, script: Vec<u8>) -> Self {
        self.verification = script;
        self
    }

    pub fn add_invocation(&mut self, script: Vec<u8>) -> &mut Self {
        if !self.invocation.is_empty() {
            panic!(
                "Invocation script already exists in the witness builder. Only one invocation script can be added per witness."
            );
        }
        self.invocation = script;
        self
    }

    pub fn add_verification(&mut self, script: Vec<u8>) -> &mut Self {
        if !self.verification.is_empty() {
            panic!(
                "Verification script already exists in the witness builder. Only one verification script can be added per witness."
            );
        }
        self.verification = script;
        self
    }

    pub fn add_invocation_with_builder<F>(&mut self, config: F) -> &mut Self
    where
        F: FnOnce(&mut ScriptBuilder),
    {
        if !self.invocation.is_empty() {
            panic!(
                "Invocation script already exists in the witness builder. Only one invocation script can be added per witness."
            );
        }
        let mut builder = ScriptBuilder::new();
        config(&mut builder);
        self.invocation = builder.to_array();
        self
    }

    pub fn add_verification_with_builder<F>(&mut self, config: F) -> &mut Self
    where
        F: FnOnce(&mut ScriptBuilder),
    {
        if !self.verification.is_empty() {
            panic!(
                "Verification script already exists in the witness builder. Only one verification script can be added per witness."
            );
        }
        let mut builder = ScriptBuilder::new();
        config(&mut builder);
        self.verification = builder.to_array();
        self
    }

    pub fn build(&self) -> Witness {
        if self.invocation.is_empty() && self.verification.is_empty() {
            Witness::new()
        } else {
            Witness::new_with_scripts(self.invocation.clone(), self.verification.clone())
        }
    }
}
