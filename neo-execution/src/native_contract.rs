//! Base native contract trait and types.
//!
//! This module owns the abstract [`NativeContract`] trait and the
//! [`NativeMethod`] data struct that the application engine and the
//! concrete native contracts (in `neo-native-contracts`) both depend
//! on. The trait is defined here (with the engine, the consumer) so
//! that the engine can dispatch `System.Contract.CallNative` against
//! any `Arc<dyn NativeContract>` without depending on
//! `neo-native-contracts` directly.
//!
//! The concrete native-contract implementations live in
//! `neo-native-contracts`; the [`build_native_contract_state`] helper that
//! composes their on-disk `ContractState` (NEF + manifest) lives here with
//! the trait it consumes.

use neo_primitives::{ContractParameterType, UInt160};

use neo_config::{Hardfork, ProtocolSettings};
use neo_error::CoreResult;
use neo_manifest::{
    ContractAbi, ContractEventDescriptor, ContractManifest, ContractMethodDescriptor,
    ContractParameterDefinition, NefFile,
};
use neo_vm::script_builder::ScriptBuilder;
use neo_vm_rs::OpCode;
use serde::{Deserialize, Serialize};
use std::any::Any;

use crate::application_engine::ApplicationEngine;
use crate::contract_state::ContractState;
use crate::hardfork_activable::HardforkActivable;

pub use crate::native_contract_cache::{NativeContractsCache, NativeContractsCacheEntry};

/// Trait for native contract implementations.
pub trait NativeContract: Any + Send + Sync {
    /// Gets the unique identifier of the native contract.
    fn id(&self) -> i32;

    /// Gets the hash of the native contract.
    fn hash(&self) -> UInt160;

    /// Gets the name of the native contract.
    fn name(&self) -> &str;

    /// Hardfork that activates this contract (matches C# `NativeContract.ActiveIn`).
    fn active_in(&self) -> Option<Hardfork> {
        None
    }

    /// Gets the supported methods of the native contract.
    fn methods(&self) -> &[NativeMethod];

    /// Determines whether the native contract is active under the given settings.
    fn is_active(&self, settings: &ProtocolSettings, block_height: u32) -> bool {
        match self.active_in() {
            // C# NativeContract.IsActive (NativeContract.cs:341) does NOT route
            // through ProtocolSettings.IsHardforkEnabled (which treats an
            // unconfigured hardfork as disabled). It reads the configured
            // activation height itself and falls back to 0 when the ActiveIn
            // hardfork is NOT configured: "If is not set in the configuration
            // is treated as enabled from the genesis". Note the deliberate C#
            // asymmetry with IsInitializeBlock (NativeContract.cs:297+), which
            // SKIPS unconfigured hardforks ("treated as disabled"): a native
            // whose ActiveIn hardfork is unconfigured is active from genesis
            // (its hooks run, it passes CallNative gating) but is never
            // deployed or hardfork-initialized.
            Some(hardfork) => {
                settings.hardforks.get(&hardfork).copied().unwrap_or(0) <= block_height
            }
            None => true,
        }
    }

    /// Returns hardforks that should trigger a contract manifest refresh.
    ///
    /// This mirrors C# `NativeContract.Activations` for manifest-only updates
    /// such as supported standards changes.
    fn activations(&self) -> Vec<Hardfork> {
        Vec::new()
    }

    /// Returns the hardforks that affect this contract (methods/events/activations).
    ///
    /// Mirrors C# NativeContract `_usedHardforks`, which concatenates the
    /// `ActiveIn`/`DeprecatedIn` hardforks of the method descriptors, the
    /// event descriptors, and `Activations`. Event hardforks matter for
    /// `is_initialize_block`: e.g. C# refreshes RoleManagement's stored
    /// manifest at the `HF_Echidna` boundary purely because the `Designation`
    /// event signature changes there.
    fn used_hardforks(&self) -> Vec<Hardfork> {
        let mut hardforks = Vec::new();

        if let Some(hardfork) = self.active_in() {
            hardforks.push(hardfork);
        }

        for method in self.methods() {
            if let Some(hardfork) = method.active_in {
                hardforks.push(hardfork);
            }
            if let Some(hardfork) = method.deprecated_in {
                hardforks.push(hardfork);
            }
        }

        for event in self.event_descriptors() {
            if let Some(hardfork) = event.active_in {
                hardforks.push(hardfork);
            }
            if let Some(hardfork) = event.deprecated_in {
                hardforks.push(hardfork);
            }
        }

        hardforks.extend(self.activations());
        hardforks.sort();
        hardforks.dedup();
        hardforks
    }

    /// Returns whether this block should initialize/refresh the native contract.
    ///
    /// Matches C# `NativeContract.IsInitializeBlock` semantics.
    fn is_initialize_block(
        &self,
        settings: &ProtocolSettings,
        index: u32,
    ) -> (bool, Vec<Hardfork>) {
        let mut hits = Vec::new();
        for hardfork in self.used_hardforks() {
            if let Some(&activation_height) = settings.hardforks.get(&hardfork) {
                if activation_height == index {
                    hits.push(hardfork);
                }
            }
        }

        if !hits.is_empty() {
            return (true, hits);
        }

        if index == 0 && self.active_in().is_none() {
            return (true, Vec::new());
        }

        (false, Vec::new())
    }

    /// Returns the contract state metadata if available.
    fn contract_state(
        &self,
        settings: &ProtocolSettings,
        block_height: u32,
    ) -> Option<ContractState> {
        if !self.is_active(settings, block_height) {
            return None;
        }
        Some(build_native_contract_state(self, settings, block_height))
    }

    /// Returns supported standards for the contract manifest.
    fn supported_standards(&self, _settings: &ProtocolSettings, _block_height: u32) -> Vec<String> {
        Vec::new()
    }

    /// Returns the full (unfiltered) event declarations of this contract.
    ///
    /// Mirrors C# `NativeContract._eventsDescriptors`: the `[ContractEvent]`
    /// attributes on the contract's constructor (including the base type's,
    /// which is how `FungibleToken.Transfer` reaches NEO and GAS). The
    /// manifest projection ([`NativeContract::events`]) filters these by
    /// hardfork activation and orders them by [`NativeEvent::order`].
    fn event_descriptors(&self) -> &[NativeEvent] {
        &[]
    }

    /// Returns event descriptors for the contract manifest ABI.
    ///
    /// Mirrors C# `GetContractState`:
    /// `Events = _eventsDescriptors.Where(u => IsActive(u, hfChecker, blockHeight))
    /// .Select(p => p.Descriptor)` where `_eventsDescriptors` is pre-sorted by
    /// `OrderBy(p => p.Order)` (a stable sort, so dual registrations sharing an
    /// order index keep their declaration order).
    fn events(
        &self,
        settings: &ProtocolSettings,
        block_height: u32,
    ) -> Vec<ContractEventDescriptor> {
        let mut declared: Vec<&NativeEvent> = self
            .event_descriptors()
            .iter()
            .filter(|event| {
                is_active_for(
                    *event,
                    |hardfork, height| settings.is_hardfork_enabled(hardfork, height),
                    block_height,
                )
            })
            .collect();
        declared.sort_by_key(|event| event.order);
        declared
            .into_iter()
            .map(|event| event.descriptor.clone())
            .collect()
    }

    /// Invokes a method on the native contract.
    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>>;

    /// Called when the contract is initialized.
    fn initialize(&self, _engine: &mut ApplicationEngine) -> CoreResult<()> {
        Ok(())
    }

    /// Called on each block persistence.
    fn on_persist(&self, _engine: &mut ApplicationEngine) -> CoreResult<()> {
        Ok(())
    }

    /// Called after block persistence.
    fn post_persist(&self, _engine: &mut ApplicationEngine) -> CoreResult<()> {
        Ok(())
    }

    /// Returns the contract state for a deployed contract by hash, if
    /// this contract stores contract-state records.
    ///
    /// The default implementation returns `Ok(None)`; only
    /// `ContractManagement` overrides this to query the storage.
    fn lookup_contract_state(
        &self,
        _snapshot: &neo_storage::DataCache,
        _hash: &neo_primitives::UInt160,
    ) -> CoreResult<Option<crate::ContractState>> {
        Ok(None)
    }

    /// Returns whether the given contract hash is currently blocked
    /// from being invoked.
    ///
    /// The default implementation returns `Ok(false)`; only
    /// `PolicyContract` overrides this.
    fn is_contract_blocked(
        &self,
        _snapshot: &neo_storage::DataCache,
        _contract_hash: &neo_primitives::UInt160,
    ) -> CoreResult<bool> {
        Ok(false)
    }

    /// Returns the committee multisig address (C# `NEO.GetCommitteeAddress`),
    /// used by `check_committee_witness` to authorize committee-gated operations.
    ///
    /// The default implementation returns `Ok(None)`; only `NeoToken` (which
    /// owns the committee cache) overrides this.
    fn committee_address(
        &self,
        _snapshot: &neo_storage::DataCache,
    ) -> CoreResult<Option<neo_primitives::UInt160>> {
        Ok(None)
    }

    /// Returns the whitelisted fee (in datoshi) for the given contract
    /// method, or `None` if no whitelist applies.
    ///
    /// The default implementation returns `Ok(None)`; only
    /// `PolicyContract` overrides this.
    fn whitelisted_fee(
        &self,
        _snapshot: &neo_storage::DataCache,
        _contract_hash: &neo_primitives::UInt160,
        _method: &str,
        _param_count: u32,
    ) -> CoreResult<Option<i64>> {
        Ok(None)
    }

    /// Returns the URL and original txid of the oracle request for a given
    /// request ID, if any.
    ///
    /// The default implementation returns `Ok(None)`; only
    /// `OracleContract` overrides this. Consumed by the engine's
    /// oracle-response witness path (`CheckWitness` signer inheritance).
    fn oracle_request_url_full(
        &self,
        _snapshot: &neo_storage::DataCache,
        _id: u64,
    ) -> CoreResult<Option<OracleRequestDetails>> {
        Ok(None)
    }

    /// Returns the transaction state for a given transaction hash.
    fn transaction_state(
        &self,
        _snapshot: &neo_storage::DataCache,
        _tx_hash: &neo_primitives::UInt256,
    ) -> CoreResult<Option<neo_payloads::TransactionState>> {
        Ok(None)
    }

    /// Returns the default execution fee factor used when no
    /// policy-set value is configured.
    fn default_exec_fee_factor(&self) -> u32 {
        30
    }

    /// Returns the default storage price used when no policy-set
    /// value is configured.
    fn default_storage_price(&self) -> u32 {
        100_000
    }

    /// Returns a reference to self as `Any` for downcasting.
    fn as_any(&self) -> &dyn Any;
}

/// Lightweight oracle request descriptor used by the engine.
#[derive(Clone, Debug)]
pub struct OracleRequestDetails {
    /// The URL the oracle was asked to fetch.
    pub url: String,
    /// The original transaction id that submitted the request.
    pub original_tx_id: neo_primitives::UInt256,
}

impl OracleRequestDetails {
    /// Creates a new request detail.
    pub fn new(url: impl Into<String>, original_tx_id: neo_primitives::UInt256) -> Self {
        Self {
            url: url.into(),
            original_tx_id,
        }
    }
}

/// Represents a method in a native contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeMethod {
    /// The name of the method.
    pub name: String,

    /// The CPU fee for the method (in execution units, multiplied by `ExecFeeFactor`).
    #[serde(alias = "gas_cost")]
    pub cpu_fee: i64,

    /// The storage fee for the method (in storage units, multiplied by `StoragePrice`).
    #[serde(default)]
    pub storage_fee: i64,

    /// Whether the method is safe (read-only).
    pub safe: bool,

    /// The required call flags for this method.
    pub required_call_flags: u8,

    /// Parameter types for the method (used for native manifest ABI).
    pub parameters: Vec<ContractParameterType>,

    /// Parameter names for the method (used for native manifest ABI).
    ///
    /// When omitted or when the length does not match `parameters`, the ABI
    /// builder falls back to `arg{index}` names.
    #[serde(default)]
    pub parameter_names: Vec<String>,

    /// Hardfork that activates this method (matches C# `ContractMethodAttribute.ActiveIn`).
    #[serde(default)]
    pub active_in: Option<Hardfork>,

    /// Hardfork that deprecates this method (matches C# `ContractMethodAttribute.DeprecatedIn`).
    #[serde(default)]
    pub deprecated_in: Option<Hardfork>,

    /// Return type for the method (used for native manifest ABI).
    pub return_type: ContractParameterType,
}

impl NativeMethod {
    /// Creates a new native method.
    pub fn new(
        name: String,
        cpu_fee: i64,
        safe: bool,
        required_call_flags: u8,
        parameters: Vec<ContractParameterType>,
        return_type: ContractParameterType,
    ) -> Self {
        Self {
            name,
            cpu_fee,
            storage_fee: 0,
            safe,
            required_call_flags,
            parameters,
            parameter_names: Vec::new(),
            active_in: None,
            deprecated_in: None,
            return_type,
        }
    }

    /// Marks the method as active starting from the given hardfork (inclusive).
    pub fn with_active_in(mut self, hardfork: Hardfork) -> Self {
        self.active_in = Some(hardfork);
        self
    }

    /// Marks the method as deprecated starting from the given hardfork (inclusive).
    pub fn with_deprecated_in(mut self, hardfork: Hardfork) -> Self {
        self.deprecated_in = Some(hardfork);
        self
    }

    /// Overrides the required call flags for this method.
    pub fn with_required_call_flags(mut self, flags: neo_manifest::CallFlags) -> Self {
        self.required_call_flags = flags.bits();
        self
    }

    /// Sets parameter names used in the generated native manifest ABI.
    ///
    /// The names must be the C# reflection parameter names of the
    /// corresponding `[ContractMethod]` (the signature parameters after the
    /// leading `ApplicationEngine`/`DataCache`/`IReadOnlyStore` one); the ABI
    /// builder falls back to `arg{index}` when unset.
    pub fn with_parameter_names<I, S>(mut self, parameter_names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.parameter_names = parameter_names.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the storage fee for this method (multiplied by `StoragePrice`).
    pub fn with_storage_fee(mut self, storage_fee: i64) -> Self {
        self.storage_fee = storage_fee;
        self
    }

    /// Returns true if this method is active at the given height.
    pub fn is_active(&self, settings: &ProtocolSettings, block_height: u32) -> bool {
        is_active_for(
            self,
            |hardfork, height| settings.is_hardfork_enabled(hardfork, height),
            block_height,
        )
    }

    /// Creates a new safe (read-only) method.
    pub fn safe(
        name: String,
        gas_cost: i64,
        parameters: Vec<ContractParameterType>,
        return_type: ContractParameterType,
    ) -> Self {
        Self::new(name, gas_cost, true, 0, parameters, return_type)
    }

    /// Creates a new unsafe (state-changing) method.
    pub fn unsafe_method(
        name: String,
        gas_cost: i64,
        required_call_flags: u8,
        parameters: Vec<ContractParameterType>,
        return_type: ContractParameterType,
    ) -> Self {
        Self::new(
            name,
            gas_cost,
            false,
            required_call_flags,
            parameters,
            return_type,
        )
    }
}

impl HardforkActivable for NativeMethod {
    fn active_in(&self) -> Option<Hardfork> {
        self.active_in
    }

    fn deprecated_in(&self) -> Option<Hardfork> {
        self.deprecated_in
    }
}

/// Represents an event declared by a native contract.
///
/// Mirrors C# `ContractEventAttribute`: the contract manifest's event list is
/// the hardfork-filtered projection of these declarations, ordered by
/// [`NativeEvent::order`] (the attribute's `order` argument). Hardfork gating
/// uses the same `IsActive` semantics as methods, which is how dual
/// registrations under one name work (e.g. RoleManagement's `Designation`
/// V0/V1 across `HF_Echidna`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeEvent {
    /// Declaration order (the C# `ContractEventAttribute.Order` value).
    pub order: i32,

    /// The manifest descriptor (event name + named, typed parameters).
    pub descriptor: ContractEventDescriptor,

    /// Hardfork that activates this event (C# `ContractEventAttribute.ActiveIn`).
    pub active_in: Option<Hardfork>,

    /// Hardfork that deprecates this event (C# `ContractEventAttribute.DeprecatedIn`).
    pub deprecated_in: Option<Hardfork>,
}

impl NativeEvent {
    /// Creates a new ungated event declaration from `(name, type)` parameter
    /// pairs.
    ///
    /// Panics only on inputs that are statically invalid as manifest data
    /// (empty event name or duplicate parameter names), which for the fixed
    /// native event tables is a compile-time-style invariant.
    pub fn new(order: i32, name: &str, parameters: &[(&str, ContractParameterType)]) -> Self {
        let parameters = parameters
            .iter()
            .map(|(parameter_name, parameter_type)| {
                ContractParameterDefinition::new((*parameter_name).to_string(), *parameter_type)
                    .expect("native event parameter definition")
            })
            .collect();
        Self {
            order,
            descriptor: ContractEventDescriptor::new(name.to_string(), parameters)
                .expect("native event descriptor"),
            active_in: None,
            deprecated_in: None,
        }
    }

    /// Marks the event as active starting from the given hardfork (inclusive).
    pub fn with_active_in(mut self, hardfork: Hardfork) -> Self {
        self.active_in = Some(hardfork);
        self
    }

    /// Marks the event as deprecated starting from the given hardfork (inclusive).
    pub fn with_deprecated_in(mut self, hardfork: Hardfork) -> Self {
        self.deprecated_in = Some(hardfork);
        self
    }
}

impl HardforkActivable for NativeEvent {
    fn active_in(&self) -> Option<Hardfork> {
        self.active_in
    }

    fn deprecated_in(&self) -> Option<Hardfork> {
        self.deprecated_in
    }
}

/// Checks whether a hardfork-activable item is active.
///
/// Mirrors C# v3.10.0 `NativeContract.IsActive(...)` (PR #4520/#4524): a
/// method/event is active **iff** its `ActiveIn` hardfork is active (or unset)
/// **and** its `DeprecatedIn` hardfork is NOT yet active (or unset). The earlier
/// OR-form wrongly activated a descriptor whose `DeprecatedIn` had already
/// passed (when `ActiveIn` also passed) and one whose `ActiveIn`+`DeprecatedIn`
/// were both still in the future.
pub fn is_active_for<T: HardforkActivable>(
    item: &T,
    hf_checker: impl Fn(Hardfork, u32) -> bool,
    block_height: u32,
) -> bool {
    let active_in_ok = item
        .active_in()
        .is_none_or(|hf| hf_checker(hf, block_height));
    let not_deprecated = item
        .deprecated_in()
        .is_none_or(|hf| !hf_checker(hf, block_height));
    active_in_ok && not_deprecated
}

/// Builds a [`ContractState`] for a native contract at the given
/// block height.
///
/// Mirrors the C# logic that produces the on-disk contract state for
/// each native contract, including the NEF bytecode that the
/// application engine loads when dispatching `System.Contract.CallNative`.
pub fn build_native_contract_state<T: NativeContract + ?Sized>(
    contract: &T,
    settings: &ProtocolSettings,
    block_height: u32,
) -> ContractState {
    let syscall_hash = neo_vm_rs::interop_hash("System.Contract.CallNative");

    let mut methods: Vec<&NativeMethod> = contract
        .methods()
        .iter()
        .filter(|method| method.is_active(settings, block_height))
        .collect();
    methods.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then(a.parameters.len().cmp(&b.parameters.len()))
    });

    let mut builder = ScriptBuilder::new();
    let mut abi_methods = Vec::with_capacity(methods.len());

    for method in methods {
        let offset = builder.len() as i32;
        builder.emit_push_int(0);
        builder.emit_syscall_hash(syscall_hash);
        builder.emit_opcode(OpCode::RET);

        let has_names = method.parameter_names.len() == method.parameters.len();
        let parameters = method
            .parameters
            .iter()
            .enumerate()
            .map(|(index, param_type)| {
                let name = if has_names {
                    method.parameter_names[index].clone()
                } else {
                    format!("arg{index}")
                };
                ContractParameterDefinition::new(name, *param_type)
                    .expect("native parameter definition")
            })
            .collect();

        let descriptor = ContractMethodDescriptor::new(
            method.name.clone(),
            parameters,
            method.return_type,
            offset,
            method.safe,
        )
        .expect("native method descriptor");
        abi_methods.push(descriptor);
    }

    let nef = NefFile::new("neo-core-v3.0".to_string(), builder.to_array());

    let mut manifest = ContractManifest::new_native(contract.name().to_string());
    manifest.supported_standards = contract.supported_standards(settings, block_height);
    let events: Vec<ContractEventDescriptor> = contract.events(settings, block_height);
    manifest.abi = ContractAbi::new(abi_methods, events);

    ContractState::new(contract.id(), contract.hash(), nef, manifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_error::CoreError;
    use std::collections::HashMap;

    /// Pins the C# v3.10.0 `NativeContract.IsActive` AND-form (PR #4520/#4524):
    /// a descriptor is active iff `ActiveIn` is active (or unset) AND
    /// `DeprecatedIn` is NOT active (or unset). The two both-set cases the old
    /// OR-form wrongly activated must now be INACTIVE.
    #[test]
    fn is_active_for_matches_v3100_and_form() {
        fn method(active: Option<Hardfork>, deprecated: Option<Hardfork>) -> NativeMethod {
            let mut m = NativeMethod::new(
                "m".to_string(),
                0,
                true,
                0,
                vec![],
                ContractParameterType::Void,
            );
            if let Some(a) = active {
                m = m.with_active_in(a);
            }
            if let Some(d) = deprecated {
                m = m.with_deprecated_in(d);
            }
            m
        }
        // A hardfork is "active" iff it is in `passed`.
        fn checker(passed: Vec<Hardfork>) -> impl Fn(Hardfork, u32) -> bool {
            move |hf, _h| passed.contains(&hf)
        }
        let (c, g) = (Hardfork::HfCockatrice, Hardfork::HfGorgon);

        // Neither set -> always active.
        assert!(is_active_for(&method(None, None), checker(vec![]), 0));
        // Active-only.
        assert!(!is_active_for(&method(Some(g), None), checker(vec![]), 0));
        assert!(is_active_for(&method(Some(g), None), checker(vec![g]), 0));
        // Deprecated-only.
        assert!(is_active_for(&method(None, Some(g)), checker(vec![]), 0));
        assert!(!is_active_for(&method(None, Some(g)), checker(vec![g]), 0));
        // Both set: active(c) -> deprecated(g).
        assert!(!is_active_for(
            &method(Some(c), Some(g)),
            checker(vec![]),
            0
        )); // divergent: neither passed
        assert!(is_active_for(
            &method(Some(c), Some(g)),
            checker(vec![c]),
            0
        )); // active window
        assert!(!is_active_for(
            &method(Some(c), Some(g)),
            checker(vec![c, g]),
            0
        )); // divergent: both passed
        assert!(!is_active_for(
            &method(Some(c), Some(g)),
            checker(vec![g]),
            0
        )); // deprecated, not yet active
    }

    /// A minimal native contract exercising the event/parameter-name plumbing:
    /// one ungated event at order 1, a dual registration at order 0 across
    /// `HfEchidna` (V0 deprecated / V1 active), and one method with explicit
    /// parameter names plus one without (the `arg{N}` fallback).
    struct MockNative {
        methods: Vec<NativeMethod>,
        events: Vec<NativeEvent>,
    }

    impl MockNative {
        fn new() -> Self {
            Self {
                methods: vec![
                    NativeMethod::new(
                        "named".to_string(),
                        0,
                        true,
                        0,
                        vec![
                            ContractParameterType::Hash160,
                            ContractParameterType::Integer,
                        ],
                        ContractParameterType::Void,
                    )
                    .with_parameter_names(["account", "value"]),
                    NativeMethod::new(
                        "unnamed".to_string(),
                        0,
                        true,
                        0,
                        vec![ContractParameterType::String],
                        ContractParameterType::Void,
                    ),
                ],
                events: vec![
                    // Declared out of order on purpose: `events()` must sort by
                    // the order index, not the declaration index.
                    NativeEvent::new(1, "Ungated", &[("value", ContractParameterType::Integer)]),
                    NativeEvent::new(0, "Dual", &[("a", ContractParameterType::Integer)])
                        .with_deprecated_in(Hardfork::HfEchidna),
                    NativeEvent::new(
                        0,
                        "Dual",
                        &[
                            ("a", ContractParameterType::Integer),
                            ("b", ContractParameterType::Array),
                        ],
                    )
                    .with_active_in(Hardfork::HfEchidna),
                ],
            }
        }
    }

    impl NativeContract for MockNative {
        fn id(&self) -> i32 {
            -100
        }

        fn hash(&self) -> UInt160 {
            UInt160::zero()
        }

        fn name(&self) -> &str {
            "MockNative"
        }

        fn methods(&self) -> &[NativeMethod] {
            &self.methods
        }

        fn event_descriptors(&self) -> &[NativeEvent] {
            &self.events
        }

        fn invoke(
            &self,
            _engine: &mut ApplicationEngine,
            _method: &str,
            _args: &[Vec<u8>],
        ) -> CoreResult<Vec<u8>> {
            Err(CoreError::invalid_operation("MockNative is metadata-only"))
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    fn settings_with_echidna_at(height: u32) -> ProtocolSettings {
        let mut hardforks = HashMap::new();
        hardforks.insert(Hardfork::HfEchidna, height);
        ProtocolSettings {
            hardforks,
            ..ProtocolSettings::mainnet()
        }
    }

    #[test]
    fn events_filter_by_hardfork_and_sort_by_order() {
        let contract = MockNative::new();
        let settings = settings_with_echidna_at(100);

        // Pre-Echidna: the deprecated V0 `Dual` is active; ordering puts
        // order 0 before order 1 even though `Ungated` was declared first.
        let pre = contract.events(&settings, 0);
        assert_eq!(
            pre.iter()
                .map(|e| (e.name.as_str(), e.parameters.len()))
                .collect::<Vec<_>>(),
            vec![("Dual", 1), ("Ungated", 1)]
        );

        // Post-Echidna: V0 drops out, V1 (two parameters) replaces it.
        let post = contract.events(&settings, 100);
        assert_eq!(
            post.iter()
                .map(|e| (e.name.as_str(), e.parameters.len()))
                .collect::<Vec<_>>(),
            vec![("Dual", 2), ("Ungated", 1)]
        );

        // An unscheduled hardfork keeps the deprecated V0 active forever and
        // never activates V1 (C# IsActive semantics).
        let unscheduled = ProtocolSettings {
            hardforks: HashMap::new(),
            ..ProtocolSettings::mainnet()
        };
        let never = contract.events(&unscheduled, u32::MAX);
        assert_eq!(
            never
                .iter()
                .map(|e| (e.name.as_str(), e.parameters.len()))
                .collect::<Vec<_>>(),
            vec![("Dual", 1), ("Ungated", 1)]
        );
    }

    #[test]
    fn used_hardforks_include_event_attributes() {
        // C# `_usedHardforks` concatenates event ActiveIn/DeprecatedIn; the
        // mock's methods carry no hardforks, so Echidna can only come from the
        // events. This is what makes `is_initialize_block` refresh a manifest
        // at a boundary that only changes an event signature.
        let contract = MockNative::new();
        assert_eq!(contract.used_hardforks(), vec![Hardfork::HfEchidna]);

        let settings = settings_with_echidna_at(100);
        let (initialize, hits) = contract.is_initialize_block(&settings, 100);
        assert!(initialize);
        assert_eq!(hits, vec![Hardfork::HfEchidna]);
    }

    #[test]
    fn manifest_composes_parameter_names_with_argn_fallback() {
        let contract = MockNative::new();
        let settings = settings_with_echidna_at(100);
        let state = build_native_contract_state(&contract, &settings, 0);

        let named = state
            .manifest
            .abi
            .methods
            .iter()
            .find(|m| m.name == "named")
            .expect("named method");
        assert_eq!(
            named
                .parameters
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            vec!["account", "value"]
        );

        let unnamed = state
            .manifest
            .abi
            .methods
            .iter()
            .find(|m| m.name == "unnamed")
            .expect("unnamed method");
        assert_eq!(
            unnamed
                .parameters
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            vec!["arg0"]
        );

        // The composed manifest carries the filtered, ordered event list.
        assert_eq!(
            state
                .manifest
                .abi
                .events
                .iter()
                .map(|e| (e.name.as_str(), e.parameters.len()))
                .collect::<Vec<_>>(),
            vec![("Dual", 1), ("Ungated", 1)]
        );
    }
}
