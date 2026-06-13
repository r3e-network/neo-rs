//! Canonical catalog for Neo's standard native contracts.

use std::sync::Arc;

use neo_execution::NativeContract;
use neo_primitives::UInt160;

use crate::{
    ContractManagement, CryptoLib, GasToken, LedgerContract, NeoToken, Notary, OracleContract,
    PolicyContract, RoleManagement, StdLib, Treasury,
};

/// Metadata shared by every standard native contract handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StandardNativeContractSpec {
    /// Canonical native contract id.
    pub id: i32,
    /// Canonical native contract name.
    pub name: &'static str,
    /// Canonical native contract script hash.
    pub hash: UInt160,
}

macro_rules! for_each_standard_native_contract {
    ($macro:ident) => {
        $macro!(
            ContractManagement,
            StdLib,
            CryptoLib,
            LedgerContract,
            NeoToken,
            GasToken,
            PolicyContract,
            RoleManagement,
            OracleContract,
            Notary,
            Treasury,
        )
    };
}

macro_rules! build_specs {
    ($($contract:ident),+ $(,)?) => {
        [
            $(
                StandardNativeContractSpec {
                    id: $contract::ID,
                    name: $contract::NAME,
                    hash: $contract::script_hash(),
                },
            )+
        ]
    };
}

/// Returns the canonical standard native-contract catalog in C# id order.
pub fn standard_native_contract_specs() -> [StandardNativeContractSpec; 11] {
    for_each_standard_native_contract!(build_specs)
}

macro_rules! build_contracts {
    ($($contract:ident),+ $(,)?) => {
        vec![
            $(
                Arc::new($contract::new()) as Arc<dyn NativeContract>,
            )+
        ]
    };
}

pub(crate) fn standard_native_contracts() -> Vec<Arc<dyn NativeContract>> {
    for_each_standard_native_contract!(build_contracts)
}

pub(crate) fn is_standard_native_contract_hash(hash: &UInt160) -> bool {
    standard_native_contract_specs()
        .iter()
        .any(|spec| &spec.hash == hash)
}
