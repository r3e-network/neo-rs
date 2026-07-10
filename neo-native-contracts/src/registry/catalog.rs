//! Canonical catalog for Neo's standard native contracts.

use neo_config::Hardfork;
use neo_primitives::UInt160;

use crate::StandardNativeContract;

/// Number of canonical Neo N3 native contracts exposed by this crate.
pub const STANDARD_NATIVE_CONTRACT_COUNT: usize = 11;
/// Fixed-size canonical native-contract spec list in C# id order.
pub type StandardNativeContractSpecs = [StandardNativeContractSpec; STANDARD_NATIVE_CONTRACT_COUNT];
/// Fixed-size canonical native-contract hash list in C# id order.
pub type StandardNativeContractHashes = [UInt160; STANDARD_NATIVE_CONTRACT_COUNT];
type StandardNativeContractDescriptors =
    [StandardNativeContractDescriptor; STANDARD_NATIVE_CONTRACT_COUNT];

/// Exact C# `NativeContract.Activations` declaration for a native contract.
///
/// `None` represents C#'s nullable `Hardfork?` `null` entry, which means the
/// contract is genesis-active. This is intentionally separate from the
/// normalized `active_in`/`activations` view used by Rust dispatch code.
pub type NativeContractActivationSchedule = &'static [Option<Hardfork>];

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
    /// Rust-normalized hardforks that explicitly refresh stored native state.
    pub activations: &'static [Hardfork],
    /// Exact C# `Activations` schedule, including nullable genesis entries.
    pub csharp_activations: NativeContractActivationSchedule,
}

#[derive(Clone, Copy)]
struct StandardNativeContractDescriptor {
    id: i32,
    name: &'static str,
    hash: fn() -> UInt160,
    construct: fn() -> StandardNativeContract,
}

impl StandardNativeContractDescriptor {
    fn spec(self) -> StandardNativeContractSpec {
        let contract = self.contract();
        StandardNativeContractSpec {
            id: self.id,
            name: self.name,
            hash: (self.hash)(),
            active_in: contract.active_in(),
            activations: contract.activations(),
            csharp_activations: csharp_activation_schedule(self.name),
        }
    }

    fn contract(self) -> StandardNativeContract {
        (self.construct)()
    }
}

macro_rules! native_contract_descriptor {
    ($contract:ident, $variant:ident) => {
        StandardNativeContractDescriptor {
            id: crate::$contract::ID,
            name: crate::$contract::NAME,
            hash: crate::$contract::script_hash,
            construct: || StandardNativeContract::$variant(crate::$contract::new()),
        }
    };
}

fn standard_native_contract_descriptors() -> StandardNativeContractDescriptors {
    [
        native_contract_descriptor!(ContractManagement, ContractManagement),
        native_contract_descriptor!(StdLib, StdLib),
        native_contract_descriptor!(CryptoLib, CryptoLib),
        native_contract_descriptor!(LedgerContract, LedgerContract),
        native_contract_descriptor!(NeoToken, NeoToken),
        native_contract_descriptor!(GasToken, GasToken),
        native_contract_descriptor!(PolicyContract, PolicyContract),
        native_contract_descriptor!(RoleManagement, RoleManagement),
        native_contract_descriptor!(OracleContract, OracleContract),
        native_contract_descriptor!(Notary, Notary),
        native_contract_descriptor!(Treasury, Treasury),
    ]
}

fn csharp_activation_schedule(name: &str) -> NativeContractActivationSchedule {
    match name {
        // OracleContract.cs: `Activations => [null, Hardfork.HF_Faun]`.
        "OracleContract" => &[None, Some(Hardfork::HfFaun)],
        // Notary.cs: `Activations => [Hardfork.HF_Echidna, Hardfork.HF_Faun]`.
        "Notary" => &[Some(Hardfork::HfEchidna), Some(Hardfork::HfFaun)],
        // Treasury.cs: `Activations => [Hardfork.HF_Faun]`.
        "Treasury" => &[Some(Hardfork::HfFaun)],
        _ => &[],
    }
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
pub fn standard_native_contracts() -> Vec<StandardNativeContract> {
    StandardNativeContract::all().into_iter().collect()
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
#[path = "../tests/registry/catalog.rs"]
mod tests;
