use super::registration::default_interops;
use super::*;

impl ApplicationEngine {
    /// Creates a new application engine for the given trigger and script container.
    pub fn new(
        trigger: TriggerType,
        script_container: Option<Arc<dyn Verifiable>>,
        snapshot_cache: Arc<PersistenceDataCache>,
        persisting_block: Option<Block>,
        protocol_settings: ProtocolSettings,
        gas_limit: i64,
        diagnostic: Option<Box<dyn Diagnostic>>,
        native_services: EngineNativeServices,
    ) -> CoreResult<Self> {
        Self::new_with_shared_block(
            trigger,
            script_container,
            snapshot_cache,
            persisting_block.map(Arc::new),
            protocol_settings,
            gas_limit,
            diagnostic,
            native_services,
        )
    }
    /// Creates a new application engine, sharing the persisting block as an `Arc`.
    pub fn new_with_shared_block(
        trigger: TriggerType,
        script_container: Option<Arc<dyn Verifiable>>,
        snapshot_cache: Arc<PersistenceDataCache>,
        persisting_block: Option<Arc<Block>>,
        protocol_settings: ProtocolSettings,
        gas_limit: i64,
        diagnostic: Option<Box<dyn Diagnostic>>,
        native_services: EngineNativeServices,
    ) -> CoreResult<Self> {
        let nonce_data =
            Self::initialize_nonce_data(script_container.as_ref(), persisting_block.as_deref());
        let original_snapshot_cache = Arc::clone(&snapshot_cache);
        let engine = ExecutionEngine::new(Some(JumpTable::default()));
        let mut app = Self {
            trigger,
            script_container,
            persisting_block,
            protocol_settings,
            gas_limit,
            gas_consumed: 0,
            fee_amount: gas_limit.saturating_mul(FEE_FACTOR * OPCODE_PRICE_MULTIPLIER),
            fee_consumed: 0,
            exec_fee_factor: native_services.default_exec_fee_factor * (FEE_FACTOR as u32),
            dynamic_price_table: &super::op_code_prices_v1::DEFAULT_DYNAMIC_PRICE_TABLE,
            use_dynamic_pricing: false,
            fee_whitelist_context: 0,
            fee_whitelist_flag: false,
            whitelisted: false,
            storage_price: native_services.default_storage_price,
            call_flags: CallFlags::ALL,
            vm_engine: VmEngineHost::new(engine),
            interop_handlers: Arc::new(HashMap::new()),
            snapshot_cache,
            original_snapshot_cache,
            notifications: Vec::new(),
            logs: Vec::new(),
            native_registry: native_services.registry,
            native_state_reader: native_services.state_reader,
            witness_hash_resolver: native_services.witness_hash_resolver,
            chain_queries: None,
            policy_hash: native_services.policy_hash,
            policy_hardfork_heights: OnceLock::new(),
            native_contract_cache: Arc::new(Mutex::new(NativeContractsCache::default())),
            contracts: HashMap::new(),
            contract_query_results: HashMap::new(),
            native_fee_whitelist: HashMap::new(),
            native_fee_cache_dirty: false,
            parsed_contract_scripts: HashMap::new(),
            storage_iterators: HashMap::new(),
            next_iterator_id: 1,
            current_script_hash: None,
            calling_script_hash: None,
            native_calling_override: None,
            entry_script_hash: None,
            invocation_counter: HashMap::new(),
            pending_native_calls: Vec::new(),
            nonce_data,
            random_times: 0,
            diagnostic,
            fault_exception: None,
            unknown_hardfork_fault: None,
            states: HashMap::new(),
            event_observer: None,
            profiler: None,
            rw_set_tracker: Mutex::new(None),
            rw_set_tracking: AtomicBool::new(false),
        };
        app.select_hardfork_vm_semantics();

        // No `attach_host` here: the engine is returned by value, so a pointer
        // taken now would dangle the moment the caller receives it. The host
        // pointer is attached at the entry points that actually execute the VM.
        app.register_native_contracts();
        app.refresh_policy_settings();
        app.install_default_interops()?;
        // Ignore any fees incurred during engine initialization (native contract setup, policy reads).
        app.fee_consumed = 0;
        app.gas_consumed = 0;
        if let Some(mut diagnostic) = app.diagnostic.take() {
            diagnostic.initialized(&mut app);
            app.diagnostic = Some(diagnostic);
        }
        Ok(app)
    }
    /// Creates a new application engine with preloaded contract states and a
    /// shared native-contract cache (used by hosts/tests that seed state).
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_preloaded_native(
        trigger: TriggerType,
        script_container: Option<Arc<dyn Verifiable>>,
        snapshot_cache: Arc<PersistenceDataCache>,
        persisting_block: Option<Arc<Block>>,
        protocol_settings: ProtocolSettings,
        gas_limit: i64,
        contracts: HashMap<UInt160, ContractState>,
        native_contract_cache: Arc<Mutex<NativeContractsCache>>,
        diagnostic: Option<Box<dyn Diagnostic>>,
        native_services: EngineNativeServices,
    ) -> CoreResult<Self> {
        let contracts = contracts
            .into_iter()
            .map(|(hash, state)| (hash, Arc::new(state)))
            .collect();
        Self::new_with_preloaded_native_shared(
            trigger,
            script_container,
            snapshot_cache,
            persisting_block,
            protocol_settings,
            gas_limit,
            contracts,
            native_contract_cache,
            diagnostic,
            native_services,
        )
    }

    /// Shared-contract-state variant used by the block persistence pipeline.
    ///
    /// Cloning the map only bumps `Arc` references; the NEF scripts and
    /// manifests are not copied per transaction.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_preloaded_native_shared(
        trigger: TriggerType,
        script_container: Option<Arc<dyn Verifiable>>,
        snapshot_cache: Arc<PersistenceDataCache>,
        persisting_block: Option<Arc<Block>>,
        protocol_settings: ProtocolSettings,
        gas_limit: i64,
        contracts: HashMap<UInt160, Arc<ContractState>>,
        native_contract_cache: Arc<Mutex<NativeContractsCache>>,
        diagnostic: Option<Box<dyn Diagnostic>>,
        native_services: EngineNativeServices,
    ) -> CoreResult<Self> {
        let nonce_data =
            Self::initialize_nonce_data(script_container.as_ref(), persisting_block.as_deref());
        let original_snapshot_cache = Arc::clone(&snapshot_cache);
        let engine = ExecutionEngine::new(Some(JumpTable::default()));
        let mut app = Self {
            trigger,
            script_container,
            persisting_block,
            protocol_settings,
            gas_limit,
            gas_consumed: 0,
            fee_amount: gas_limit.saturating_mul(FEE_FACTOR * OPCODE_PRICE_MULTIPLIER),
            fee_consumed: 0,
            exec_fee_factor: native_services.default_exec_fee_factor * (FEE_FACTOR as u32),
            dynamic_price_table: &super::op_code_prices_v1::DEFAULT_DYNAMIC_PRICE_TABLE,
            use_dynamic_pricing: false,
            fee_whitelist_context: 0,
            fee_whitelist_flag: false,
            whitelisted: false,
            storage_price: native_services.default_storage_price,
            call_flags: CallFlags::ALL,
            vm_engine: VmEngineHost::new(engine),
            interop_handlers: Arc::new(HashMap::new()),
            snapshot_cache,
            original_snapshot_cache,
            notifications: Vec::new(),
            logs: Vec::new(),
            native_registry: native_services.registry,
            native_state_reader: native_services.state_reader,
            witness_hash_resolver: native_services.witness_hash_resolver,
            chain_queries: None,
            policy_hash: native_services.policy_hash,
            policy_hardfork_heights: OnceLock::new(),
            native_contract_cache,
            contracts,
            contract_query_results: HashMap::new(),
            native_fee_whitelist: HashMap::new(),
            native_fee_cache_dirty: false,
            parsed_contract_scripts: HashMap::new(),
            storage_iterators: HashMap::new(),
            next_iterator_id: 1,
            current_script_hash: None,
            calling_script_hash: None,
            native_calling_override: None,
            entry_script_hash: None,
            invocation_counter: HashMap::new(),
            pending_native_calls: Vec::new(),
            nonce_data,
            random_times: 0,
            diagnostic,
            fault_exception: None,
            unknown_hardfork_fault: None,
            states: HashMap::new(),
            event_observer: None,
            profiler: None,
            rw_set_tracker: Mutex::new(None),
            rw_set_tracking: AtomicBool::new(false),
        };
        // A05/R06: persist path must apply hardfork-dependent VM semantics
        // (e.g. Gorgon zero-shift typing), matching `new` / `new_with_shared_block`.
        app.select_hardfork_vm_semantics();

        // See the note in `new_with_shared_block`: attaching here would leave a
        // dangling pointer behind, because the engine moves when it is returned.
        app.refresh_policy_settings();
        app.install_default_interops()?;
        // Ignore any fees incurred during engine initialization.
        app.fee_consumed = 0;
        app.gas_consumed = 0;
        if let Some(mut diagnostic) = app.diagnostic.take() {
            diagnostic.initialized(&mut app);
            app.diagnostic = Some(diagnostic);
        }
        Ok(app)
    }
    /// Points the VM's interop host at this engine, so syscalls dispatch back
    /// into it.
    ///
    /// This stores a self-reference, and a raw pointer does not follow the
    /// struct when it moves -- returning an engine by value invalidates it. The
    /// pointer is therefore attached inside every VM entry point
    /// (`execute_allow_fault`, `execute_until_invocation_stack_depth`,
    /// `load_script_with_state`) and nowhere else, and the constructors leave it
    /// unset so that no stale pointer is ever reachable.
    pub(in crate::application_engine) fn attach_host(&mut self) {
        let host: &mut dyn InteropHost = self;
        let host_ptr = host as *mut dyn InteropHost;
        // SAFETY: `host_ptr` is derived from `&mut self`, so it is valid as long
        // as this engine stays at its current address, and `&mut self` keeps the
        // engine exclusively borrowed until execution returns. Every path that
        // dereferences the pointer runs inside a VM entry point, which calls this
        // method first; nothing may dereference it between a move and the next
        // attach.
        unsafe { self.vm_engine.engine_mut().set_interop_host(host_ptr) };
    }
    fn install_default_interops(&mut self) -> Result<()> {
        let shared = default_interops(
            self.is_hardfork_enabled(Hardfork::HfFaun),
            self.is_hardfork_enabled(Hardfork::HfHuyao),
        )?;
        self.vm_engine
            .engine_mut()
            .set_interop_service(shared.service.clone());
        self.interop_handlers = Arc::clone(&shared.handlers);
        Ok(())
    }
    pub(super) fn map_vm_error(context: &str, error: VmError) -> Error {
        Error::invalid_operation(format!("{context} interop failed: {error}"))
    }
    pub(crate) fn register_host_service(
        &mut self,
        name: &str,
        price: i64,
        call_flags: CallFlags,
        handler: InteropHandler,
    ) -> VmResult<()> {
        let interop_service = self
            .vm_engine
            .engine_mut()
            .interop_service_mut()
            .ok_or_else(|| VmError::invalid_operation_msg("Interop service not configured"))?;
        let hash = interop_service.register_host_descriptor(name, price, call_flags)?;
        Arc::make_mut(&mut self.interop_handlers).insert(
            hash,
            HostInteropHandler {
                price,
                huyao_price: None,
                required_call_flags: call_flags,
                handler,
            },
        );
        Ok(())
    }
    /// Registers a host service whose `HF_Huyao` fixed price (1e-11 GAS units,
    /// C# `InteropPriceAttribute.Coefficient`) replaces the base price once
    /// `IsHardforkEnabled(HF_Huyao)` holds at the execution height.
    pub(crate) fn register_host_service_huyao(
        &mut self,
        name: &str,
        price: i64,
        huyao_price: i64,
        call_flags: CallFlags,
        handler: InteropHandler,
    ) -> VmResult<()> {
        let interop_service = self
            .vm_engine
            .engine_mut()
            .interop_service_mut()
            .ok_or_else(|| VmError::invalid_operation_msg("Interop service not configured"))?;
        let hash = interop_service.register_host_descriptor(name, price, call_flags)?;
        Arc::make_mut(&mut self.interop_handlers).insert(
            hash,
            HostInteropHandler {
                price,
                huyao_price: Some(huyao_price),
                required_call_flags: call_flags,
                handler,
            },
        );
        Ok(())
    }
}
