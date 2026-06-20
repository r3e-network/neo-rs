//! JSON-RPC server parameter model helpers.

/// Address parameter model.
pub mod address;
/// Block hash-or-index parameter model.
pub mod block_hash_or_index;
/// Contract name, hash, or id parameter model.
pub mod contract_name_or_hash_or_id;
/// Signers and witnesses parameter bundle.
pub mod signers_and_witnesses;

pub use address::Address;
pub use block_hash_or_index::BlockHashOrIndex;
pub use contract_name_or_hash_or_id::ContractNameOrHashOrId;
pub use signers_and_witnesses::SignersAndWitnesses;
