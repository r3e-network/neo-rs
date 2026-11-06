pub mod attribute;
pub mod block;
pub mod header;
pub mod header_cache;
pub mod resolver;
pub mod rules;
pub mod transaction;
pub mod transaction_context;
pub mod transaction_witness;
pub mod witness;
pub mod witness_crypto;

pub use attribute::{attribute_network_fee, oracle_fixed_script, verify_attribute};
pub use block::{calculate_block_network_fee, verify_block, verify_block_with_cache};
pub use header::{
    HEADER_VERIFY_GAS, validate_header_against_previous, verify_header, verify_header_with_cache,
    verify_header_witness_against_hash,
};
pub use header_cache::{HeaderCache, HeaderCacheIter, MAX_HEADERS};
pub use resolver::{
    LedgerWitnessHashResolver, VerifiableExt, block_script_hashes_for_verifying,
    header_script_hashes_for_verifying, transaction_script_hashes_for_verifying,
};
pub use rules::{
    BlockLike, BlockValidationError, MAX_TIMESTAMP_DRIFT_MS, MIN_TIMESTAMP_MS, validate_block_size,
    validate_block_size_raw, validate_block_version, validate_merkle_root,
    validate_no_duplicate_transactions, validate_primary_index, validate_timestamp_bounds,
    validate_timestamp_progression, validate_transaction_count, validate_transaction_count_raw,
    validate_witness_scripts,
};
pub use transaction::{
    transaction_sign_data, verify_transaction, verify_transaction_state_dependent,
    verify_transaction_state_dependent_at_height, verify_transaction_state_independent,
};
pub use transaction_context::TransactionVerificationContext;
pub use transaction_witness::{
    StandardWitnessVerification, parse_single_signature_contract,
    parse_single_signature_invocation, verify_standard_witness, verify_transaction_witness,
};
pub use witness::{verify_witness, verify_witnesses};
pub use witness_crypto::{verify_witness_multi_signature, verify_witness_signature};

#[cfg(test)]
mod block_validation_compliance_tests;
#[cfg(test)]
mod block_validation_refinement_tests;
#[cfg(test)]
mod header_tests;
#[cfg(test)]
mod multisig_validation_refinement_tests;
#[cfg(test)]
mod transaction_context_tests;
#[cfg(test)]
mod witness_tests;
#[cfg(test)]
mod witness_validation_refinement_tests;
