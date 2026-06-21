//! Canonical catalog for Neo's standard native contracts.

use std::sync::Arc;

use neo_config::Hardfork;
use neo_execution::NativeContract;
use neo_primitives::UInt160;

use crate::{
    ContractManagement, CryptoLib, GasToken, LedgerContract, NeoToken, Notary, OracleContract,
    PolicyContract, RoleManagement, StdLib, Treasury,
};

/// Number of canonical Neo N3 native contracts exposed by this crate.
pub const STANDARD_NATIVE_CONTRACT_COUNT: usize = 11;
/// Fixed-size canonical native-contract spec list in C# id order.
pub type StandardNativeContractSpecs = [StandardNativeContractSpec; STANDARD_NATIVE_CONTRACT_COUNT];
/// Fixed-size canonical native-contract hash list in C# id order.
pub type StandardNativeContractHashes = [UInt160; STANDARD_NATIVE_CONTRACT_COUNT];
type StandardNativeContractDescriptors =
    [StandardNativeContractDescriptor; STANDARD_NATIVE_CONTRACT_COUNT];

/// Metadata shared by every standard native contract handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StandardNativeContractSpec {
    /// Canonical native contract id.
    pub id: i32,
    /// Canonical native contract name.
    pub name: &'static str,
    /// Canonical native contract script hash.
    pub hash: UInt160,
    /// Hardfork that activates the contract itself.
    pub active_in: Option<Hardfork>,
    /// Hardforks that explicitly refresh the stored native contract state.
    pub activations: &'static [Hardfork],
}

#[derive(Clone, Copy)]
struct StandardNativeContractDescriptor {
    id: i32,
    name: &'static str,
    hash: fn() -> UInt160,
    active_in: Option<Hardfork>,
    activations: &'static [Hardfork],
    construct: fn() -> Arc<dyn NativeContract>,
}

impl StandardNativeContractDescriptor {
    fn spec(self) -> StandardNativeContractSpec {
        StandardNativeContractSpec {
            id: self.id,
            name: self.name,
            hash: (self.hash)(),
            active_in: self.active_in,
            activations: self.activations,
        }
    }

    fn contract(self) -> Arc<dyn NativeContract> {
        (self.construct)()
    }
}

macro_rules! native_contract_descriptor {
    ($contract:ident) => {
        native_contract_descriptor!($contract, active_in: None, activations: &[])
    };
    ($contract:ident, activations: $activations:expr) => {
        native_contract_descriptor!($contract, active_in: None, activations: $activations)
    };
    ($contract:ident, active_in: $active_in:expr) => {
        native_contract_descriptor!($contract, active_in: $active_in, activations: &[])
    };
    ($contract:ident, active_in: $active_in:expr, activations: $activations:expr) => {
        StandardNativeContractDescriptor {
            id: $contract::ID,
            name: $contract::NAME,
            hash: $contract::script_hash,
            active_in: $active_in,
            activations: $activations,
            construct: || Arc::new($contract::new()) as Arc<dyn NativeContract>,
        }
    };
}

fn standard_native_contract_descriptors() -> StandardNativeContractDescriptors {
    [
        native_contract_descriptor!(ContractManagement),
        native_contract_descriptor!(StdLib),
        native_contract_descriptor!(CryptoLib),
        native_contract_descriptor!(LedgerContract),
        native_contract_descriptor!(NeoToken, activations: &[Hardfork::HfEchidna]),
        native_contract_descriptor!(GasToken),
        native_contract_descriptor!(PolicyContract),
        native_contract_descriptor!(RoleManagement),
        native_contract_descriptor!(OracleContract, activations: &[Hardfork::HfFaun]),
        native_contract_descriptor!(
            Notary,
            active_in: Some(Hardfork::HfEchidna),
            activations: &[Hardfork::HfEchidna, Hardfork::HfFaun]
        ),
        native_contract_descriptor!(
            Treasury,
            active_in: Some(Hardfork::HfFaun),
            activations: &[Hardfork::HfFaun]
        ),
    ]
}

/// Returns the canonical standard native-contract catalog in C# id order.
pub fn standard_native_contract_specs() -> StandardNativeContractSpecs {
    standard_native_contract_descriptors().map(StandardNativeContractDescriptor::spec)
}

/// Returns the canonical standard native-contract hashes in C# id order.
pub fn standard_native_contract_hashes() -> StandardNativeContractHashes {
    standard_native_contract_specs().map(|spec| spec.hash)
}

/// Returns freshly constructed handles for the canonical standard
/// native-contract set in C# id order.
pub fn standard_native_contracts() -> Vec<Arc<dyn NativeContract>> {
    standard_native_contract_descriptors()
        .into_iter()
        .map(StandardNativeContractDescriptor::contract)
        .collect()
}

fn standard_native_contract_spec_by(
    mut predicate: impl FnMut(&StandardNativeContractSpec) -> bool,
) -> Option<StandardNativeContractSpec> {
    standard_native_contract_descriptors()
        .into_iter()
        .map(StandardNativeContractDescriptor::spec)
        .find(|spec| predicate(spec))
}

/// Returns metadata for the standard native contract with `id`.
pub fn standard_native_contract_spec_by_id(id: i32) -> Option<StandardNativeContractSpec> {
    standard_native_contract_spec_by(|spec| spec.id == id)
}

/// Returns metadata for the standard native contract with `hash`.
pub fn standard_native_contract_spec_by_hash(hash: &UInt160) -> Option<StandardNativeContractSpec> {
    standard_native_contract_spec_by(|spec| &spec.hash == hash)
}

/// Returns metadata for the standard native contract named `name`.
///
/// Matching is ASCII-case-insensitive, like the standard provider's
/// name-based native-contract lookup.
pub fn standard_native_contract_spec_by_name(name: &str) -> Option<StandardNativeContractSpec> {
    standard_native_contract_spec_by(|spec| spec.name.eq_ignore_ascii_case(name))
}

/// Returns `true` when `hash` is one of the 11 standard Neo N3 native
/// contract script hashes.
pub fn is_standard_native_contract_hash(hash: &UInt160) -> bool {
    standard_native_contract_spec_by_hash(hash).is_some()
}

#[cfg(test)]
#[path = "tests/catalog.rs"]
mod tests;
