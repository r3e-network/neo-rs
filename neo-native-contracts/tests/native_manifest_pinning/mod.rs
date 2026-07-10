//! # neo-native-contracts::tests::native_manifest_pinning
//!
//! Test module grouping native manifest pinning behavior coverage for neo-
//! native-contracts.
//!
//! ## Boundary
//!
//! This is test/benchmark-only code for neo-native-contracts; it may assemble
//! fixtures but must not introduce production behavior.
//!
//! ## Contents
//!
//! - `activation_semantics`: hardfork activation and manifest-refresh coverage.
//! - `contract_manifests`: native manifest pinning coverage.
//! - `fungible_and_policy`: fungible token and policy manifest coverage.
//! - `handles_and_catalog`: native handles and catalog coverage.
//! - `method_metadata`: native method metadata coverage.
//! - `safe_flags`: safe-method flag coverage.

use std::collections::HashMap;

use neo_config::{Hardfork, ProtocolSettings};
use neo_execution::native_contract::{NativeContract, build_native_contract_state};
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_native_contracts::{
    ContractManagement, CryptoLib, GasToken, LedgerContract, NeoToken, Notary, OracleContract,
    PolicyContract, RoleManagement, StandardNativeProvider, StdLib, Treasury,
    standard_native_contract_specs,
};
use neo_primitives::{CallFlags, ContractParameterType};

/// Test settings scheduling every hardfork at a distinct height so each
/// gating boundary can be probed: Aspidochelone=10, Basilisk=20,
/// Cockatrice=30, Domovoi=40, Echidna=50, Faun=60, Gorgon=70.
fn test_settings() -> ProtocolSettings {
    let mut hardforks = HashMap::new();
    hardforks.insert(Hardfork::HfAspidochelone, 10);
    hardforks.insert(Hardfork::HfBasilisk, 20);
    hardforks.insert(Hardfork::HfCockatrice, 30);
    hardforks.insert(Hardfork::HfDomovoi, 40);
    hardforks.insert(Hardfork::HfEchidna, 50);
    hardforks.insert(Hardfork::HfFaun, 60);
    hardforks.insert(Hardfork::HfGorgon, 70);
    ProtocolSettings {
        hardforks,
        ..ProtocolSettings::mainnet()
    }
}

/// A height at which every hardfork in `test_settings` is active.
const ALL_ACTIVE: u32 = 100;
/// Genesis: no hardfork active.
const GENESIS: u32 = 0;

/// The composed manifest's methods as `(name, [parameter names])`, in
/// manifest order (sorted by name then parameter count, like C#).
fn manifest_methods<C>(
    contract: &C,
    settings: &ProtocolSettings,
    height: u32,
) -> Vec<(String, Vec<String>)>
where
    C: NativeContract,
{
    build_native_contract_state(contract, settings, height)
        .manifest
        .abi
        .methods
        .iter()
        .map(|method| {
            (
                method.name.clone(),
                method
                    .parameters
                    .iter()
                    .map(|parameter| parameter.name.clone())
                    .collect(),
            )
        })
        .collect()
}

/// The composed manifest's events as `(name, [(param name, param type)])`,
/// in manifest order (the C# attribute order index).
fn manifest_events<C>(
    contract: &C,
    settings: &ProtocolSettings,
    height: u32,
) -> Vec<(String, Vec<(String, ContractParameterType)>)>
where
    C: NativeContract,
{
    build_native_contract_state(contract, settings, height)
        .manifest
        .abi
        .events
        .iter()
        .map(|event| {
            (
                event.name.clone(),
                event
                    .parameters
                    .iter()
                    .map(|parameter| (parameter.name.clone(), parameter.param_type))
                    .collect(),
            )
        })
        .collect()
}

/// The composed manifest's SAFE methods as `(name, parameter count)`, in
/// manifest order — the consensus-observable projection of each method's
/// `safe` flag (parameter count disambiguates same-name overloads like
/// `deploy`/`memorySearch`). The complement of this set against the full
/// method lists pinned per contract above is the not-safe set, so this pins
/// every method's safe flag.
fn manifest_safe_methods<C>(
    contract: &C,
    settings: &ProtocolSettings,
    height: u32,
) -> Vec<(String, usize)>
where
    C: NativeContract,
{
    build_native_contract_state(contract, settings, height)
        .manifest
        .abi
        .methods
        .iter()
        .filter(|method| method.safe)
        .map(|method| (method.name.clone(), method.parameters.len()))
        .collect()
}

/// Expectation literal: a manifest method entry.
fn m(name: &str, params: &[&str]) -> (String, Vec<String>) {
    (
        name.to_string(),
        params.iter().map(|p| (*p).to_string()).collect(),
    )
}

/// Expectation literal: a safe-method entry as `(name, parameter count)`.
fn s(name: &str, arity: usize) -> (String, usize) {
    (name.to_string(), arity)
}

/// Expectation literal: a manifest event entry.
fn e(
    name: &str,
    params: &[(&str, ContractParameterType)],
) -> (String, Vec<(String, ContractParameterType)>) {
    (
        name.to_string(),
        params.iter().map(|(p, t)| ((*p).to_string(), *t)).collect(),
    )
}

use ContractParameterType::{
    Any, Array, Boolean, Hash160, Hash256, Integer, PublicKey, String as StringT,
};

mod activation_semantics;
mod contract_manifests;
mod fungible_and_policy;
mod handles_and_catalog;
mod method_metadata;
mod safe_flags;
