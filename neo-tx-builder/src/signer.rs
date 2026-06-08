use neo_payloads::{Signer, WitnessRule, WitnessRuleAction};
use neo_primitives::{UInt160, WitnessScope};
use neo_crypto::ECPoint;

use super::WitnessRuleBuilder;

/// Builder for `Signer` instances. Provides a fluent API matching the
/// expectations of the converted C# tests.
#[derive(Clone)]
#[must_use]
pub struct SignerBuilder {
    account: UInt160,
    scopes: WitnessScope,
    allowed_contracts: Vec<UInt160>,
    allowed_groups: Vec<ECPoint>,
    rules: Vec<WitnessRule>,
}

neo_primitives::impl_default_via_new!(SignerBuilder);

impl SignerBuilder {
    /// Creates a builder with default signer settings (zero account,
    /// `None` scope).
    pub fn new() -> Self {
        Self {
            account: UInt160::zero(),
            scopes: WitnessScope::NONE,
            allowed_contracts: Vec::new(),
            allowed_groups: Vec::new(),
            rules: Vec::new(),
        }
    }

    /// Sets the account for this signer.
    pub fn account(&mut self, account: UInt160) -> &mut Self {
        self.account = account;
        self
    }

    /// Sets the witness scope for this signer.
    pub fn scope(&mut self, scope: WitnessScope) -> &mut Self {
        self.scopes = scope;
        self
    }

    /// Adds a contract to the allowed contracts list.
    pub fn with_allowed_contract(&mut self, contract: UInt160) -> &mut Self {
        self.allowed_contracts.push(contract);
        self
    }

    /// Adds a group to the allowed groups list.
    pub fn with_allowed_group(&mut self, group: ECPoint) -> &mut Self {
        self.allowed_groups.push(group);
        self
    }

    /// Alias for `with_allowed_contract`.
    pub fn allow_contract(&mut self, contract: UInt160) -> &mut Self {
        self.with_allowed_contract(contract)
    }

    /// Alias for `with_allowed_group`.
    pub fn allow_group(&mut self, group: ECPoint) -> &mut Self {
        self.with_allowed_group(group)
    }

    /// Adds a witness scope flag to the existing scopes.
    pub fn add_witness_scope(&mut self, scope: WitnessScope) -> &mut Self {
        self.scopes |= scope;
        self
    }

    /// Adds a witness rule with the specified action.
    pub fn add_witness_rule<F>(&mut self, action: WitnessRuleAction, config: F) -> &mut Self
    where
        F: FnOnce(&mut WitnessRuleBuilder),
    {
        let mut builder = WitnessRuleBuilder::new(action);
        config(&mut builder);
        self.rules.push(builder.build());
        self
    }

    /// Builds and returns the configured Signer.
    pub fn build(&self) -> Signer {
        let mut signer = Signer::new(self.account, self.scopes);
        signer.allowed_contracts = self.allowed_contracts.clone();
        signer.allowed_groups = self.allowed_groups.clone();
        signer.rules = self.rules.clone();
        signer
    }
}
