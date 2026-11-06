//! Smart contract application engine, manifests, iterators, and storage.

pub mod application_engine;
pub mod contract;
pub mod contract_state;
pub mod diagnostic;
pub mod env_flags;
pub mod error;
pub mod execution_context_state;
pub mod helper;
pub mod interop_descriptor;
pub mod interop_parameter_descriptor;
pub mod iterators;
pub mod key_builder;
pub mod manifest;
pub mod max_length_attribute;
pub mod protocol_projection;
pub mod storage_item;
pub mod validator_attribute;
pub mod witness_verification;

// Compatibility re-exports preserving the pre-decomposition `neo-core`
// module paths that downstream native-contract sources still reference.
// `binary_serializer` / `storage_context` canonically live in `neo-vm`;
// `application_engine_contract` is the `application_engine::contract` module.
pub use application_engine::contract as application_engine_contract;
pub use neo_vm::binary_serializer;
pub use neo_vm::storage_context;
