use super::*;
use crate::env_flags::env_flag_enabled;
use parking_lot::Mutex;
use std::cmp::Ordering as CmpOrdering;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tracing::info;

#[derive(Default)]
struct NativeOnPersistPerfStats {
    blocks: AtomicU64,
    total_ns_by_contract: Mutex<HashMap<String, u64>>,
}

fn native_on_persist_perf_stats() -> &'static NativeOnPersistPerfStats {
    static STATS: OnceLock<NativeOnPersistPerfStats> = OnceLock::new();
    STATS.get_or_init(NativeOnPersistPerfStats::default)
}

fn native_on_persist_perf_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| env_flag_enabled("NEO_PERSIST_PROFILE", false))
}

fn duration_to_u64_ns(duration: std::time::Duration) -> u64 {
    duration.as_nanos().min(u128::from(u64::MAX)) as u64
}

fn native_fee_trace_tx<P, D, B>(app: &ApplicationEngine<P, D, B>) -> String
where
    P: crate::native_contract_provider::NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    app.get_script_container()
        .and_then(|container| container.as_transaction())
        .and_then(|transaction| transaction.try_hash().ok())
        .map(|hash| hash.to_string())
        .unwrap_or_else(|| "none".to_string())
}

fn native_fee_trace_enabled<P, D, B>(app: &ApplicationEngine<P, D, B>) -> bool
where
    P: crate::native_contract_provider::NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    if std::env::var_os("NEO_TRACE_NATIVE_FEES").is_some() {
        return true;
    }
    let Ok(raw) = std::env::var("NEO_TRACE_NATIVE_FEES_TX") else {
        return false;
    };
    let tx_hash = native_fee_trace_tx(app);
    raw.split(',').any(|entry| {
        let entry = entry.trim();
        entry == "*" || entry.eq_ignore_ascii_case("all") || entry.eq_ignore_ascii_case(&tx_hash)
    })
}

impl<P, D, B> ApplicationEngine<P, D, B>
where
    P: crate::native_contract_provider::NativeContractProvider + 'static,
    D: crate::diagnostic::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    pub(crate) fn add_runtime_fee(&mut self, fee: u64) -> CoreResult<()> {
        self.add_fee_datoshi(
            i64::try_from(fee)
                .map_err(|_| CoreError::invalid_operation("Fee does not fit into i64"))?,
        )
    }

    /// Adds datoshis to `FeeConsumed` / `GasConsumed`.
    pub(crate) fn add_fee_datoshi(&mut self, datoshi: i64) -> CoreResult<()> {
        let pico_gas = datoshi
            .checked_mul(FEE_FACTOR)
            .ok_or_else(|| CoreError::invalid_operation("Fee multiplication overflow"))?;
        self.add_fee_pico(pico_gas)
    }

    /// Adds picoGAS to `FeeConsumed` / `GasConsumed`.
    fn add_fee_pico(&mut self, pico_gas: i64) -> CoreResult<()> {
        // C# v3.10.1 validates `AddFee` arguments before applying the
        // whitelist bypass, so a negative fee must fault even inside a
        // whitelisted call context.
        if pico_gas < 0 {
            return Err(CoreError::invalid_operation(
                "Negative gas fee is not allowed".to_string(),
            ));
        }

        if self.fee_whitelist_enabled {
            if let Ok(state_arc) = self.current_execution_state() {
                if state_arc.lock().whitelisted {
                    return Ok(());
                }
            }
        }

        let total = self
            .fee_consumed
            .checked_add(pico_gas)
            .ok_or_else(|| CoreError::invalid_operation("Fee addition overflow"))?;

        self.fee_consumed = total;
        self.gas_consumed = total;

        if self.fee_consumed > self.fee_amount {
            let required = (self.fee_consumed.max(0) as u64).div_ceil(FEE_FACTOR as u64);
            let available = (self.fee_amount.max(0) as u64) / (FEE_FACTOR as u64);
            return Err(CoreError::insufficient_gas(required, available));
        }

        Ok(())
    }

    /// Adds a fee expressed in execution units.
    ///
    /// C# formula: `AddFee(ExecFeeFactor * feeUnits)` where `ExecFeeFactor` is
    /// already in picoGAS (300,000 = 30 * 10,000).  So the result is
    /// `feeUnits * 300,000 picoGAS = feeUnits * 30 datoshi`.
    pub(crate) fn add_cpu_fee(&mut self, fee_units: i64) -> CoreResult<()> {
        if fee_units < 0 {
            return Err(CoreError::invalid_operation(
                "Negative cpu fee is not allowed".to_string(),
            ));
        }
        if fee_units == 0 {
            return Ok(());
        }

        let pico_gas = fee_units
            .checked_mul(i64::from(self.exec_fee_factor))
            .ok_or_else(|| CoreError::invalid_operation("CPU fee overflow"))?;
        self.add_fee_pico(pico_gas)
    }

    /// Charges an execution fee in datoshi.
    pub fn charge_execution_fee(&mut self, fee: u64) -> CoreResult<()> {
        let fee_datoshi = i64::try_from(fee)
            .map_err(|_| CoreError::invalid_operation("Fee does not fit into i64"))?;
        let result = self.add_fee_datoshi(fee_datoshi);
        self.observe_fee_charge(fee);
        result
    }

    fn add_native_method_fee(&mut self, cpu_fee: i64, storage_fee: i64) -> CoreResult<()> {
        if cpu_fee < 0 || storage_fee < 0 {
            return Err(CoreError::invalid_operation(
                "Negative native method fee is not allowed",
            ));
        }

        // C# v3.10.1 `NativeContract.Invoke` computes:
        // (CpuFee * ExecFeePicoFactor) + (StorageFee * StoragePrice * FeeFactor)
        // and then calls `AddFee(total, applyFactor: false)` once. Keep the same
        // shape so arithmetic faults happen before `FeeConsumed` is mutated.
        let cpu_pico = cpu_fee
            .checked_mul(i64::from(self.exec_fee_factor))
            .ok_or_else(|| CoreError::invalid_operation("Native method fee overflow"))?;
        let storage_datoshi = storage_fee
            .checked_mul(i64::from(self.storage_price))
            .ok_or_else(|| CoreError::invalid_operation("Native method fee overflow"))?;
        let storage_pico = storage_datoshi
            .checked_mul(FEE_FACTOR)
            .ok_or_else(|| CoreError::invalid_operation("Native method fee overflow"))?;
        let total = cpu_pico
            .checked_add(storage_pico)
            .ok_or_else(|| CoreError::invalid_operation("Native method fee overflow"))?;

        self.add_fee_pico(total)
    }

    /// Emits a notification event.
    pub fn notify(&mut self, event_name: String, state: Vec<u8>) -> CoreResult<()> {
        if let (Some(container), Some(contract_hash)) =
            (self.script_container.as_ref(), self.current_script_hash)
        {
            let event = NotifyEventArgs::new(
                Arc::clone(container),
                contract_hash,
                event_name,
                vec![StackItem::from_byte_string(state)],
            );
            self.emit_notify_event(event);
        }
        Ok(())
    }

    /// Emits a log event.
    pub fn log(&mut self, message: String) -> CoreResult<()> {
        if let (Some(container), Some(contract_hash)) =
            (self.script_container.as_ref(), self.current_script_hash)
        {
            let log_event = LogEventArgs::new(Arc::clone(container), contract_hash, message);
            self.emit_log_event(log_event);
        }
        Ok(())
    }

    /// Emits an event.
    pub fn emit_event(&mut self, event_name: &str, args: Vec<Vec<u8>>) -> CoreResult<()> {
        // 1. Validate event name length (must not exceed HASH_SIZE bytes)
        if event_name.len() > HASH_SIZE {
            return Err(CoreError::invalid_operation("Event name too long"));
        }

        // 2. Validate arguments count (must not exceed 16 arguments)
        if args.len() > 16 {
            return Err(CoreError::invalid_operation("Too many arguments"));
        }

        // 3. Get current contract hash
        let Some(contract_hash) = self.current_script_hash else {
            return Err(CoreError::invalid_operation("No current contract"));
        };

        let Some(container) = &self.script_container else {
            return Err(CoreError::invalid_operation(
                "Cannot emit event without a script container".to_string(),
            ));
        };

        let state_items = args
            .into_iter()
            .map(StackItem::from_byte_string)
            .collect::<Vec<_>>();

        let notification = NotifyEventArgs::new(
            Arc::clone(container),
            contract_hash,
            event_name.to_string(),
            state_items.clone(),
        );
        self.emit_notify_event(notification);

        Ok(())
    }

    /// Gets the calling script hash.
    pub fn calling_script_hash(&self) -> UInt160 {
        self.calling_script_hash.unwrap_or_else(UInt160::zero)
    }

    pub(crate) fn with_resolved_native_method<R>(
        &mut self,
        contract_hash: UInt160,
        method: &str,
        argument_count: usize,
        invoke: impl FnOnce(
            &P::Contract,
            &crate::native_contract_cache::ResolvedNativeMethod,
            &mut Self,
        ) -> CoreResult<R>,
    ) -> CoreResult<R> {
        self.refresh_context_tracking()?;

        // Resolve from the engine-local registry first, then the provider
        // captured when this engine was constructed. Engine methods do not read
        // the process-global compatibility bridge.
        let native = self
            .native_contract_by_hash(&contract_hash)
            .ok_or_else(|| CoreError::not_found(contract_hash.to_string()))?;

        let block_height = self.current_block_index();
        if !native.is_active(&self.protocol_settings, block_height) {
            return Err(CoreError::invalid_operation(format!(
                "Native contract {} is not active at height {}",
                native.name(),
                block_height
            )));
        }

        let cache_arc = self.native_contract_cache();
        let resolved_method = {
            let mut cache = cache_arc.lock();
            cache.get_or_build::<P, _>(&native).get_method(
                method,
                argument_count,
                &self.protocol_settings,
                block_height,
            )?
        }
        .ok_or_else(|| {
            CoreError::invalid_operation(format!(
                "Method '{}({})' not found in native contract {} at height {}",
                method,
                argument_count,
                native.name(),
                block_height
            ))
        })?;
        let method_meta = resolved_method.method();

        let required_flags =
            CallFlags::from_bits(method_meta.required_call_flags).ok_or_else(|| {
                CoreError::invalid_operation(format!(
                    "Method '{}' in native contract {} specifies invalid call flags",
                    method,
                    native.name()
                ))
            })?;
        if !self.call_flags.contains(required_flags) {
            return Err(CoreError::invalid_operation(format!(
                "Call flags {:?} do not satisfy required permissions {:?} for {}",
                self.call_flags, required_flags, method
            )));
        }

        let mut is_whitelisted = false;
        if self
            .protocol_settings
            .is_hardfork_enabled(Hardfork::HfFaun, block_height)
            && self
                .whitelisted_fee_for_policy(&contract_hash, method, argument_count as u32)?
                .is_some()
        {
            is_whitelisted = true;
        }

        if !is_whitelisted {
            // Charge native contract fees upfront (matches C# NativeContract.Invoke).
            let fee_before = self.fee_consumed;
            self.add_native_method_fee(method_meta.cpu_fee, method_meta.storage_fee)?;
            if native_fee_trace_enabled(self) {
                eprintln!(
                    "trace native.fee: tx={} contract={} method={} component=combined cpu_fee_units={} storage_fee_units={} exec_fee_factor={} storage_price={} fee_before_pico={} fee_after_pico={}",
                    native_fee_trace_tx(self),
                    contract_hash,
                    method,
                    method_meta.cpu_fee,
                    method_meta.storage_fee,
                    self.exec_fee_factor,
                    self.storage_price,
                    fee_before,
                    self.fee_consumed,
                );
            }
        }

        let result = invoke(&native, &resolved_method, self).map_err(|err| {
            CoreError::native_contract(format!(
                "{}({}) method `{}` failed: {}",
                native.name(),
                contract_hash,
                method,
                err
            ))
        })?;

        Ok(result)
    }

    /// Calls a native contract through the byte-oriented compatibility API.
    pub fn call_native_contract(
        &mut self,
        contract_hash: UInt160,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let observed_arguments = self.execution_observations_enabled().then(|| {
            args.iter()
                .cloned()
                .map(StackItem::from_byte_string)
                .collect::<Vec<_>>()
        });
        self.with_resolved_native_method(
            contract_hash,
            method,
            args.len(),
            move |native, resolved_method, engine| {
                let access = observed_arguments
                    .as_ref()
                    .and_then(|_| engine.native_call_access(native, resolved_method, args.len()));
                let result = native.invoke_resolved(
                    engine,
                    resolved_method.method_index(),
                    resolved_method.method(),
                    args,
                );
                if let (Some(access), Some(observed_arguments)) = (access, observed_arguments) {
                    let outcome = match &result {
                        Ok(_) if access.result_count() == 0 => {
                            crate::execution_artifact::CallObservationOutcome::Returned(Vec::new())
                        }
                        Ok(value) => {
                            crate::execution_artifact::CallObservationOutcome::Returned(vec![
                                StackItem::from_byte_string(value.clone()),
                            ])
                        }
                        Err(error) => crate::execution_artifact::CallObservationOutcome::Fault {
                            message: error.to_string(),
                            exception: None,
                        },
                    };
                    engine.observe_completed_call(access, observed_arguments, outcome);
                }
                result
            },
        )
    }

    /// Runs `on_persist` for every active native contract.
    pub fn native_on_persist(&mut self) -> CoreResult<()> {
        if self.trigger != TriggerType::OnPersist {
            return Err(CoreError::invalid_operation(
                "System.Contract.NativeOnPersist is only valid during OnPersist".to_string(),
            ));
        }

        let block_height = self
            .persisting_block
            .as_ref()
            .map(|block| block.header.index())
            .unwrap_or_else(|| self.current_block_index());

        let active_contracts: Vec<P::Contract> = self
            .native_registry
            .contracts()
            .filter(|contract| contract.is_active(&self.protocol_settings, block_height))
            .collect();

        let profiling = native_on_persist_perf_enabled();
        for contract in active_contracts {
            if profiling {
                let started = Instant::now();
                contract.on_persist(self)?;
                let elapsed_ns = duration_to_u64_ns(started.elapsed());
                let mut totals = native_on_persist_perf_stats().total_ns_by_contract.lock();
                let key = contract.name().to_string();
                let entry = totals.entry(key).or_insert(0);
                *entry = entry.saturating_add(elapsed_ns);
            } else {
                contract.on_persist(self)?;
            }
        }

        if profiling {
            let stats = native_on_persist_perf_stats();
            let blocks = stats.blocks.fetch_add(1, Ordering::Relaxed) + 1;
            if blocks.is_multiple_of(1000) {
                let blocks_f = blocks as f64;
                let mut top = {
                    let totals = stats.total_ns_by_contract.lock();
                    totals
                        .iter()
                        .map(|(name, ns)| (name.clone(), (*ns as f64) / blocks_f / 1_000_000.0))
                        .collect::<Vec<_>>()
                };
                top.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(CmpOrdering::Equal));
                let summary = top
                    .into_iter()
                    .take(8)
                    .map(|(name, avg_ms)| format!("{name}={avg_ms:.3}ms"))
                    .collect::<Vec<_>>()
                    .join(", ");
                info!(
                    target: "neo",
                    blocks,
                    top = %summary,
                    "native on-persist contract profile"
                );
            }
        }

        Ok(())
    }

    /// Runs `post_persist` for every active native contract.
    pub fn native_post_persist(&mut self) -> CoreResult<()> {
        if self.trigger != TriggerType::PostPersist {
            return Err(CoreError::invalid_operation(
                "System.Contract.NativePostPersist is only valid during PostPersist".to_string(),
            ));
        }

        let block_height = self
            .persisting_block
            .as_ref()
            .map(|block| block.header.index())
            .unwrap_or_else(|| self.current_block_index());

        let active_contracts: Vec<P::Contract> = self
            .native_registry
            .contracts()
            .filter(|contract| contract.is_active(&self.protocol_settings, block_height))
            .collect();

        for contract in active_contracts {
            contract.post_persist(self)?;
        }

        Ok(())
    }

    /// Gets the script container (transaction or block).
    pub fn get_script_container(&self) -> Option<&Arc<VerifiableContainer>> {
        self.script_container.as_ref()
    }

    /// Gets the transaction sender if the container is a transaction.
    /// This matches C# ApplicationEngine.GetTransactionSender exactly.
    pub fn get_transaction_sender(&self) -> Option<UInt160> {
        // 1. Check if we have a container
        let container = self.script_container.as_ref()?;

        // 2. Try to downcast to Transaction
        if let Some(transaction) = container.as_transaction() {
            // 3. Get the first signer's script hash (matches C# logic)
            if let Some(first_signer) = transaction.signers().first() {
                return Some(first_signer.account);
            }
        }

        // 4. Not a transaction or no signers
        None
    }
}
