//! Minimal builder helpers mirroring the C# `Neo.Builder` utilities used by
//! the test suite. These builders intentionally expose only the functionality
//! currently required by the tests while keeping the API ergonomic for future
//! extensions.

use crate::network::p2p::payloads::{Signer, Transaction, Witness};
use crate::{cryptography::crypto_utils::ECPoint, UInt160, WitnessScope};

/// Convenience builder for constructing transactions in tests.
#[derive(Default)]
pub struct TransactionBuilder {
    inner: Transaction,
}

impl TransactionBuilder {
    /// Creates a builder seeded with an empty transaction.
    pub fn create_empty() -> Self {
        Self {
            inner: Transaction::new(),
        }
    }

    /// Sets the script for the transaction being built.
    pub fn script(mut self, script: Vec<u8>) -> Self {
        self.inner.set_script(script);
        self
    }

    /// Sets the nonce for the transaction being built.
    pub fn nonce(mut self, nonce: u32) -> Self {
        self.inner.set_nonce(nonce);
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
}

impl SignerBuilder {
    /// Creates a builder with default signer settings (zero account,
    /// `CalledByEntry` scope).
    pub fn create_empty() -> Self {
        Self {
            account: UInt160::zero(),
            scopes: WitnessScope::CALLED_BY_ENTRY,
            allowed_contracts: Vec::new(),
            allowed_groups: Vec::new(),
        }
    }

    pub fn account(mut self, account: UInt160) -> Self {
        self.account = account;
        self
    }

    pub fn scope(mut self, scope: WitnessScope) -> Self {
        self.scopes = scope;
        self
    }

    pub fn with_allowed_contract(mut self, contract: UInt160) -> Self {
        self.allowed_contracts.push(contract);
        self
    }

    pub fn with_allowed_group(mut self, group: ECPoint) -> Self {
        self.allowed_groups.push(group);
        self
    }

    pub fn build(self) -> Signer {
        let mut signer = Signer::new(self.account, self.scopes);
        signer.allowed_contracts = self.allowed_contracts;
        signer.allowed_groups = self.allowed_groups;
        signer
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

    pub fn build(self) -> Witness {
        if self.invocation.is_empty() && self.verification.is_empty() {
            Witness::new()
        } else {
            Witness::new_with_scripts(self.invocation, self.verification)
        }
    }
}
