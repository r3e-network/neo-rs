//! Normative Neo payload data and codecs, without VM or snapshot validation.

pub mod block;
pub mod conflicts;
pub mod extensible;
pub mod header;
pub mod high_priority_attribute;
pub mod not_valid_before;
pub mod notary_assisted;
pub mod oracle_response;
pub mod signer;
pub mod transaction;
pub mod transaction_attribute;

/// The canonical protocol witness type.
pub mod witness {
    pub use crate::witness::{Witness, WitnessData};
}
/// The primitive oracle response code authority.
pub mod oracle_response_code {
    pub use neo_primitives::OracleResponseCode;
}

pub use crate::witness::Witness;
pub use block::Block;
pub use conflicts::Conflicts;
pub use extensible::Extensible;
pub use header::Header;
pub use high_priority_attribute::HighPriorityAttribute;
pub use neo_primitives::{
    Inventory, InventoryType, OracleResponseCode, TransactionAttributeType, WitnessCondition,
    WitnessRule, WitnessRuleAction, WitnessScope,
};
pub use not_valid_before::NotValidBefore;
pub use notary_assisted::NotaryAssisted;
pub use oracle_response::{OracleResponse, get_fixed_script};
pub use signer::{MAX_SIGNER_SUBITEMS, Signer};
pub use transaction::Transaction;
pub use transaction_attribute::TransactionAttribute;
