use neo_payloads::{Signer, Transaction, Witness};
use neo_script_builder::ScriptBuilder;
use neo_vm_rs::OpCode;

use super::{SignerBuilder, TransactionAttributesBuilder, WitnessBuilder};

/// Convenience builder for constructing transactions in tests.
#[derive(Default)]
#[must_use]
pub struct TransactionBuilder {
    inner: Transaction,
}

impl TransactionBuilder {
    /// Creates a builder seeded with an empty transaction.
    pub fn new() -> Self {
        let mut tx = Transaction::new();
        tx.set_script(vec![OpCode::RET.byte()]);
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
        let mut builder = TransactionAttributesBuilder::new();
        config(&mut builder);
        self.inner.set_attributes(builder.build());
        self
    }

    /// Adds a witness using a builder.
    pub fn add_witness<F>(mut self, config: F) -> Self
    where
        F: FnOnce(&mut WitnessBuilder),
    {
        let mut builder = WitnessBuilder::new();
        config(&mut builder);
        self.inner.add_witness(builder.build());
        self
    }

    /// Adds a witness using a builder with access to the transaction.
    pub fn add_witness_with_tx<F>(mut self, config: F) -> Self
    where
        F: FnOnce(&mut WitnessBuilder, &Transaction),
    {
        let mut builder = WitnessBuilder::new();
        config(&mut builder, &self.inner);
        self.inner.add_witness(builder.build());
        self
    }

    /// Adds a signer using a builder with access to the transaction.
    pub fn add_signer<F>(mut self, config: F) -> Self
    where
        F: FnOnce(&mut SignerBuilder, &Transaction),
    {
        let mut builder = SignerBuilder::new();
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
