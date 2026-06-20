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
mod tests {
    use super::*;

    use neo_config::Hardfork;

    #[test]
    fn descriptors_match_constructed_contract_handles() {
        for descriptor in standard_native_contract_descriptors() {
            let contract = descriptor.contract();
            assert_eq!(contract.id(), descriptor.id, "{} id", descriptor.name);
            assert_eq!(contract.name(), descriptor.name, "{} name", descriptor.name);
            assert_eq!(
                contract.hash(),
                (descriptor.hash)(),
                "{} script hash",
                descriptor.name
            );
            assert_eq!(
                contract.active_in(),
                descriptor.active_in,
                "{} ActiveIn",
                descriptor.name
            );
            assert_eq!(
                contract.activations(),
                descriptor.activations,
                "{} activations",
                descriptor.name
            );
        }
    }

    #[test]
    fn standard_contract_handles_share_a_uniform_shape() {
        fn assert_handle<T>()
        where
            T: NativeContract + Default + Copy + Clone + std::fmt::Debug + 'static,
        {
        }

        assert_handle::<ContractManagement>();
        assert_handle::<StdLib>();
        assert_handle::<CryptoLib>();
        assert_handle::<LedgerContract>();
        assert_handle::<NeoToken>();
        assert_handle::<GasToken>();
        assert_handle::<PolicyContract>();
        assert_handle::<RoleManagement>();
        assert_handle::<OracleContract>();
        assert_handle::<Notary>();
        assert_handle::<Treasury>();
    }

    #[test]
    fn standard_contract_handles_are_const_constructible() {
        const CONTRACT_MANAGEMENT: ContractManagement = ContractManagement::new();
        const STD_LIB: StdLib = StdLib::new();
        const CRYPTO_LIB: CryptoLib = CryptoLib::new();
        const LEDGER_CONTRACT: LedgerContract = LedgerContract::new();
        const NEO_TOKEN: NeoToken = NeoToken::new();
        const GAS_TOKEN: GasToken = GasToken::new();
        const POLICY_CONTRACT: PolicyContract = PolicyContract::new();
        const ROLE_MANAGEMENT: RoleManagement = RoleManagement::new();
        const ORACLE_CONTRACT: OracleContract = OracleContract::new();
        const NOTARY: Notary = Notary::new();
        const TREASURY: Treasury = Treasury::new();

        assert_eq!(CONTRACT_MANAGEMENT.name(), "ContractManagement");
        assert_eq!(STD_LIB.name(), "StdLib");
        assert_eq!(CRYPTO_LIB.name(), "CryptoLib");
        assert_eq!(LEDGER_CONTRACT.name(), "LedgerContract");
        assert_eq!(NEO_TOKEN.name(), "NeoToken");
        assert_eq!(GAS_TOKEN.name(), "GasToken");
        assert_eq!(POLICY_CONTRACT.name(), "PolicyContract");
        assert_eq!(ROLE_MANAGEMENT.name(), "RoleManagement");
        assert_eq!(ORACLE_CONTRACT.name(), "OracleContract");
        assert_eq!(NOTARY.name(), "Notary");
        assert_eq!(TREASURY.name(), "Treasury");
    }

    #[test]
    fn standard_contract_activation_policy_matches_neo_n3_v3100() {
        let contracts = standard_native_contracts();
        let contract = |name: &str| {
            contracts
                .iter()
                .find(|contract| contract.name() == name)
                .unwrap_or_else(|| panic!("{name} should be in the standard native catalog"))
        };

        for native in &contracts {
            match native.name() {
                "Notary" => assert_eq!(native.active_in(), Some(Hardfork::HfEchidna)),
                "Treasury" => assert_eq!(native.active_in(), Some(Hardfork::HfFaun)),
                name => assert_eq!(
                    native.active_in(),
                    None,
                    "{name} should be genesis-active in Neo N3 v3.10.0"
                ),
            }
        }

        assert_eq!(contract("NeoToken").activations(), &[Hardfork::HfEchidna]);
        assert_eq!(
            contract("OracleContract").activations(),
            &[Hardfork::HfFaun]
        );
        assert_eq!(
            contract("Notary").activations(),
            &[Hardfork::HfEchidna, Hardfork::HfFaun]
        );
        assert_eq!(contract("Treasury").activations(), &[Hardfork::HfFaun]);

        for native in &contracts {
            match native.name() {
                "NeoToken" | "OracleContract" | "Notary" | "Treasury" => {}
                name => assert!(
                    native.activations().is_empty(),
                    "{name} should not declare manifest-refresh activations beyond method/event hardfork metadata"
                ),
            }
        }
    }

    #[test]
    fn specs_keep_canonical_native_contract_order() {
        use crate::hashes::{
            CONTRACT_MANAGEMENT_HASH, CRYPTO_LIB_HASH, GAS_TOKEN_HASH, LEDGER_CONTRACT_HASH,
            NEO_TOKEN_HASH, NOTARY_HASH, ORACLE_CONTRACT_HASH, POLICY_CONTRACT_HASH,
            ROLE_MANAGEMENT_HASH, STDLIB_HASH, TREASURY_HASH,
        };

        let specs = standard_native_contract_specs();
        assert_eq!(specs.len(), STANDARD_NATIVE_CONTRACT_COUNT);
        assert_eq!(
            specs.map(|spec| {
                (
                    spec.name,
                    spec.id,
                    spec.hash,
                    spec.active_in,
                    spec.activations,
                )
            }),
            [
                (
                    "ContractManagement",
                    -1,
                    *CONTRACT_MANAGEMENT_HASH,
                    None,
                    &[][..]
                ),
                ("StdLib", -2, *STDLIB_HASH, None, &[][..]),
                ("CryptoLib", -3, *CRYPTO_LIB_HASH, None, &[][..]),
                ("LedgerContract", -4, *LEDGER_CONTRACT_HASH, None, &[][..]),
                (
                    "NeoToken",
                    -5,
                    *NEO_TOKEN_HASH,
                    None,
                    &[Hardfork::HfEchidna][..],
                ),
                ("GasToken", -6, *GAS_TOKEN_HASH, None, &[][..]),
                ("PolicyContract", -7, *POLICY_CONTRACT_HASH, None, &[][..]),
                ("RoleManagement", -8, *ROLE_MANAGEMENT_HASH, None, &[][..]),
                (
                    "OracleContract",
                    -9,
                    *ORACLE_CONTRACT_HASH,
                    None,
                    &[Hardfork::HfFaun][..],
                ),
                (
                    "Notary",
                    -10,
                    *NOTARY_HASH,
                    Some(Hardfork::HfEchidna),
                    &[Hardfork::HfEchidna, Hardfork::HfFaun][..],
                ),
                (
                    "Treasury",
                    -11,
                    *TREASURY_HASH,
                    Some(Hardfork::HfFaun),
                    &[Hardfork::HfFaun][..],
                ),
            ]
        );
    }

    #[test]
    fn specs_have_unique_protocol_identifiers() {
        let specs = standard_native_contract_specs();
        for i in 0..specs.len() {
            for j in (i + 1)..specs.len() {
                assert_ne!(
                    specs[i].id, specs[j].id,
                    "duplicate native contract id for {} / {}",
                    specs[i].name, specs[j].name
                );
                assert_ne!(
                    specs[i].name, specs[j].name,
                    "duplicate native contract name for id {} / {}",
                    specs[i].id, specs[j].id
                );
                assert_ne!(
                    specs[i].hash, specs[j].hash,
                    "duplicate native contract hash for {} / {}",
                    specs[i].name, specs[j].name
                );
            }
        }
    }

    #[test]
    fn standard_hash_membership_matches_spec_catalog() {
        let specs = standard_native_contract_specs();
        assert_eq!(
            standard_native_contract_hashes(),
            specs.map(|spec| spec.hash)
        );
        for spec in specs {
            assert!(
                is_standard_native_contract_hash(&spec.hash),
                "{} hash should be recognized as standard",
                spec.name
            );
        }

        assert!(
            !is_standard_native_contract_hash(&UInt160::zero()),
            "zero hash must not be recognized as a standard native contract"
        );
    }

    #[test]
    fn spec_lookup_matches_canonical_catalog() {
        for spec in standard_native_contract_specs() {
            assert_eq!(
                standard_native_contract_spec_by_id(spec.id),
                Some(spec),
                "{} id lookup",
                spec.name
            );
            assert_eq!(
                standard_native_contract_spec_by_hash(&spec.hash),
                Some(spec),
                "{} hash lookup",
                spec.name
            );
            assert_eq!(
                standard_native_contract_spec_by_name(spec.name),
                Some(spec),
                "{} exact name lookup",
                spec.name
            );
            assert_eq!(
                standard_native_contract_spec_by_name(&spec.name.to_ascii_lowercase()),
                Some(spec),
                "{} case-insensitive name lookup",
                spec.name
            );
        }

        assert_eq!(standard_native_contract_spec_by_id(0), None);
        assert_eq!(
            standard_native_contract_spec_by_hash(&UInt160::zero()),
            None
        );
        assert_eq!(standard_native_contract_spec_by_name("MissingNative"), None);
    }
}
