//! Base native contract trait and types.
//!
//! This module owns the abstract [`NativeContract`] trait and the
//! [`NativeMethod`] data struct that the application engine and the
//! concrete native contracts (in `neo-native-contracts`) both depend
//! on. The trait is defined here (with the engine, the consumer) so
//! that the engine can dispatch `System.Contract.CallNative` through a
//! provider-generic native-contract registry without depending on
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
use neo_payloads::Block;
use neo_vm::script_builder::ScriptBuilder;
use neo_vm::{OpCode, StackItem};
use serde::{Deserialize, Serialize};

use crate::application_engine::ApplicationEngine;
use crate::contract_state::ContractState;
use crate::diagnostic::Diagnostic;
use crate::hardfork_activable::HardforkActivable;
use crate::native_contract_provider::{NativeContractProvider, NoNativeContractProvider};

pub use crate::native_contract_cache::{NativeContractsCache, NativeContractsCacheEntry};

/// Trait for native contract implementations.
pub trait NativeContract<P = NoNativeContractProvider>: Send + Sync + Sized
where
    P: NativeContractProvider + 'static,
{
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

    /// Returns hardforks explicitly declared by a contract-level activation schedule.
    ///
    /// This is the Rust `Hardfork` projection of C# `NativeContract.Activations`
    /// with nullable genesis entries omitted. It can include the contract's
    /// activation hardfork itself (e.g. Notary/Treasury) as well as later
    /// manifest-refresh hardforks (e.g. Oracle/Notary Faun).
    fn activations(&self) -> &'static [Hardfork] {
        &[]
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

        hardforks.extend_from_slice(self.activations());
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
        Some(build_native_contract_state_for::<P, Self>(
            self,
            settings,
            block_height,
        ))
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
    fn invoke<D, B>(
        &self,
        engine: &mut ApplicationEngine<P, D, B>,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>>
    where
        D: Diagnostic + 'static,
        B: neo_storage::CacheRead;

    /// Invokes a method after the engine has resolved its active ABI metadata.
    ///
    /// `ApplicationEngine::call_native_contract` resolves native methods by
    /// `(name, parameter count, hardfork)` before charging fees and checking
    /// call flags. Concrete native contracts can use `method_index` to jump to
    /// the binding table entry that produced `methods()[method_index]`, avoiding
    /// a second string/arity/hardfork lookup. The default keeps direct tests and
    /// mock contracts simple by falling back to the legacy name-based entry.
    fn invoke_resolved<D, B>(
        &self,
        engine: &mut ApplicationEngine<P, D, B>,
        method_index: usize,
        method: &NativeMethod,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>>
    where
        D: Diagnostic + 'static,
        B: neo_storage::CacheRead,
    {
        let _ = method_index;
        self.invoke(engine, &method.name, args)
    }

    /// Tries to invoke a resolved method without projecting VM values through
    /// the legacy byte-oriented compatibility API.
    ///
    /// The outer [`Option`] reports whether this contract implements a typed
    /// path for the resolved method. The inner result contains an optional VM
    /// value (`None` for a void method). Unsupported methods fall back to
    /// [`NativeContract::invoke_resolved`] at the ApplicationEngine boundary.
    fn try_invoke_resolved_stack_items<D, B>(
        &self,
        _engine: &mut ApplicationEngine<P, D, B>,
        _method_index: usize,
        _method: &NativeMethod,
        _args: &[StackItem],
    ) -> Option<CoreResult<Option<StackItem>>>
    where
        D: Diagnostic + 'static,
        B: neo_storage::CacheRead,
    {
        None
    }

    /// Called when the contract is initialized.
    fn initialize<D, B>(&self, _engine: &mut ApplicationEngine<P, D, B>) -> CoreResult<()>
    where
        D: Diagnostic + 'static,
        B: neo_storage::CacheRead,
    {
        Ok(())
    }

    /// Called on each block persistence.
    fn on_persist<D, B>(&self, _engine: &mut ApplicationEngine<P, D, B>) -> CoreResult<()>
    where
        D: Diagnostic + 'static,
        B: neo_storage::CacheRead,
    {
        Ok(())
    }

    /// Called after block persistence.
    fn post_persist<D, B>(&self, _engine: &mut ApplicationEngine<P, D, B>) -> CoreResult<()>
    where
        D: Diagnostic + 'static,
        B: neo_storage::CacheRead,
    {
        Ok(())
    }

    /// Returns whether this contract participates in the per-block
    /// `OnPersist` loop.
    ///
    /// The conservative default keeps third-party providers compatible: a
    /// custom contract is called unless it explicitly proves the hook is a
    /// no-op. Closed protocol providers may override this to skip known
    /// default implementations without changing hook ordering or state.
    fn has_on_persist_hook(&self) -> bool {
        true
    }

    /// Returns whether this contract participates in the per-block
    /// `PostPersist` loop.
    ///
    /// As with [`NativeContract::has_on_persist_hook`], the default is
    /// conservative and closed providers may opt known no-op contracts out.
    fn has_post_persist_hook(&self) -> bool {
        true
    }

    /// Returns whether this block can produce `OnPersist` work for the
    /// contract.
    ///
    /// The default follows the static capability. Closed providers may inspect
    /// protocol heights or transaction attributes to skip a hook only when its
    /// implementation is provably a no-op for this block.
    fn should_run_on_persist(&self, _settings: &ProtocolSettings, _block: &Block) -> bool {
        self.has_on_persist_hook()
    }

    /// Returns whether this block can produce `PostPersist` work for the
    /// contract. The conservative default follows the static capability.
    fn should_run_post_persist(&self, _settings: &ProtocolSettings, _block: &Block) -> bool {
        self.has_post_persist_hook()
    }

    /// Returns whether this contract's empty-block side effects are explicitly
    /// modeled by the blockchain service's empty-block fast-forward path.
    ///
    /// The default is intentionally conservative: adding a native contract or
    /// adding new empty-block `on_persist`/`post_persist` behavior must opt in
    /// after the batched handler is updated and store-equivalence tests cover
    /// it.
    fn supports_empty_block_fast_forward(&self) -> bool {
        false
    }

    /// Returns the contract state for a deployed contract by hash, if
    /// this contract stores contract-state records.
    ///
    /// The default implementation returns `Ok(None)`; only
    /// `ContractManagement` overrides this to query the storage.
    fn lookup_contract_state<B: neo_storage::CacheRead>(
        &self,
        _snapshot: &neo_storage::DataCache<B>,
        _hash: &neo_primitives::UInt160,
    ) -> CoreResult<Option<crate::ContractState>> {
        Ok(None)
    }

    /// Returns whether the given contract hash is currently blocked
    /// from being invoked.
    ///
    /// The default implementation returns `Ok(false)`; only
    /// `PolicyContract` overrides this.
    fn is_contract_blocked<B: neo_storage::CacheRead>(
        &self,
        _snapshot: &neo_storage::DataCache<B>,
        _contract_hash: &neo_primitives::UInt160,
    ) -> CoreResult<bool> {
        Ok(false)
    }

    /// Returns the committee multisig address (C# `NEO.GetCommitteeAddress`),
    /// used by `check_committee_witness` to authorize committee-gated operations.
    ///
    /// The default implementation returns `Ok(None)`; only `NeoToken` (which
    /// owns the committee cache) overrides this.
    fn committee_address<B: neo_storage::CacheRead>(
        &self,
        _snapshot: &neo_storage::DataCache<B>,
    ) -> CoreResult<Option<neo_primitives::UInt160>> {
        Ok(None)
    }

    /// Returns the whitelisted fee (in datoshi) for the given contract
    /// method, or `None` if no whitelist applies.
    ///
    /// The default implementation returns `Ok(None)`; only
    /// `PolicyContract` overrides this.
    fn whitelisted_fee<B: neo_storage::CacheRead>(
        &self,
        _snapshot: &neo_storage::DataCache<B>,
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
    fn oracle_request_url_full<B: neo_storage::CacheRead>(
        &self,
        _snapshot: &neo_storage::DataCache<B>,
        _id: u64,
    ) -> CoreResult<Option<OracleRequestDetails>> {
        Ok(None)
    }

    /// Returns the transaction state for a given transaction hash.
    fn transaction_state<B: neo_storage::CacheRead>(
        &self,
        _snapshot: &neo_storage::DataCache<B>,
        _tx_hash: &neo_primitives::UInt256,
    ) -> CoreResult<Option<neo_payloads::TransactionState>> {
        Ok(None)
    }

    /// Returns the trimmed block stored by hash.
    fn trimmed_block<B: neo_storage::CacheRead>(
        &self,
        _snapshot: &neo_storage::DataCache<B>,
        _block_hash: &neo_primitives::UInt256,
    ) -> CoreResult<Option<neo_payloads::TrimmedBlock>> {
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
}

/// Lightweight oracle request descriptor used by the engine.
///
/// This lives in `neo-execution` (not next to `OracleContract` in
/// `neo-native-contracts`) because it is part of the [`NativeContract`]
/// trait's `post_persist` method signature (see the `oracle_request`
/// parameter below). Moving it would require either moving the trait or
/// creating a cycle (`neo-native-contracts` already depends on
/// `neo-execution`). The actual `OracleRequest` storage record — the
/// persisted state — is co-located with the contract in
/// `neo-native-contracts/src/oracle_contract.rs`.
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
        name: impl Into<String>,
        cpu_fee: i64,
        safe: bool,
        required_call_flags: u8,
        parameters: Vec<ContractParameterType>,
        return_type: ContractParameterType,
    ) -> Self {
        Self {
            name: name.into(),
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
    pub fn new(order: i32, name: &str, parameters: &[(&str, ContractParameterType)]) -> Self {
        let parameters = parameters
            .iter()
            .map(|(parameter_name, parameter_type)| {
                native_static_parameter_definition(
                    (*parameter_name).to_string(),
                    *parameter_type,
                    name,
                )
            })
            .collect();
        Self {
            order,
            descriptor: native_static_event_descriptor(name.to_string(), parameters),
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
/// Mirrors vendored C# v3.10.1 `NativeContract.IsActive(...)`: a method/event
/// is active when its `ActiveIn` hardfork is absent or active, and its
/// `DeprecatedIn` hardfork is absent or not active.
pub fn is_active_for<T: HardforkActivable>(
    item: &T,
    hf_checker: impl Fn(Hardfork, u32) -> bool,
    block_height: u32,
) -> bool {
    item.active_in()
        .is_none_or(|hf| hf_checker(hf, block_height))
        && item
            .deprecated_in()
            .is_none_or(|hf| !hf_checker(hf, block_height))
}

/// Builds a provider-free [`ContractState`] for a native contract at the given
/// block height.
///
/// Metadata pinning, manifest generation, and tests normally use this helper.
/// Provider-specific engine paths use [`build_native_contract_state_for`].
pub fn build_native_contract_state<T>(
    contract: &T,
    settings: &ProtocolSettings,
    block_height: u32,
) -> ContractState
where
    T: NativeContract<NoNativeContractProvider>,
{
    build_native_contract_state_for::<NoNativeContractProvider, T>(contract, settings, block_height)
}

/// Builds a [`ContractState`] for a native contract at the given block height
/// under a specific native-contract provider ABI.
///
/// Mirrors the C# logic that produces the on-disk contract state for
/// each native contract, including the NEF bytecode that the
/// application engine loads when dispatching `System.Contract.CallNative`.
pub fn build_native_contract_state_for<P, T>(
    contract: &T,
    settings: &ProtocolSettings,
    block_height: u32,
) -> ContractState
where
    P: NativeContractProvider + 'static,
    T: NativeContract<P>,
{
    let syscall_hash = neo_vm::interop_hash("System.Contract.CallNative");

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
                native_static_parameter_definition(name, *param_type, &method.name)
            })
            .collect();

        let descriptor = native_static_method_descriptor(
            method.name.clone(),
            parameters,
            method.return_type,
            offset,
            method.safe,
        );
        abi_methods.push(descriptor);
    }

    let nef = NefFile::new("neo-core-v3.0".to_string(), builder.to_array());

    let mut manifest = ContractManifest::new_native(contract.name().to_string());
    manifest.supported_standards = contract.supported_standards(settings, block_height);
    let events: Vec<ContractEventDescriptor> = contract.events(settings, block_height);
    manifest.abi = ContractAbi::new(abi_methods, events);

    ContractState::new(contract.id(), contract.hash(), nef, manifest)
}

fn native_static_parameter_definition(
    name: String,
    param_type: ContractParameterType,
    owner: &str,
) -> ContractParameterDefinition {
    if name.is_empty() {
        tracing::error!(
            target: "neo",
            owner,
            "native ABI parameter name is empty"
        );
    }
    if param_type == ContractParameterType::Void {
        tracing::error!(
            target: "neo",
            owner,
            parameter = name.as_str(),
            "native ABI parameter type is Void"
        );
    }
    ContractParameterDefinition { name, param_type }
}

fn native_static_event_descriptor(
    name: String,
    parameters: Vec<ContractParameterDefinition>,
) -> ContractEventDescriptor {
    log_duplicate_native_parameters("event", &name, &parameters);
    if name.is_empty() {
        tracing::error!(target: "neo", "native event name is empty");
    }
    ContractEventDescriptor { name, parameters }
}

fn native_static_method_descriptor(
    name: String,
    parameters: Vec<ContractParameterDefinition>,
    return_type: ContractParameterType,
    offset: i32,
    safe: bool,
) -> ContractMethodDescriptor {
    log_duplicate_native_parameters("method", &name, &parameters);
    if name.is_empty() {
        tracing::error!(target: "neo", "native method name is empty");
    }
    if offset < 0 {
        tracing::error!(
            target: "neo",
            method = name.as_str(),
            offset,
            "native method offset is negative"
        );
    }
    ContractMethodDescriptor {
        name,
        parameters,
        return_type,
        offset,
        safe,
    }
}

fn log_duplicate_native_parameters(
    kind: &'static str,
    owner: &str,
    parameters: &[ContractParameterDefinition],
) {
    let mut names = std::collections::HashSet::new();
    for parameter in parameters {
        if !names.insert(parameter.name.as_str()) {
            tracing::error!(
                target: "neo",
                kind,
                owner,
                parameter = parameter.name.as_str(),
                "duplicate native ABI parameter name"
            );
        }
    }
}

#[cfg(test)]
#[path = "../tests/native/native_contract.rs"]
mod tests;
