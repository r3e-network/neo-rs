//! Native-manifest metadata pinning: method parameter NAMES, per-method
//! `safe` flags, and event lists.
//!
//! One test per native contract asserting the COMPOSED manifest ABI (via
//! `build_native_contract_state`, the same path that produces the stored,
//! consensus-observable native contract state) against hand-written
//! expectations derived from the vendored C# v3.10.0 sources — the `[ContractMethod]`
//! reflection parameter names and the `[ContractEvent]` constructor
//! attributes — NOT from the Rust method/event tables the implementation
//! itself uses.
//!
//! C# reference points (neo_csharp/src/Neo/SmartContract/Native):
//! - `ContractMethodMetadata.cs`: manifest parameter names = the C# method's
//!   reflection parameter names after the leading engine/snapshot parameter;
//!   manifest methods are ordered `OrderBy(Name, Ordinal).ThenBy(Parameters.Length)`.
//! - `ContractMethodMetadata.cs:74`: the manifest Safe flag is DERIVED, never
//!   hand-set: `Safe = (attribute.RequiredCallFlags & ~CallFlags.ReadOnly) == 0`
//!   with `ReadOnly = ReadStates | AllowCall` (CallFlags.cs:55) — i.e. a
//!   method is safe iff it needs neither WriteStates nor AllowNotify.
//! - `NativeContract.cs` (GetContractState): events =
//!   `_eventsDescriptors.Where(IsActive).Select(Descriptor)` with the
//!   declarations pre-sorted by the attribute's `order` argument.
//! - Each contract's `[ContractEvent]` attributes for names/params/gating.

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
fn manifest_methods(
    contract: &dyn NativeContract,
    settings: &ProtocolSettings,
    height: u32,
) -> Vec<(String, Vec<String>)> {
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
fn manifest_events(
    contract: &dyn NativeContract,
    settings: &ProtocolSettings,
    height: u32,
) -> Vec<(String, Vec<(String, ContractParameterType)>)> {
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
fn manifest_safe_methods(
    contract: &dyn NativeContract,
    settings: &ProtocolSettings,
    height: u32,
) -> Vec<(String, usize)> {
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

#[path = "native_manifest_pinning/contract_manifests.rs"]
mod contract_manifests;
#[path = "native_manifest_pinning/fungible_and_policy.rs"]
mod fungible_and_policy;
#[path = "native_manifest_pinning/handles_and_catalog.rs"]
mod handles_and_catalog;
#[path = "native_manifest_pinning/method_metadata.rs"]
mod method_metadata;
#[path = "native_manifest_pinning/safe_flags.rs"]
mod safe_flags;
