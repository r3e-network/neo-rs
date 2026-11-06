//! ApplicationEngine.Storage - matches C# Neo.SmartContract.ApplicationEngine.Storage.cs exactly
use super::*;
use crate::iterators::IteratorInterop;
use neo_primitives::constants::{MAX_STORAGE_KEY_SIZE, MAX_STORAGE_VALUE_SIZE};
use std::sync::OnceLock;

fn trace_storage_get() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("NEO_TRACE_STORAGE_GET").is_some())
}

fn trace_storage_put() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("NEO_TRACE_STORAGE_PUT").is_some())
}
impl ApplicationEngine {
    /// Gets storage context for reading
    pub fn storage_get_context(&mut self) -> Result<StorageContext, String> {
        self.get_storage_context().map_err(|err| err.to_string())
    }
    /// Gets storage context for reading (readonly)
    pub fn storage_get_read_only_context(&mut self) -> Result<StorageContext, String> {
        self.get_read_only_storage_context()
            .map_err(|err| err.to_string())
    }
    /// Converts context to read-write
    pub fn storage_as_read_write(
        &mut self,
        context: StorageContext,
    ) -> Result<StorageContext, String> {
        if !self.has_call_flags(CallFlags::WRITE_STATES) {
            return Err("Write states not allowed".to_string());
        }
        Ok(StorageContext::read_write(context.id))
    }
    /// Gets a storage value
    pub fn storage_get(
        &mut self,
        context: StorageContext,
        key: Vec<u8>,
    ) -> Result<Option<Vec<u8>>, String> {
        if !self.has_call_flags(CallFlags::READ_STATES) {
            return Err("Read states not allowed".to_string());
        }
        // Check key size
        if key.len() > MAX_STORAGE_KEY_SIZE {
            return Err("Key too large".to_string());
        }
        Ok(self.get_storage_item(&context, &key))
    }
    /// Sets a storage value
    pub fn storage_put(
        &mut self,
        context: StorageContext,
        key: Vec<u8>,
        value: Vec<u8>,
    ) -> Result<(), String> {
        if context.is_read_only {
            return Err("Context is read-only".to_string());
        }
        if !self.has_call_flags(CallFlags::WRITE_STATES) {
            return Err("Write states not allowed".to_string());
        }
        // Check sizes
        if key.len() > MAX_STORAGE_KEY_SIZE {
            return Err("Key too large".to_string());
        }
        if value.len() > MAX_STORAGE_VALUE_SIZE {
            return Err("Value too large".to_string());
        }
        self.put_storage_item(&context, &key, &value)
            .map_err(|err| err.to_string())?;
        Ok(())
    }
    /// Deletes a storage value
    pub fn storage_delete(&mut self, context: StorageContext, key: Vec<u8>) -> Result<(), String> {
        if context.is_read_only {
            return Err("Context is read-only".to_string());
        }
        if !self.has_call_flags(CallFlags::WRITE_STATES) {
            return Err("Write states not allowed".to_string());
        }
        // Check key size
        if key.len() > MAX_STORAGE_KEY_SIZE {
            return Err("Key too large".to_string());
        }
        self.delete_storage_item(&context, &key)
            .map_err(|err| err.to_string())?;
        Ok(())
    }
    /// Finds storage values
    pub fn storage_find(
        &mut self,
        context: StorageContext,
        prefix: Vec<u8>,
        options: FindOptions,
    ) -> Result<StorageIterator, String> {
        if !self.has_call_flags(CallFlags::READ_STATES) {
            return Err("Read states not allowed".to_string());
        }
        // Check prefix size
        if prefix.len() > MAX_STORAGE_KEY_SIZE {
            return Err("Prefix too large".to_string());
        }
        self.find_storage_entries(&context, &prefix, options)
    }
    /// Gets a value from the current contract's storage context.
    pub fn storage_get_local(&mut self, key: Vec<u8>) -> Result<Option<Vec<u8>>, String> {
        let context = self.storage_get_read_only_context()?;
        self.storage_get(context, key)
    }
    /// Puts a key/value pair into the current contract's storage context.
    pub fn storage_put_local(&mut self, key: Vec<u8>, value: Vec<u8>) -> Result<(), String> {
        let context = self.storage_get_context()?;
        self.storage_put(context, key, value)
    }
    /// Deletes an entry from the current contract's storage context.
    pub fn storage_delete_local(&mut self, key: Vec<u8>) -> Result<(), String> {
        let context = self.storage_get_context()?;
        self.storage_delete(context, key)
    }
    /// Finds entries with the given prefix in the current contract's storage context.
    pub fn storage_find_local(
        &mut self,
        prefix: Vec<u8>,
        options: FindOptions,
    ) -> Result<StorageIterator, String> {
        let context = self.storage_get_read_only_context()?;
        self.storage_find(context, prefix, options)
    }
}
fn map_storage_error(service: &str, error: String) -> VmError {
    VmError::InteropService {
        service: service.to_string(),
        error,
    }
}
fn pop_storage_bytes(app: &mut ApplicationEngine, service: &str, label: &str) -> VmResult<Vec<u8>> {
    let item = app.pop().map_err(|e| map_storage_error(service, e))?;
    item.as_bytes().map_err(|error| {
        map_storage_error(
            service,
            format!(
                "Storage {label} conversion failed for {:?}: {error}",
                item.stack_item_type()
            ),
        )
    })
}
fn storage_get_context_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let context = app
        .storage_get_context()
        .map_err(|e| map_storage_error("System.Storage.GetContext", e))?;
    app.push(context.to_stack_item())
        .map_err(|e| map_storage_error("System.Storage.GetContext", e))?;
    Ok(())
}
fn storage_get_read_only_context_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let context = app
        .storage_get_read_only_context()
        .map_err(|e| map_storage_error("System.Storage.GetReadOnlyContext", e))?;
    app.push(context.to_stack_item())
        .map_err(|e| map_storage_error("System.Storage.GetReadOnlyContext", e))?;
    Ok(())
}
fn storage_as_read_only_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let item = app
        .pop()
        .map_err(|e| map_storage_error("System.Storage.AsReadOnly", e))?;
    let context = StorageContext::from_stack_item(&item)
        .map_err(|e| map_storage_error("System.Storage.AsReadOnly", e))?;
    app.push(context.as_read_only().to_stack_item())
        .map_err(|e| map_storage_error("System.Storage.AsReadOnly", e))?;
    Ok(())
}
fn storage_get_handler(app: &mut ApplicationEngine, _engine: &mut ExecutionEngine) -> VmResult<()> {
    let context_item = app
        .pop()
        .map_err(|e| map_storage_error("System.Storage.Get", e))?;
    let context = StorageContext::from_stack_item(&context_item)
        .map_err(|e| map_storage_error("System.Storage.Get", e))?;
    let key = pop_storage_bytes(app, "System.Storage.Get", "key")?;
    let trace_enabled = trace_storage_get();
    let trace_context_id = context.id;
    let trace_context_read_only = context.is_read_only;
    let trace_key_len = key.len();
    let trace_key_preview = if trace_enabled {
        Some(
            key.iter()
                .take(24)
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
        )
    } else {
        None
    };
    let value = app
        .storage_get(context, key)
        .map_err(|e| map_storage_error("System.Storage.Get", e))?;
    if trace_enabled {
        eprintln!(
            "trace storage.get: ctx_id={} readonly={} key_len={} key_prefix={} hit={} value_len={}",
            trace_context_id,
            trace_context_read_only,
            trace_key_len,
            trace_key_preview.as_deref().unwrap_or(""),
            value.is_some(),
            value.as_ref().map(|v| v.len()).unwrap_or(0)
        );
    }
    match value {
        Some(value) => app
            .push_bytes(value)
            .map_err(|e| map_storage_error("System.Storage.Get", e))?,
        None => {
            app.push_null()
                .map_err(|e| map_storage_error("System.Storage.Get", e))?;
        }
    }
    Ok(())
}
fn storage_put_handler(app: &mut ApplicationEngine, _engine: &mut ExecutionEngine) -> VmResult<()> {
    let context_item = app
        .pop()
        .map_err(|e| map_storage_error("System.Storage.Put", e))?;
    let context = StorageContext::from_stack_item(&context_item)
        .map_err(|e| map_storage_error("System.Storage.Put", e))?;
    let key = pop_storage_bytes(app, "System.Storage.Put", "key")?;
    let value = pop_storage_bytes(app, "System.Storage.Put", "value")?;
    if trace_storage_put() {
        let key_preview = key
            .iter()
            .take(16)
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        eprintln!(
            "trace storage.put: ctx_id={} readonly={} key_len={} value_len={} key_prefix={} key={} value={}",
            context.id,
            context.is_read_only,
            key.len(),
            value.len(),
            key_preview,
            hex::encode(&key),
            hex::encode(&value),
        );
    }
    app.storage_put(context, key, value)
        .map_err(|e| map_storage_error("System.Storage.Put", e))?;
    Ok(())
}
fn storage_delete_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let context_item = app
        .pop()
        .map_err(|e| map_storage_error("System.Storage.Delete", e))?;
    let context = StorageContext::from_stack_item(&context_item)
        .map_err(|e| map_storage_error("System.Storage.Delete", e))?;
    let key = pop_storage_bytes(app, "System.Storage.Delete", "key")?;
    app.storage_delete(context, key)
        .map_err(|e| map_storage_error("System.Storage.Delete", e))?;
    Ok(())
}
fn storage_get_local_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let key = app
        .pop_bytes()
        .map_err(|e| map_storage_error("System.Storage.Local.Get", e))?;
    match app
        .storage_get_local(key)
        .map_err(|e| map_storage_error("System.Storage.Local.Get", e))?
    {
        Some(value) => app
            .push_bytes(value)
            .map_err(|e| map_storage_error("System.Storage.Local.Get", e))?,
        None => app
            .push_null()
            .map_err(|e| map_storage_error("System.Storage.Local.Get", e))?,
    }
    Ok(())
}
fn storage_put_local_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    // Interop arguments are popped in declaration order: PutLocal(key, value).
    let key = pop_storage_bytes(app, "System.Storage.Local.Put", "key")?;
    let value = pop_storage_bytes(app, "System.Storage.Local.Put", "value")?;
    if trace_storage_put() {
        eprintln!(
            "trace storage.local.put: current={:?} key={} value={}",
            app.current_script_hash(),
            hex::encode(&key),
            hex::encode(&value),
        );
    }
    app.storage_put_local(key, value)
        .map_err(|e| map_storage_error("System.Storage.Local.Put", e))?;
    Ok(())
}
fn storage_delete_local_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let key = app
        .pop_bytes()
        .map_err(|e| map_storage_error("System.Storage.Local.Delete", e))?;
    app.storage_delete_local(key)
        .map_err(|e| map_storage_error("System.Storage.Local.Delete", e))?;
    Ok(())
}
fn storage_find_local_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    // FindLocal(prefix, options) follows the same declaration-order popping
    // convention as Find(context, prefix, options).
    let prefix = app
        .pop_bytes()
        .map_err(|e| map_storage_error("System.Storage.Local.Find", e))?;
    let options_bits = app
        .pop_integer()
        .map_err(|e| map_storage_error("System.Storage.Local.Find", e))?;
    let options = FindOptions::from_bits(options_bits as u8).ok_or_else(|| {
        map_storage_error(
            "System.Storage.Local.Find",
            format!("Invalid FindOptions value: {options_bits}"),
        )
    })?;
    options
        .validate()
        .map_err(|e| map_storage_error("System.Storage.Local.Find", e))?;
    let iterator = app
        .storage_find_local(prefix, options)
        .map_err(|e| map_storage_error("System.Storage.Local.Find", e))?;
    let iterator_id = app
        .store_storage_iterator(iterator)
        .map_err(|e| map_storage_error("System.Storage.Local.Find", e))?;
    app.push(StackItem::from_interface(IteratorInterop::new(iterator_id)))
        .map_err(|e| map_storage_error("System.Storage.Local.Find", e))?;
    Ok(())
}
fn storage_find_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let context_item = app
        .pop()
        .map_err(|e| map_storage_error("System.Storage.Find", e))?;
    let context = StorageContext::from_stack_item(&context_item)
        .map_err(|e| map_storage_error("System.Storage.Find", e))?;
    let prefix = app
        .pop_bytes()
        .map_err(|e| map_storage_error("System.Storage.Find", e))?;
    let options_bits = app
        .pop_integer()
        .map_err(|e| map_storage_error("System.Storage.Find", e))?;
    let options = FindOptions::from_bits(options_bits as u8).ok_or_else(|| {
        map_storage_error(
            "System.Storage.Find",
            format!("Invalid FindOptions value: {options_bits}"),
        )
    })?;
    options
        .validate()
        .map_err(|e| map_storage_error("System.Storage.Find", e))?;
    let iterator = app
        .storage_find(context, prefix.clone(), options)
        .map_err(|e| map_storage_error("System.Storage.Find", e))?;
    let iterator_id = app
        .store_storage_iterator(iterator)
        .map_err(|e| map_storage_error("System.Storage.Find", e))?;
    app.push(StackItem::from_interface(IteratorInterop::new(iterator_id)))
        .map_err(|e| map_storage_error("System.Storage.Find", e))?;
    Ok(())
}
pub(crate) fn register_storage_interops(engine: &mut ApplicationEngine) -> VmResult<()> {
    engine.register_host_service(
        "System.Storage.GetContext",
        1 << 4,
        CallFlags::READ_STATES,
        storage_get_context_handler,
    )?;
    engine.register_host_service(
        "System.Storage.GetReadOnlyContext",
        1 << 4,
        CallFlags::READ_STATES,
        storage_get_read_only_context_handler,
    )?;
    engine.register_host_service(
        "System.Storage.AsReadOnly",
        1 << 4,
        CallFlags::READ_STATES,
        storage_as_read_only_handler,
    )?;
    engine.register_host_service(
        "System.Storage.Get",
        1 << 15,
        CallFlags::READ_STATES,
        storage_get_handler,
    )?;
    engine.register_host_service(
        "System.Storage.Put",
        1 << 15,
        CallFlags::WRITE_STATES,
        storage_put_handler,
    )?;
    engine.register_host_service(
        "System.Storage.Delete",
        1 << 15,
        CallFlags::WRITE_STATES,
        storage_delete_handler,
    )?;
    engine.register_host_service(
        "System.Storage.Find",
        1 << 15,
        CallFlags::READ_STATES,
        storage_find_handler,
    )?;
    if engine.is_hardfork_enabled(Hardfork::HfFaun) {
        engine.register_host_service(
            "System.Storage.Local.Get",
            1 << 15,
            CallFlags::READ_STATES,
            storage_get_local_handler,
        )?;
        engine.register_host_service(
            "System.Storage.Local.Put",
            1 << 15,
            CallFlags::WRITE_STATES,
            storage_put_local_handler,
        )?;
        engine.register_host_service(
            "System.Storage.Local.Delete",
            1 << 15,
            CallFlags::WRITE_STATES,
            storage_delete_local_handler,
        )?;
        engine.register_host_service(
            "System.Storage.Local.Find",
            1 << 15,
            CallFlags::READ_STATES,
            storage_find_local_handler,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CoreResult;
    use neo_protocol::signer::Signer;
    use neo_vm::{ScriptBuilder, VmState};

    // Engine-owned no-op native service doubles. The engine layer must not
    // depend on concrete natives, so these tests inject minimal trait objects
    // rather than the composition crate's real reader. The two tests below
    // execute storage syscalls against a contract injected via
    // `put_contract_cache` and never trigger a native/contract read, so the
    // empty implementations are sufficient and introduce no reverse edge.
    struct NoopStateReader;
    impl NativeStateReader for NoopStateReader {
        fn contract(
            &self,
            _snapshot: &DataCache,
            _hash: &UInt160,
        ) -> CoreResult<Option<ContractState>> {
            Ok(None)
        }
        fn current_index(&self, _snapshot: &DataCache) -> CoreResult<u32> {
            Ok(0)
        }
        fn is_blocked(&self, _snapshot: &DataCache, _hash: &UInt160) -> CoreResult<bool> {
            Ok(false)
        }
        fn whitelisted_fee(
            &self,
            _snapshot: &DataCache,
            _hash: &UInt160,
            _method: &str,
            _param_count: u32,
        ) -> CoreResult<Option<i64>> {
            Ok(None)
        }
        fn oracle_original_signers(
            &self,
            _snapshot: &DataCache,
            _id: u64,
        ) -> CoreResult<Option<Vec<Signer>>> {
            Ok(None)
        }
    }
    struct NoopResolver;
    impl WitnessHashResolver for NoopResolver {
        fn resolve(
            &self,
            _verifiable: &dyn neo_primitives::Verifiable,
            _snapshot: &DataCache,
        ) -> Vec<UInt160> {
            Vec::new()
        }
    }
    fn test_services() -> EngineNativeServices {
        EngineNativeServices {
            registry: NativeRegistry::empty(),
            state_reader: Arc::new(NoopStateReader),
            witness_hash_resolver: Arc::new(NoopResolver),
            policy_hash: UInt160::zero(),
            default_exec_fee_factor: 30,
            default_storage_price: 100_000,
        }
    }

    fn contract_engine(snapshot: Arc<DataCache>, script: Vec<u8>) -> (ApplicationEngine, UInt160) {
        let hash = UInt160::from_bytes(&[0x42; 20]).expect("contract hash");
        let mut contract = ContractState::default();
        contract.id = 42;
        contract.hash = hash;
        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            None,
            snapshot,
            None,
            ProtocolSettings::default(),
            TEST_MODE_GAS,
            None,
            test_services(),
        )
        .expect("application engine");
        assert!(engine.is_hardfork_enabled(Hardfork::HfFaun));
        engine.put_contract_cache(hash, contract);
        engine
            .load_script(script, CallFlags::ALL, Some(hash))
            .expect("load contract script");
        (engine, hash)
    }

    #[test]
    fn local_put_pops_key_before_value() {
        let snapshot = Arc::new(DataCache::new(false));
        let mut script = ScriptBuilder::new();
        // C# scripts push value first, then key; PutLocal pops key, then value.
        script.emit_push(b"value");
        script.emit_push(b"key");
        script
            .emit_syscall("System.Storage.Local.Put")
            .expect("Local.Put syscall");
        script.emit_opcode(OpCode::RET);
        let (mut engine, _) = contract_engine(Arc::clone(&snapshot), script.to_array());

        assert_eq!(engine.execute_allow_fault(), VmState::HALT);
        let stored = snapshot
            .get(&StorageKey::new(42, b"key".to_vec()))
            .expect("stored value");
        assert_eq!(stored.value_bytes().as_ref(), b"value");
        assert!(
            snapshot
                .get(&StorageKey::new(42, b"value".to_vec()))
                .is_none()
        );
    }

    #[test]
    fn local_find_pops_prefix_before_options() {
        let snapshot = DataCache::new(false);
        snapshot.add(
            StorageKey::new(42, b"ab1".to_vec()),
            StorageItem::from_bytes(b"one".to_vec()),
        );
        snapshot.add(
            StorageKey::new(42, b"ab2".to_vec()),
            StorageItem::from_bytes(b"two".to_vec()),
        );
        let snapshot = Arc::new(snapshot);
        let mut script = ScriptBuilder::new();
        // FindLocal pops prefix, then options, just like Find after its context.
        script.emit_push_int(0);
        script.emit_push(b"ab");
        script
            .emit_syscall("System.Storage.Local.Find")
            .expect("Local.Find syscall");
        script.emit_opcode(OpCode::DUP);
        script
            .emit_syscall("System.Iterator.Next")
            .expect("Iterator.Next syscall");
        script.emit_opcode(OpCode::DROP);
        script
            .emit_syscall("System.Iterator.Value")
            .expect("Iterator.Value syscall");
        script.emit_opcode(OpCode::RET);
        let (mut engine, _) = contract_engine(snapshot, script.to_array());

        assert_eq!(engine.execute_allow_fault(), VmState::HALT);
        let result = engine.result_stack().peek(0).expect("iterator result");
        let entry = result.as_array().expect("iterator entry");
        assert_eq!(entry[0].as_bytes().expect("entry key"), b"ab1");
        assert_eq!(entry[1].as_bytes().expect("entry value"), b"one");
    }
}
