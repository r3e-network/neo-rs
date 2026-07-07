//! # neo-payloads::transaction_attribute
//!
//! Transaction attribute records and validation helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-payloads`. This protocol crate owns payload
//! records and validation helpers and must not perform IO, storage commits, or
//! service orchestration.
//!
//! ## Contents
//!
//! - `conflicts`: transaction conflict attribute records.
//! - `fees`: Transaction attribute network-fee calculation.
//! - `high_priority_attribute`: high-priority transaction attribute records.
//! - `json`: JSON projection for transaction attributes.
//! - `not_valid_before`: NotValidBefore transaction attribute records.
//! - `notary_assisted`: NotaryAssisted transaction attribute records.
//! - `oracle_response`: OracleResponse transaction attribute records.
//! - `tests`: Module-local tests and regression coverage.
//! - `wire`: transaction attribute wire serialization and type-byte dispatch.

/// Conflicting transaction reference attribute.
pub mod conflicts;
mod fees;
/// High-priority transaction marker attribute.
pub mod high_priority_attribute;
mod json;
/// Height gate for transaction validity.
pub mod not_valid_before;
/// Notary-assisted transaction attribute.
pub mod notary_assisted;
/// Oracle response transaction attribute.
pub mod oracle_response;
mod wire;

use self::{
    conflicts::Conflicts, not_valid_before::NotValidBefore, notary_assisted::NotaryAssisted,
    oracle_response::OracleResponse,
};
use crate::{OracleResponseCode, TransactionAttributeType};
use serde::{Deserialize, Serialize};

/// Represents an attribute of a transaction.
/// Matches C# TransactionAttribute abstract class.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionAttribute {
    /// High priority attribute
    HighPriority,
    /// Oracle response
    OracleResponse(OracleResponse),
    /// Not valid before attribute
    NotValidBefore(NotValidBefore),
    /// Conflicts attribute
    Conflicts(Conflicts),
    /// Notary assisted attribute
    NotaryAssisted(NotaryAssisted),
}

impl TransactionAttribute {
    /// Convenience constructor for the high priority attribute.
    pub fn high_priority() -> Self {
        Self::HighPriority
    }

    /// Convenience constructor for an oracle response with default success code and empty result.
    pub fn oracle_response(id: u64) -> Self {
        Self::OracleResponse(OracleResponse::new(
            id,
            OracleResponseCode::Success,
            Vec::new(),
        ))
    }

    /// Convenience constructor for a "not valid before" attribute.
    pub fn not_valid_before(height: u32) -> Self {
        Self::NotValidBefore(NotValidBefore::new(height))
    }

    /// Alias for type_id() to match C# naming.
    pub fn attribute_type(&self) -> TransactionAttributeType {
        self.type_id()
    }

    /// Indicates whether multiple instances of this attribute are allowed.
    /// Matches C# AllowMultiple property.
    pub fn allow_multiple(&self) -> bool {
        self.type_id().allows_multiple()
    }

    // verify: Matches C# Verify method. Handled by attribute-type dispatch.
}

#[cfg(test)]
#[path = "../tests/transaction_attribute/transaction_attribute.rs"]
mod tests;
