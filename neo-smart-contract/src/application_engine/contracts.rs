use super::*;
use neo_primitives::ContractBasicMethod;
use std::collections::VecDeque;
use std::sync::OnceLock;

const CONTRACT_SCRIPT_CACHE_CAPACITY: usize = 256;
const CONTRACT_SCRIPT_CACHE_MAX_BYTES: usize = 64 * 1024 * 1024;

/// Immutable parsed scripts shared by all engines in this process.
///
/// Keys are the exact NEF script bytes, not contract hashes or update counters:
/// an upgrade can retain the contract hash while replacing its byte code. Both
/// entry count and retained byte code are bounded because callers can deploy
/// arbitrary contracts. Instruction-table memory is additionally bounded by the
/// NEF script-size limit enforced at deployment.
#[derive(Default)]
struct ParsedContractScriptCache {
    scripts: HashMap<Arc<[u8]>, Script>,
    insertion_order: VecDeque<Arc<[u8]>>,
    cached_bytes: usize,
}

impl ParsedContractScriptCache {
    fn get_or_parse(&mut self, script_bytes: &[u8]) -> Result<Script> {
        self.get_or_parse_with_byte_budget(script_bytes, CONTRACT_SCRIPT_CACHE_MAX_BYTES)
    }

    fn get_or_parse_with_byte_budget(
        &mut self,
        script_bytes: &[u8],
        max_bytes: usize,
    ) -> Result<Script> {
        if let Some(script) = self.scripts.get(script_bytes) {
            return Ok(script.clone());
        }

        let key: Arc<[u8]> = Arc::from(script_bytes);
        let script = Script::from_shared(Arc::clone(&key))
            .map_err(|error| Error::invalid_operation(format!("Invalid script: {error}")))?;

        // An individually oversized script still executes normally. Bypass the
        // cache before eviction so it cannot flush every useful hot contract.
        if script_bytes.len() > max_bytes {
            return Ok(script);
        }

        while !self.scripts.is_empty()
            && (self.scripts.len() >= CONTRACT_SCRIPT_CACHE_CAPACITY
                || self.cached_bytes.saturating_add(script_bytes.len()) > max_bytes)
        {
            let oldest = self
                .insertion_order
                .pop_front()
                .expect("a non-empty script cache has an insertion-order entry");
            if self.scripts.remove(&oldest).is_some() {
                self.cached_bytes = self.cached_bytes.saturating_sub(oldest.len());
            }
        }

        self.cached_bytes = self.cached_bytes.saturating_add(key.len());
        self.insertion_order.push_back(Arc::clone(&key));
        self.scripts.insert(key, script.clone());
        Ok(script)
    }
}

fn cached_contract_script(script_bytes: &[u8]) -> Result<Script> {
    static CACHE: OnceLock<Mutex<ParsedContractScriptCache>> = OnceLock::new();
    CACHE
        .get_or_init(|| Mutex::new(ParsedContractScriptCache::default()))
        .lock()
        .get_or_parse(script_bytes)
}

// NOTE: The two integration tests that originally lived here
// (`call_contract_uses_execution_state_script_hash_for_caller` and
// `pending_native_callbacks_share_native_invoke_calling_context`) construct a
// concrete `GasToken` and canonical network payloads. Keeping them inside the
// engine crate would create an engine -> native-contracts reverse
// dev-dependency, which the layering forbids. They are relocated verbatim to
// `neo-native-contracts` (see that crate's `native::contracts_engine_tests`),
// which legitimately owns both the engine and the concrete natives. No test
// behaviour is dropped; only ownership moves. See the task-96 handoff.
fn trace_block_range() -> Option<(u32, u32)> {
    static RANGE: OnceLock<Option<(u32, u32)>> = OnceLock::new();
    *RANGE.get_or_init(|| {
        std::env::var("NEO_TRACE_BLOCK").ok().and_then(|raw| {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return None;
            }
            if let Some((start, end)) = trimmed.split_once('-') {
                let start = start.trim().parse::<u32>().ok()?;
                let end = end.trim().parse::<u32>().ok()?;
                Some((start, end))
            } else {
                let block = trimmed.parse::<u32>().ok()?;
                Some((block, block))
            }
        })
    })
}
fn contract_state_cache_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("NEO_RS_DISABLE_CONTRACT_STATE_CACHE").is_none())
}
impl ApplicationEngine {
    pub(crate) fn should_trace_block(block_idx: u32) -> bool {
        trace_block_range()
            .map(|(start, end)| block_idx >= start && block_idx <= end)
            .unwrap_or(false)
    }
    pub(super) fn fetch_contract(&mut self, hash: &UInt160) -> Result<Arc<ContractState>> {
        if let Some(contract) = self.contracts.get(hash) {
            return Ok(Arc::clone(contract));
        }
        if contract_state_cache_enabled()
            && !self.native_fee_cache_dirty
            && let Some(contract) = self.native_contract_cache.lock().contract_state(hash)
        {
            self.contracts.insert(*hash, Arc::clone(&contract));
            return Ok(contract);
        }
        let block_idx = self.persisting_block().map(|b| b.index()).unwrap_or(0);
        let diag = Self::should_trace_block(block_idx);
        match self
            .native_state_reader
            .contract(self.snapshot_cache.as_ref(), hash)
        {
            Ok(Some(contract)) => {
                if diag {
                    tracing::warn!(target: "neo", block_index = block_idx, %hash, id = contract.id, "TRACE: fetch_contract found in snapshot");
                }
                let contract = Arc::new(contract);
                self.cache_committed_contract(*hash, Arc::clone(&contract));
                return Ok(contract);
            }
            Ok(None) => {
                if diag {
                    tracing::warn!(target: "neo", block_index = block_idx, %hash, "TRACE: fetch_contract NOT found in snapshot (None)");
                }
            }
            Err(e) => {
                if diag {
                    tracing::warn!(target: "neo", block_index = block_idx, %hash, error = %e, "TRACE: fetch_contract snapshot error");
                }
                return Err(Error::invalid_operation(e.to_string()));
            }
        }
        if let Some(native) = self.native_registry.get(hash) {
            let block_height = self.current_block_index();
            if let Some(contract) = native.contract_state(&self.protocol_settings, block_height) {
                if diag {
                    tracing::warn!(target: "neo", block_index = block_idx, %hash, id = contract.id, "TRACE: fetch_contract found as native");
                }
                let contract = Arc::new(contract);
                self.cache_committed_contract(*hash, Arc::clone(&contract));
                return Ok(contract);
            }
        }
        if diag {
            tracing::warn!(target: "neo", block_index = block_idx, %hash, "TRACE: fetch_contract FAILED - not found anywhere");
        }
        Err(Error::not_found(format!("Contract not found: {hash:?}")))
    }
    fn is_contract_blocked(&mut self, contract_hash: &UInt160) -> Result<bool> {
        self.native_state_reader
            .is_blocked(self.snapshot_cache.as_ref(), contract_hash)
    }
    fn cache_committed_contract(&mut self, hash: UInt160, contract: Arc<ContractState>) {
        self.contracts.insert(hash, Arc::clone(&contract));
        if contract_state_cache_enabled() && !self.native_fee_cache_dirty {
            self.native_contract_cache
                .lock()
                .cache_contract_state(hash, contract);
        }
    }

    /// Refreshes the engine's per-tx contract cache after ContractManagement
    /// mutates a contract record. Without this, a queued `_deploy` invocation
    /// would re-fetch the OLD cached `ContractState` and execute the previous
    /// NEF/manifest, producing different `_initialize` static fields than C#.
    pub fn put_contract_cache(&mut self, hash: UInt160, contract: ContractState) {
        self.contracts.insert(hash, Arc::new(contract));
        self.invalidate_contract_query_results();
    }

    /// Removes a destroyed contract from the current transaction before its
    /// hooks can run, and prevents later transactions from using stale metadata.
    pub fn remove_contract_cache(&mut self, hash: &UInt160) {
        self.contracts.remove(hash);
        self.invalidate_contract_query_results();
    }

    /// Returns a cached serialized `getContract` result. The outer `Option`
    /// distinguishes a miss from a cached contract-absent result. Mutating
    /// transactions use only their private overlay so an eventual FAULT cannot
    /// publish uncommitted contract state to a later transaction.
    pub fn cached_contract_query_result(&self, hash: &UInt160) -> Option<Option<Vec<u8>>> {
        if let Some(cached) = self.contract_query_results.get(hash).cloned() {
            return Some(cached);
        }
        if self.native_fee_cache_dirty {
            None
        } else {
            self.native_contract_cache
                .lock()
                .contract_query_result(hash)
        }
    }

    /// Caches immutable serialized bytes, never VM containers, so sharing a
    /// result cannot introduce object aliasing between execution engines.
    pub fn cache_contract_query_result(&mut self, hash: UInt160, result: Option<Vec<u8>>) {
        if self.native_fee_cache_dirty {
            self.contract_query_results.insert(hash, result);
        } else {
            self.native_contract_cache
                .lock()
                .cache_contract_query_result(hash, result);
        }
    }

    /// Invalidates contract-dependent query results after any contract mutation,
    /// including before reentrant `_deploy` hooks can observe the modified state.
    pub fn invalidate_contract_query_results(&mut self) {
        self.contract_query_results.clear();
        self.native_fee_cache_dirty = true;
        self.native_fee_whitelist.clear();
        self.native_contract_cache
            .lock()
            .invalidate_contract_states();
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn load_contract_context(
        &mut self,
        contract: Arc<ContractState>,
        method: &ContractMethodDescriptor,
        flags: CallFlags,
        argument_count: usize,
        previous_context: Option<ExecutionContext>,
        previous_hash: Option<UInt160>,
        has_return_value: bool,
    ) -> Result<ExecutionContext> {
        if method.offset < 0 {
            return Err(Error::invalid_operation(
                "Method offset cannot be negative".to_string(),
            ));
        }
        let cache_matches = self
            .parsed_contract_scripts
            .get(&contract.hash)
            .and_then(|(owner, _)| owner.upgrade())
            .is_some_and(|owner| Arc::ptr_eq(&owner, &contract));
        if !cache_matches {
            let parsed = cached_contract_script(&contract.nef.script)?;
            self.parsed_contract_scripts
                .insert(contract.hash, (Arc::downgrade(&contract), parsed));
        }
        // Clone the outer Script to preserve C# reference identity and per-call
        // snapshot unload behavior; immutable bytes and instructions are shared.
        let script = self
            .parsed_contract_scripts
            .get(&contract.hash)
            .expect("parsed script cache was just populated")
            .1
            .clone();
        let rvcount = if has_return_value { 1 } else { 0 };
        let offset = method.offset as usize;
        let contract_for_state = Arc::clone(&contract);
        let prev_hash = previous_hash;
        // The context owns only the metadata it can observe during execution.
        // Keeping the ABI descriptor borrowed avoids cloning every parameter
        // name once during lookup and again while loading the context.
        let method_name = method.name.clone();
        let return_type = method.return_type;
        let parameter_types = method
            .parameters
            .iter()
            .map(|param| param.param_type)
            .collect();
        if Self::should_trace_block(self.current_block_index())
            && contract.hash.to_string() == "0xcc638d55d99fc81295daccbaf722b84f179fb9c4"
        {
            eprintln!(
                "TRACE dump contract method={} offset={} script_len={}",
                method_name,
                offset,
                contract.nef.script.len()
            );
            std::fs::write("data/contract-2655903-actual.script", &contract.nef.script)
                .map_err(|error| Error::invalid_operation(error.to_string()))?;
        }
        let context =
            self.load_parsed_script_with_state(script, rvcount, offset, move |state| {
                state.call_flags = flags;
                state.script_hash = Some(contract_for_state.hash);
                state.contract = Some(contract_for_state);
                state.method_name = Some(method_name);
                state.argument_count = argument_count;
                state.return_type = Some(return_type);
                state.parameter_types = parameter_types;
                state.native_calling_script_hash = None;
                state.is_dynamic_call = false;
                state.calling_context = previous_context;
                state.calling_script_hash = prev_hash;
            })?;
        if let Some(init) = contract.manifest.abi.get_method_ref(
            ContractBasicMethod::INITIALIZE,
            ContractBasicMethod::INITIALIZE_P_COUNT as usize,
        ) {
            if init.offset < 0 {
                return Err(Error::invalid_operation(
                    "Initialization method offset cannot be negative".to_string(),
                ));
            }
            let init_context = context.clone_with_position(init.offset as usize);
            self.vm_engine
                .engine_mut()
                .load_context(init_context)
                .map_err(|e| Error::invalid_operation(e.to_string()))?;
            self.refresh_context_tracking()?;
        }
        Ok(context)
    }
    pub(super) fn call_contract_internal(
        &mut self,
        contract: &Arc<ContractState>,
        method: &ContractMethodDescriptor,
        mut flags: CallFlags,
        has_return_value: bool,
        args: &[StackItem],
        calling_context_override: Option<ExecutionContext>,
    ) -> Result<ExecutionContext> {
        if args.len() != method.parameters.len() {
            return Err(Error::invalid_operation(format!(
                "Method '{}' expects {} arguments but received {}.",
                method.name,
                method.parameters.len(),
                args.len()
            )));
        }
        if has_return_value != (method.return_type != ContractParameterType::Void) {
            return Err(Error::invalid_operation(
                "The return value type does not match.".to_string(),
            ));
        }
        if self.is_contract_blocked(&contract.hash)? {
            return Err(Error::invalid_operation(format!(
                "The contract {} has been blocked.",
                contract.hash
            )));
        }
        let previous_context = match calling_context_override {
            Some(context) => context,
            None => self
                .vm_engine
                .engine()
                .current_context()
                .cloned()
                .ok_or_else(|| Error::invalid_operation("No current execution context"))?,
        };
        let state_arc = previous_context
            .get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
        let (calling_flags, executing_contract, previous_hash_from_state) = {
            let state = state_arc.lock();
            (state.call_flags, state.contract.clone(), state.script_hash)
        };
        let previous_hash = previous_hash_from_state
            .or_else(|| UInt160::from_bytes(&previous_context.script_hash()).ok())
            .ok_or_else(|| Error::invalid_operation("Invalid script hash in execution context"))?;
        if method.safe {
            flags.remove(CallFlags::WRITE_STATES | CallFlags::ALLOW_NOTIFY);
        } else {
            let executing_contract = if self.is_hardfork_enabled(Hardfork::HfDomovoi) {
                executing_contract
            } else {
                match executing_contract {
                    Some(contract) => Some(contract),
                    None => self.fetch_contract(&previous_hash).ok(),
                }
            };
            if let Some(executing_contract) = executing_contract
                && !executing_contract.manifest.can_call(
                    &contract.manifest,
                    &contract.hash,
                    &method.name,
                )
            {
                return Err(Error::invalid_operation(format!(
                    "Cannot call method {} of contract {} from contract {}.",
                    method.name, contract.hash, previous_hash
                )));
            }
        }
        flags &= calling_flags;
        let whitelisted_fixed_fee = if self
            .protocol_settings
            .is_hardfork_enabled(Hardfork::HfFaun, self.current_block_index())
        {
            self.native_state_reader.whitelisted_fee(
                self.snapshot_cache.as_ref(),
                &contract.hash,
                &method.name,
                method.parameters.len() as u32,
            )?
        } else {
            None
        };
        self.increment_invocation_counter(&contract.hash);
        let new_context = self.load_contract_context(
            Arc::clone(contract),
            method,
            flags,
            args.len(),
            Some(previous_context),
            Some(previous_hash),
            has_return_value,
        )?;
        if let Some(fixed_fee) = whitelisted_fixed_fee {
            self.add_fee_datoshi(fixed_fee)?;
            let state_arc = new_context
                .get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
            state_arc.lock().whitelisted = true;
            // The fixed fee above ran before this assignment and cached "not
            // whitelisted". Drop that cache so later opcodes see the flag.
            self.invalidate_fee_whitelist();
        }
        {
            let engine = self.vm_engine.engine_mut();
            let context_mut = engine
                .current_context_mut()
                .ok_or_else(|| Error::invalid_operation("No current execution context"))?;
            // The native CALLNATIVE handler pops arguments in declaration order
            // (arg[0] first).  Therefore arg[0] must be the topmost item here;
            // push in reverse order to match C# CallContractInternal's stack
            // layout and preserve the original argument order for the callee.
            for arg in args.iter().rev() {
                context_mut
                    .push(arg.clone())
                    .map_err(|err| Error::invalid_operation(err.to_string()))?;
            }
        }
        // Loading the contract (and its optional initialize frame) already
        // refreshed the cached context state. Charging a fixed fee and pushing
        // arguments do not replace the current context or its snapshot.
        Ok(new_context)
    }
    /// Dynamically invokes a contract method (C# `System.Contract.Call`):
    /// validates the method name and contract state, applies the call flags,
    /// pushes the arguments, and enters the callee's execution context.
    pub fn call_contract_dynamic(
        &mut self,
        contract_hash: &UInt160,
        method: &str,
        call_flags: CallFlags,
        args: Vec<StackItem>,
    ) -> Result<()> {
        let profiler = self.profiler.clone();
        let started = profiler.as_ref().map(|_| std::time::Instant::now());
        let argument_count = profiler.as_ref().map(|_| args.len());
        let result = self.call_contract_dynamic_inner(contract_hash, method, call_flags, args);
        if let (Some(profiler), Some(started), Some(argument_count)) =
            (profiler, started, argument_count)
        {
            profiler.record_contract_call(
                *contract_hash,
                method,
                argument_count,
                started.elapsed(),
            );
        }
        result
    }

    fn call_contract_dynamic_inner(
        &mut self,
        contract_hash: &UInt160,
        method: &str,
        call_flags: CallFlags,
        args: Vec<StackItem>,
    ) -> Result<()> {
        let block_idx = self.persisting_block().map(|b| b.index()).unwrap_or(0);
        let diag = Self::should_trace_block(block_idx);
        if method.starts_with('_') {
            return Err(Error::invalid_operation(format!(
                "Method name '{}' cannot start with underscore.",
                method
            )));
        }
        let contract = self.fetch_contract(contract_hash)?;
        if Self::should_trace_block(self.current_block_index()) && method == "updateListToMap" {
            std::fs::write("data/contract-2655903-actual.script", &contract.nef.script)
                .map_err(|error| Error::invalid_operation(error.to_string()))?;
        }
        if diag {
            let caller = self
                .get_calling_script_hash()
                .map(|h| h.to_string())
                .unwrap_or_else(|| "none".to_string());
            let current = self
                .current_script_hash()
                .map(|h| h.to_string())
                .unwrap_or_else(|| "none".to_string());
            tracing::warn!(target: "neo", block_index = block_idx, %contract_hash, method, args_len = args.len(), caller, current, "TRACE: call_contract_dynamic");
        }
        let method_descriptor = contract
            .manifest
            .abi
            .get_method_ref(method, args.len())
            .ok_or_else(|| {
                let available: Vec<String> = contract.manifest.abi.methods.iter()
                    .map(|m| format!("{}({})", m.name, m.parameters.len()))
                    .collect();
                if diag {
                    tracing::warn!(target: "neo", block_index = block_idx, %contract_hash, method, args_len = args.len(), ?available, "TRACE: method NOT FOUND in ABI");
                }
                Error::invalid_operation(format!(
                    "Method \"{method}\" with {} parameter(s) doesn't exist in the contract {contract_hash}.",
                    args.len(),
                ))
            })?;
        if diag {
            tracing::warn!(target: "neo", block_index = block_idx, %contract_hash, method, offset = method_descriptor.offset, "TRACE: method found, calling internal");
        }
        let has_return_value = method_descriptor.return_type != ContractParameterType::Void;
        let context = self.call_contract_internal(
            &contract,
            method_descriptor,
            call_flags,
            has_return_value,
            &args,
            None,
        )?;
        let state_arc =
            context.get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
        state_arc.lock().is_dynamic_call = true;
        Ok(())
    }
    /// Calls a contract from a native contract context with the calling hash set
    /// to the native contract.
    ///
    /// This mirrors C# `ApplicationEngine.CallFromNativeContractAsync` by:
    /// - Using `CallFlags::ALL`
    /// - Bypassing the underscore method-name restriction
    /// - Setting `native_calling_script_hash` on the new context
    ///
    /// The call is loaded immediately; execution continues when the VM resumes.
    pub fn call_from_native_contract_dynamic(
        &mut self,
        calling_script_hash: &UInt160,
        contract_hash: &UInt160,
        method: &str,
        args: Vec<StackItem>,
    ) -> Result<()> {
        self.call_from_native_contract_dynamic_with_context(
            calling_script_hash,
            contract_hash,
            method,
            args,
            None,
        )
    }

    fn call_from_native_contract_dynamic_with_context(
        &mut self,
        calling_script_hash: &UInt160,
        contract_hash: &UInt160,
        method: &str,
        args: Vec<StackItem>,
        calling_context_override: Option<ExecutionContext>,
    ) -> Result<()> {
        let contract = self.fetch_contract(contract_hash)?;
        let method_descriptor = contract
            .manifest
            .abi
            .get_method_ref(method, args.len())
            .ok_or_else(|| {
                Error::invalid_operation(format!(
                    "Method \"{method}\" with {} parameter(s) doesn't exist in the contract {contract_hash}.",
                    args.len(),
                ))
            })?;
        let has_return_value = method_descriptor.return_type != ContractParameterType::Void;
        let context = self.call_contract_internal(
            &contract,
            method_descriptor,
            CallFlags::ALL,
            has_return_value,
            &args,
            calling_context_override,
        )?;
        let state_arc =
            context.get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
        state_arc.lock().native_calling_script_hash = Some(*calling_script_hash);
        // Refresh cached hashes so `GetCallingScriptHash` sees the native caller.
        self.refresh_context_tracking()?;
        Ok(())
    }
    /// Queues a contract call requested by a native contract.
    ///
    /// The queued call will be loaded after the current native syscall finishes,
    /// ensuring the native return value is pushed to the correct context.
    ///
    /// Public because concrete native contracts (NEP-17 `onNEP17Payment`
    /// callbacks) live in the `neo-native-contracts` crate and must be able to
    /// queue callbacks across the crate boundary.
    pub fn queue_contract_call_from_native(
        &mut self,
        calling_script_hash: UInt160,
        contract_hash: UInt160,
        method: impl Into<String>,
        args: Vec<StackItem>,
    ) {
        self.pending_native_calls.push(PendingNativeCall {
            calling_script_hash,
            contract_hash,
            method: method.into(),
            args,
        });
    }
    /// Loads any queued native calls (FIFO order) into the VM.
    ///
    /// This is invoked by `System.Contract.CallNative` after a native method
    /// returns, and may also be called directly by tests.
    ///
    /// Every callback uses the native-invoke context for caller hashes, call flags,
    /// witness checks, and manifest permission checks. Using the previously loaded
    /// callback for any of these makes a later native-originated call look like a
    /// contract-to-contract call (MainNet block 3464675).
    pub fn process_pending_native_calls(&mut self) -> Result<()> {
        if self.pending_native_calls.is_empty() {
            return Ok(());
        }
        let native_invoke_context = self
            .current_context()
            .cloned()
            .ok_or_else(|| Error::invalid_operation("No current execution context".to_string()))?;
        let pending = std::mem::take(&mut self.pending_native_calls);
        for call in pending.into_iter().rev() {
            self.call_from_native_contract_dynamic_with_context(
                &call.calling_script_hash,
                &call.contract_hash,
                &call.method,
                call.args,
                Some(native_invoke_context.clone()),
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod parsed_script_cache_tests {
    use super::*;

    #[test]
    fn content_cache_shares_immutable_bytes_and_instructions() {
        let mut cache = ParsedContractScriptCache::default();
        let bytes = [OpCode::PUSH1.byte(), OpCode::RET.byte()];

        let first = cache.get_or_parse(&bytes).unwrap();
        let second = cache.get_or_parse(&bytes).unwrap();

        assert_eq!(first.as_bytes().as_ptr(), second.as_bytes().as_ptr());
        assert!(Arc::ptr_eq(
            &first.get_instruction(0).unwrap(),
            &second.get_instruction(0).unwrap()
        ));
    }

    #[test]
    fn content_cache_distinguishes_equal_length_scripts() {
        let mut cache = ParsedContractScriptCache::default();
        let first = cache
            .get_or_parse(&[OpCode::PUSH1.byte(), OpCode::RET.byte()])
            .unwrap();
        let second = cache
            .get_or_parse(&[OpCode::PUSH2.byte(), OpCode::RET.byte()])
            .unwrap();

        assert_ne!(first.as_bytes().as_ptr(), second.as_bytes().as_ptr());
        assert_eq!(first.get_instruction(0).unwrap().opcode(), OpCode::PUSH1);
        assert_eq!(second.get_instruction(0).unwrap().opcode(), OpCode::PUSH2);
    }

    #[test]
    fn content_cache_evicts_oldest_script_at_capacity() {
        let mut cache = ParsedContractScriptCache::default();
        let script = |value: usize| {
            let value = value as u16;
            [
                OpCode::PUSHINT16.byte(),
                value.to_le_bytes()[0],
                value.to_le_bytes()[1],
                OpCode::RET.byte(),
            ]
        };
        let oldest = script(0);

        for value in 0..=CONTRACT_SCRIPT_CACHE_CAPACITY {
            cache.get_or_parse(&script(value)).unwrap();
        }

        assert_eq!(cache.scripts.len(), CONTRACT_SCRIPT_CACHE_CAPACITY);
        assert_eq!(cache.insertion_order.len(), CONTRACT_SCRIPT_CACHE_CAPACITY);
        assert!(!cache.scripts.contains_key(oldest.as_slice()));
        assert!(
            cache
                .scripts
                .contains_key(script(CONTRACT_SCRIPT_CACHE_CAPACITY).as_slice())
        );
    }

    #[test]
    fn oversized_script_bypasses_cache_without_evicting_hot_entries() {
        let mut cache = ParsedContractScriptCache::default();
        let hot_bytes = [OpCode::PUSH1.byte(), OpCode::RET.byte()];
        let hot = cache.get_or_parse(&hot_bytes).unwrap();
        let cached_bytes = cache.cached_bytes;

        let oversized = cache
            .get_or_parse_with_byte_budget(&[OpCode::PUSH2.byte(), OpCode::RET.byte()], 1)
            .unwrap();

        assert_eq!(
            oversized.get_instruction(0).unwrap().opcode(),
            OpCode::PUSH2
        );
        assert_eq!(cache.scripts.len(), 1);
        assert_eq!(cache.cached_bytes, cached_bytes);
        let hot_again = cache.get_or_parse(&hot_bytes).unwrap();
        assert_eq!(hot.as_bytes().as_ptr(), hot_again.as_bytes().as_ptr());
    }
}
