use crate::UInt256;
use crate::network::p2p::payloads::{
    Conflicts, NotValidBefore, OracleResponse, OracleResponseCode, TransactionAttribute,
};

/// Builder for transaction attributes.
#[must_use]
pub struct TransactionAttributesBuilder {
    attributes: Vec<TransactionAttribute>,
}

impl Default for TransactionAttributesBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl TransactionAttributesBuilder {
    pub fn new() -> Self {
        Self {
            attributes: Vec::new(),
        }
    }

    /// Adds a HighPriority attribute to the transaction.
    ///
    /// # Panics
    /// Panics if a HighPriority attribute already exists (only one allowed per transaction).
    pub fn add_high_priority(&mut self) -> &mut Self {
        assert!(
            !self
                .attributes
                .iter()
                .any(|attr| matches!(attr, TransactionAttribute::HighPriority)),
            "HighPriority attribute already exists. Only one allowed per transaction."
        );
        self.attributes.push(TransactionAttribute::HighPriority);
        self
    }

    /// Adds a Conflicts attribute to the transaction.
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

    /// Adds an OracleResponse attribute to the transaction.
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

    /// Adds a NotValidBefore attribute to the transaction.
    ///
    /// # Panics
    /// Panics if a NotValidBefore attribute for the same height already exists.
    pub fn add_not_valid_before(&mut self, height: u32) -> &mut Self {
        assert!(
            !self.attributes.iter().any(|attr| matches!(attr, TransactionAttribute::NotValidBefore(existing) if existing.height == height)),
            "NotValidBefore attribute for block {} already exists", height
        );
        self.attributes
            .push(TransactionAttribute::NotValidBefore(NotValidBefore::new(
                height,
            )));
        self
    }

    /// Builds and returns the configured attributes.
    pub fn build(&self) -> Vec<TransactionAttribute> {
        self.attributes.clone()
    }
}
