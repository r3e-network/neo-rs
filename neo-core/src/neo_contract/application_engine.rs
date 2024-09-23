use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use std::rc::Rc;
use std::cell::RefCell;
use crate::block::Block;
use crate::contract::TriggerType;
use crate::hardfork::Hardfork;
use crate::neo_contract::contract_error::ContractError;
use crate::neo_contract::contract_parameter_type::ContractParameterType;
use crate::neo_contract::contract_state::ContractState;
use crate::neo_contract::contract_task::{ContractTask};
use crate::neo_contract::contract_task_awaiter::ContractTaskAwaiter;
use crate::neo_contract::execution_context_state::ExecutionContextState;
use crate::neo_contract::idiagnostic::IDiagnostic;
use crate::neo_contract::interop_descriptor::InteropDescriptor;
use crate::neo_contract::manifest::contract_manifest::ContractManifest;
use crate::neo_contract::manifest::contract_method_descriptor::ContractMethodDescriptor;
use crate::neo_contract::native_contract::NativeContract;
use crate::neo_contract::nef_file::NefFile;
use crate::neo_contract::notify_event_args::NotifyEventArgs;
use crate::neo_contract::storage_key::StorageKey;
use crate::network::payloads::{Header, IVerifiable, Transaction, Witness};
use crate::persistence::DataCache;
use crate::protocol_settings::ProtocolSettings;
use crate::uint160::UInt160;
use crate::uint256::UInt256;

const TEST_MODE_GAS: i64 = 20_00000000;

pub struct ApplicationEngine {
    execution_engine: NeoVm,
    trigger: TriggerType,
    fee_consumed: i64,
    fee_amount: i64,
    pub(crate) current_context: Option<Rc<RefCell<ExecContext>>>,
    invocation_counter: HashMap<UInt160, i32>,
    notifications: Vec<NotifyEventArgs>,
    state_cache: dyn DataCache,
    pub(crate) protocol_settings: Arc<ProtocolSettings>,
    snapshot: dyn DataCache,
    pub(crate) script_container: Option<Box<dyn IVerifiable<Error=ContractError>>>,
    pub(crate) persisting_block: Option<Block>,
    disposables: Vec<Box<dyn Drop>>,
    contract_tasks: HashMap<Rc<RefCell<ExecContext>>, ContractTaskAwaiter>,
    exec_fee_factor: u32,
    storage_price: u32,
    nonce_data: [u8; 16],
    diagnostic: Option<Box<dyn IDiagnostic>>,
    states: HashMap<String, Box<dyn Any>>,
    fault_exception: Option<Box<dyn std::error::Error>>,
}

impl ApplicationEngine {
    pub fn new(
        trigger: TriggerType,
        container: Option<Box<dyn IVerifiable<Error=ContractError>>>,
        snapshot: Box<dyn DataCache>,
        persisting_block: Option<Block>,
        settings: Arc<ProtocolSettings>,
        gas: i64,
        diagnostic: Option<Box<dyn IDiagnostic>>,
        jump_table: Option<JumpTable>,
    ) -> Self {
        let exec_fee_factor = if snapshot.is_none() || persisting_block.as_ref().map_or(false, |b| b.index() == 0) {
            PolicyContract::DEFAULT_EXEC_FEE_FACTOR
        } else {
            NativeContract::Policy.get_exec_fee_factor(&snapshot)
        };

        let storage_price = if snapshot.is_none() || persisting_block.as_ref().map_or(false, |b| b.index() == 0) {
            PolicyContract::DEFAULT_STORAGE_PRICE
        } else {
            NativeContract::Policy.get_storage_price(&snapshot)
        };

        let mut nonce_data = [0u8; 16];
        if let Some(tx) = container.as_ref().and_then(|c| c.as_any().downcast_ref::<Transaction>()) {
            nonce_data.copy_from_slice(&tx.hash().to_vec()[..16]);
        }
        if let Some(block) = &persisting_block {
            let nonce_bytes = block.nonce().to_le_bytes();
            for (i, byte) in nonce_bytes.iter().enumerate() {
                nonce_data[i] ^= byte;
            }
        }

        let mut engine = Self {
            execution_engine: NeoVm::new(jump_table.unwrap_or_else(|| Self::compose_default_jump_table())),
            trigger,
            fee_consumed: 0,
            fee_amount: gas,
            current_context: None,
            invocation_counter: HashMap::new(),
            notifications: Vec::new(),
            state_cache: snapshot.clone(),
            protocol_settings: settings,
            snapshot,
            script_container: container,
            persisting_block,
            disposables: Vec::new(),
            contract_tasks: HashMap::new(),
            exec_fee_factor,
            storage_price,
            nonce_data,
            diagnostic,
            states: HashMap::new(),
            fault_exception: None,
        };

        if let Some(diag) = &engine.diagnostic {
            diag.initialized(&engine);
        }

        engine
    }

    pub fn load_script(&mut self, script: Script, rvcount: i32, initial_position: i32) -> Rc<RefCell<ExecContext>> {
        let context = Rc::new(RefCell::new(ExecContext::new(script, rvcount, initial_position)));
        self.load_context(Rc::clone(&context));
        context
    }

    pub fn load_context(&mut self, context: Rc<RefCell<ExecContext>>) {
        let mut ctx = context.borrow_mut();
        let state = ctx.get_state_mut::<ExecutionContextState>();
        state.script_hash = state.script_hash.or_else(|| Some(ctx.script().to_script_hash()));
        self.invocation_counter.entry(state.script_hash.unwrap()).or_insert(1);
        drop(ctx);

        self.execution_engine.load_context(Rc::clone(&context));
        if let Some(diag) = &self.diagnostic {
            diag.context_loaded(&context.borrow());
        }
        self.current_context = Some(context);
    }

    pub fn execute(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        while let Some(instruction) = self.execution_engine.next_instruction() {
            self.pre_execute_instruction(&instruction)?;
            self.execution_engine.execute_instruction(instruction)?;
            self.post_execute_instruction(&instruction)?;
        }
        Ok(())
    }

    fn pre_execute_instruction(&mut self, instruction: &Instruction) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(diag) = &self.diagnostic {
            diag.pre_execute_instruction(instruction);
        }
        let fee = self.exec_fee_factor as i64 * OpCodePriceTable::get(instruction.opcode());
        self.add_fee(fee)?;
        Ok(())
    }

    fn post_execute_instruction(&mut self, instruction: &Instruction) -> Result<(), Box<dyn std::error::Error>> {
        self.execution_engine.post_execute_instruction(instruction)?;
        if let Some(diag) = &self.diagnostic {
            diag.post_execute_instruction(instruction);
        }
        Ok(())
    }

    pub fn add_fee(&mut self, amount: i64) -> Result<(), Box<dyn std::error::Error>> {
        self.fee_consumed = self.fee_consumed.checked_add(amount)
            .ok_or_else(|| "Fee overflow".to_string())?;
        if self.fee_consumed > self.fee_amount {
            Err("Insufficient gas".into())
        } else {
            Ok(())
        }
    }

    pub fn current_script_hash(&self) -> Option<UInt160> {
        self.current_context.as_ref().map(|ctx| ctx.borrow().get_script_hash())
    }

    pub fn calling_script_hash(&self) -> Option<UInt160> {
        if let Some(ctx) = &self.current_context {
            let state = ctx.borrow().get_state::<ExecutionContextState>();
            state.native_calling_script_hash.or_else(|| {
                state.calling_context.as_ref().map(|calling_ctx| {
                    calling_ctx.borrow().get_state::<ExecutionContextState>().script_hash.unwrap()
                })
            })
        } else {
            None
        }
    }

    pub fn entry_script_hash(&self) -> Option<UInt160> {
        self.execution_engine.entry_context().map(|ctx| ctx.borrow().get_script_hash())
    }

    fn create_dummy_block(snapshot: &DataCache, settings: &ProtocolSettings) -> Block {
        let hash = NativeContract::Ledger.current_hash(snapshot);
        let current_block = NativeContract::Ledger.get_block(snapshot, &hash);
        Block {
            header: Header {
                version: 0,
                prev_hash: hash,
                merkle_root: UInt256::zero(),
                timestamp: current_block.timestamp() + settings.milliseconds_per_block,
                index: current_block.index() + 1,
                next_consensus: current_block.next_consensus().clone(),
                witnesses: Witness {
                    invocation_script: vec![],
                    verification_script: vec![],
                },
            },
            transactions: vec![],
        }
    }

    pub fn on_syscall(&mut self, descriptor: &InteropDescriptor) -> Result<(), Box<dyn std::error::Error>> {
        self.validate_call_flags(descriptor.required_call_flags)?;
        self.add_fee(descriptor.fixed_price * self.exec_fee_factor as i64)?;

        let mut parameters = Vec::new();
        for _ in 0..descriptor.parameters.len() {
            parameters.push(self.pop()?);
        }
        parameters.reverse();

        let result = (descriptor.handler)(self, parameters)?;
        if let Some(value) = result {
            self.push(value)?;
        }

        Ok(())
    }

    pub fn validate_call_flags(&self, required_flags: CallFlags) -> Result<(), Box<dyn std::error::Error>> {
        let state = self.current_context.as_ref().unwrap().borrow().get_state::<ExecutionContextState>();
        if !state.call_flags.contains(required_flags) {
            Err(format!("Cannot call this SYSCALL with the flag {:?}.", state.call_flags).into())
        } else {
            Ok(())
        }
    }

    pub fn call_contract_internal(
        &mut self,
        contract_hash: UInt160,
        method: &str,
        flags: CallFlags,
        has_return_value: bool,
        args: Vec<StackItem>,
    ) -> Result<Rc<RefCell<ExecContext>>, Box<dyn std::error::Error>> {
        let contract = NativeContract::ContractManagement.get_contract(&self.snapshot, &contract_hash)
            .ok_or_else(|| format!("Called Contract Does Not Exist: {}", contract_hash))?;
        let md = contract.manifest.abi.get_method(method, args.len() as u32)
            .ok_or_else(|| format!("Method \"{}\" with {} parameter(s) doesn't exist in the contract {}.", method, args.len(), contract_hash))?;
        self.call_contract_internal_with_descriptor(contract, md, flags, has_return_value, args)
    }

    fn call_contract_internal_with_descriptor(
        &mut self,
        contract: ContractState,
        method: &ContractMethodDescriptor,
        mut flags: CallFlags,
        has_return_value: bool,
        args: Vec<StackItem>,
    ) -> Result<Rc<RefCell<ExecContext>>, Box<dyn std::error::Error>> {
        // Check if the contract is blocked
        if NativeContract::Policy.is_blocked(&self.snapshot, &contract.hash) {
            return Err(format!("The contract {} has been blocked.", contract.hash).into());
        }

        let current_context = self.current_context.as_ref()
            .ok_or_else(|| "No current context".to_string())?;
        let state = current_context.borrow().get_state::<ExecutionContextState>();

        // Adjust flags based on method safety
        if method.safe {
            flags &= !(CallFlags::WRITE_STATES | CallFlags::ALLOW_NOTIFY);
        } else {
            let executing_contract = if self.is_hardfork_enabled(Hardfork::HF_Domovoi) {
                state.contract.as_ref().ok_or_else(|| "No contract in current context".to_string())?
            } else {
                NativeContract::ContractManagement.get_contract(&self.snapshot, &self.current_script_hash().unwrap())?
            };

            if !executing_contract.can_call(&contract, &method.name) {
                return Err(format!("Cannot Call Method {} Of Contract {} From Contract {}",
                                   method.name, contract.hash, self.current_script_hash().unwrap()).into());
            }
        }

        // Increment invocation counter
        let counter = self.invocation_counter.entry(contract.hash).or_insert(0);
        *counter += 1;

        let calling_flags = state.call_flags;

        // Validate argument count
        if args.len() != method.parameters.len() {
            return Err(format!("Method {} Expects {} Arguments But Receives {} Arguments",
                               method.name, method.parameters.len(), args.len()).into());
        }

        // Validate return value expectation
        if has_return_value != (method.return_type != ContractParameterType::Void) {
            return Err("The return value type does not match.".into());
        }

        // Load the contract
        let context_new = self.load_contract(&contract, method, flags & calling_flags)?;

        {
            let mut ctx = context_new.borrow_mut();
            let state = ctx.get_state_mut::<ExecutionContextState>();
            state.calling_context = Some(Rc::clone(current_context));

            // Push arguments onto the new context's evaluation stack
            for arg in args.into_iter().rev() {
                ctx.evaluation_stack.push(arg);
            }
        }

        // Apply gas cost for contract call
        let call_gas = self.get_call_gas(&contract, method);
        self.add_fee(call_gas)?;

        // Set up exception handling
        let prev_exception = self.fault_exception.take();
        let result = self.execute_context(&context_new);
        self.fault_exception = prev_exception;

        if let Err(e) = result {
            // Handle execution error
            self.fault_exception = Some(e.into());
            return Err("Contract execution failed".into());
        }

        // Handle return value
        if has_return_value {
            let return_value = context_new.borrow_mut().evaluation_stack.pop()
                .ok_or_else(|| "Expected return value, but stack is empty".to_string())?;
            self.push(return_value)?;
        }

        Ok(context_new)
    }

    fn get_call_gas(&self, contract: &ContractState, method: &ContractMethodDescriptor) -> i64 {
        // Implement gas calculation logic for contract calls
        // This is a placeholder implementation
        1000 * self.exec_fee_factor as i64
    }

    fn execute_context(&mut self, context: &Rc<RefCell<ExecContext>>) -> Result<(), Box<dyn std::error::Error>> {
        self.load_context(Rc::clone(context));
        let result = self.execute();
        self.unload_context(Rc::clone(context));
        result
    }


    pub fn call_from_native_contract_async(
        &mut self,
        calling_script_hash: UInt160,
        hash: UInt160,
        method: &str,
        args: Vec<StackItem>,
    ) -> ContractTask {
        let context_new = self.call_contract_internal(hash, method, CallFlags::ALL, false, args).unwrap();
        let mut state = context_new.borrow_mut().get_state_mut::<ExecutionContextState>();
        state.native_calling_script_hash = Some(calling_script_hash);
        let task = ContractTask::new();
        self.contract_tasks.insert(Rc::clone(&context_new), task.get_awaiter());
        task
    }

    pub fn call_from_native_contract_async_with_return<T: 'static>(
        &mut self,
        calling_script_hash: UInt160,
        hash: UInt160,
        method: &str,
        args: Vec<StackItem>,
    ) -> ContractTask<T> {
        let context_new = self.call_contract_internal(hash, method, CallFlags::All, true, args).unwrap();
        let mut state = context_new.borrow_mut().get_state_mut::<ExecutionContextState>();
        state.native_calling_script_hash = Some(calling_script_hash);
        let task = ContractTask::<T>::new();
        self.contract_tasks.insert(Rc::clone(&context_new), task.get_awaiter());
        task
    }

    fn unload_context(&mut self, context: Rc<RefCell<ExecContext>>) {
        self.execution_engine.unload_context(Rc::clone(&context));
        if !Rc::ptr_eq(context.borrow().script(), self.current_context.as_ref().unwrap().borrow().script()) {
            let state = context.borrow().get_state::<ExecutionContextState>();
            if self.fault_exception.is_none() {
                state.snapshot_cache.commit();
                if let Some(current_context) = &self.current_context {
                    let mut current_state = current_context.borrow_mut().get_state_mut::<ExecutionContextState>();
                    current_state.notification_count += state.notification_count;
                    if state.is_dynamic_call {
                        let eval_stack = &mut context.borrow_mut().evaluation_stack;
                        match eval_stack.len() {
                            0 => self.push(StackItem::Null).unwrap(),
                            1 => (),
                            _ => return Err("Multiple return values are not allowed in cross-contract calls.".into()),
                        }
                    }
                }
            } else {
                if state.notification_count > 0 {
                    self.notifications.truncate(self.notifications.len() - state.notification_count as usize);
                }
            }
        }
        if let Some(diag) = &self.diagnostic {
            diag.context_unloaded(&context.borrow());
        }
        if let Some(awaiter) = self.contract_tasks.remove(&context) {
            if let Some(exception) = &self.fault_exception {
                awaiter.set_exception(exception.clone());
            } else {
                awaiter.set_result(self);
            }
        }
    }

    pub fn load_contract(
        &mut self,
        contract: &ContractState,
        method: &ContractMethodDescriptor,
        call_flags: CallFlags,
    ) -> Result<Rc<RefCell<ExecContext>>, Box<dyn std::error::Error>> {
        let context = self.load_script(
            contract.script.clone(),
            if method.return_type == ContractParameterType::Void { 0 } else { 1 },
            method.offset as i32,
        );
        {
            let mut ctx = context.borrow_mut();
            let state = ctx.get_state_mut::<ExecutionContextState>();
            state.call_flags = call_flags;
            state.script_hash = Some(contract.hash);
            state.contract = Some(ContractState {
                id: contract.id,
                update_counter: contract.update_counter,
                hash: contract.hash,
                nef: contract.nef.clone(),
                manifest: contract.manifest.clone(),
            });
        }

        // Call initialization
        if let Some(init) = contract.manifest.abi.get_method(ContractBasicMethod::Initialize, ContractBasicMethod::InitializePCount) {
            self.load_context(context.borrow().clone_with_offset(init.offset));
        }

        Ok(context)
    }

    pub fn convert(&mut self, value: &dyn Any) -> Result<StackItem, Box<dyn std::error::Error>> {
        if let Some(disposable) = value.downcast_ref::<Box<dyn Drop>>() {
            self.disposables.push(disposable.clone());
        }

        match value {
            v if v.is::<()>() => Ok(StackItem::Null),
            v if v.is::<bool>() => Ok(StackItem::Boolean(*v.downcast_ref::<bool>().unwrap())),
            v if v.is::<i8>() => Ok(StackItem::Integer(*v.downcast_ref::<i8>().unwrap() as i64)),
            v if v.is::<u8>() => Ok(StackItem::Integer(*v.downcast_ref::<u8>().unwrap() as i64)),
            v if v.is::<i16>() => Ok(StackItem::Integer(*v.downcast_ref::<i16>().unwrap() as i64)),
            v if v.is::<u16>() => Ok(StackItem::Integer(*v.downcast_ref::<u16>().unwrap() as i64)),
            v if v.is::<i32>() => Ok(StackItem::Integer(*v.downcast_ref::<i32>().unwrap() as i64)),
            v if v.is::<u32>() => Ok(StackItem::Integer(*v.downcast_ref::<u32>().unwrap() as i64)),
            v if v.is::<i64>() => Ok(StackItem::Integer(*v.downcast_ref::<i64>().unwrap())),
            v if v.is::<u64>() => Ok(StackItem::Integer(*v.downcast_ref::<u64>().unwrap() as i64)),
            v if v.is::<Vec<u8>>() => Ok(StackItem::ByteString(v.downcast_ref::<Vec<u8>>().unwrap().clone())),
            v if v.is::<String>() => Ok(StackItem::ByteString(v.downcast_ref::<String>().unwrap().as_bytes().to_vec())),
            v if v.is::<BigInt>() => Ok(StackItem::Integer(v.downcast_ref::<BigInt>().unwrap().clone())),
            v if v.is::<UInt160>() => Ok(StackItem::ByteString(v.downcast_ref::<UInt160>().unwrap().to_vec())),
            v if v.is::<UInt256>() => Ok(StackItem::ByteString(v.downcast_ref::<UInt256>().unwrap().to_vec())),
            v if v.is::<InteropInterface>() => Ok(StackItem::InteropInterface(v.downcast_ref::<InteropInterface>().unwrap().clone())),
            v if v.is::<StackItem>() => Ok(v.downcast_ref::<StackItem>().unwrap().clone()),
            _ => Err("Unsupported type for conversion".into()),
        }
    }

    pub fn convert_back<T: 'static>(&self, item: StackItem) -> Result<T, Box<dyn std::error::Error>> {
        match item {
            StackItem::Null => Ok(()),
            StackItem::Boolean(b) => Ok(b),
            StackItem::Integer(i) => {
                if std::any::TypeId::of::<T>() == std::any::TypeId::of::<i8>() {
                    Ok(i as i8)
                } else if std::any::TypeId::of::<T>() == std::any::TypeId::of::<u8>() {
                    Ok(i as u8)
                } else if std::any::TypeId::of::<T>() == std::any::TypeId::of::<i16>() {
                    Ok(i as i16)
                } else if std::any::TypeId::of::<T>() == std::any::TypeId::of::<u16>() {
                    Ok(i as u16)
                } else if std::any::TypeId::of::<T>() == std::any::TypeId::of::<i32>() {
                    Ok(i as i32)
                } else if std::any::TypeId::of::<T>() == std::any::TypeId::of::<u32>() {
                    Ok(i as u32)
                } else if std::any::TypeId::of::<T>() == std::any::TypeId::of::<i64>() {
                    Ok(i)
                } else if std::any::TypeId::of::<T>() == std::any::TypeId::of::<u64>() {
                    Ok(i as u64)
                } else if std::any::TypeId::of::<T>() == std::any::TypeId::of::<BigInt>() {
                    Ok(BigInt::from(i))
                } else {
                    Err("Unsupported integer type for conversion".into())
                }
            },
            StackItem::ByteString(bytes) => {
                if std::any::TypeId::of::<T>() == std::any::TypeId::of::<Vec<u8>>() {
                    Ok(bytes)
                } else if std::any::TypeId::of::<T>() == std::any::TypeId::of::<String>() {
                    Ok(String::from_utf8(bytes)?)
                } else if std::any::TypeId::of::<T>() == std::any::TypeId::of::<UInt160>() {
                    Ok(UInt160::from_slice(&bytes)?)
                } else if std::any::TypeId::of::<T>() == std::any::TypeId::of::<UInt256>() {
                    Ok(UInt256::from_slice(&bytes)?)
                } else {
                    Err("Unsupported byte string type for conversion".into())
                }
            },
            StackItem::InteropInterface(interface) => {
                if std::any::TypeId::of::<T>() == std::any::TypeId::of::<InteropInterface>() {
                    Ok(interface)
                } else {
                    Err("Unsupported interop interface type for conversion".into())
                }
            },
            _ => Err("Unsupported stack item type for conversion".into()),
        }
    }

    pub fn get_state<T: 'static>(&self) -> Option<&T> {
        self.states.get(&std::any::TypeId::of::<T>().to_string()).and_then(|state| state.downcast_ref::<T>())
    }

    pub fn get_state_or_create<T: 'static, F: FnOnce() -> T>(&mut self, factory: F) -> &T {
        let type_id = std::any::TypeId::of::<T>().to_string();
        self.states.entry(type_id).or_insert_with(|| Box::new(factory()) as Box<dyn Any>);
        self.states[&type_id].downcast_ref::<T>().unwrap()
    }

    pub fn set_state<T: 'static>(&mut self, state: T) {
        self.states.insert(std::any::TypeId::of::<T>().to_string(), Box::new(state) as Box<dyn Any>);
    }

    pub fn is_hardfork_enabled(&self, hardfork: Hardfork) -> bool {
        if self.persisting_block.is_none() {
            self.protocol_settings.hardforks.contains_key(&hardfork)
        } else {
            self.protocol_settings.is_hardfork_enabled(hardfork, self.persisting_block.as_ref().unwrap().index())
        }
    }

    pub fn compose_default_jump_table() -> JumpTable {
        let mut table = JumpTable::new();
        table.insert(OpCode::Syscall, ApplicationEngine::on_syscall);
        table.insert(OpCode::CallT, ApplicationEngine::on_call_t);
        table
    }

    fn on_syscall(&mut self, instruction: &Instruction) -> Result<(), Box<dyn std::error::Error>> {
        let method = instruction.token_u32();
        self.on_syscall(&SERVICES[&method])
    }

    fn on_call_t(&mut self, instruction: &Instruction) -> Result<(), Box<dyn std::error::Error>> {
        let token_id = instruction.token_u16() as usize;
        self.validate_call_flags(CallFlags::READ_STATES | CallFlags::ALLOW_CALL)?;

        let contract = self.current_context.as_ref().unwrap().borrow().get_state::<ExecutionContextState>().contract.as_ref()
            .ok_or_else(|| "No contract in current context".to_string())?;

        if token_id >= contract.nef.tokens.len() {
            return Err("Invalid token ID".into());
        }

        let token = &contract.nef.tokens[token_id];
        if token.parameters_count as usize > self.current_context.as_ref().unwrap().borrow().evaluation_stack.len() {
            return Err("Not enough parameters for token call".into());
        }

        let mut args = Vec::new();
        for _ in 0..token.parameters_count {
            args.push(self.pop()?);
        }
        args.reverse();

        self.call_contract_internal(token.hash, &token.method, token.call_flags, token.has_return_value, args)?;
        Ok(())
    }
}

pub type Handler = fn(&mut ApplicationEngine, &[StackItem]) -> Result<StackItem, Box<dyn std::error::Error>>;

#[macro_export]
macro_rules! register_syscall {
    ($name:expr, $handler:expr, $fixed_price:expr, $required_call_flags:expr) => {{
        let descriptor = InteropDescriptor {
            name: $name.to_string(),
            handler: $handler,
            fixed_price: $fixed_price,
            required_call_flags: $required_call_flags,
        };
        register_interop_descriptor(descriptor.hash(), descriptor.clone());
        descriptor
    }};
}

lazy_static! {
    static ref SERVICES: Mutex<HashMap<u32, InteropDescriptor>> = Mutex::new(HashMap::new());
}

pub fn register_interop_descriptor(hash: u32, descriptor: InteropDescriptor) {
    let mut services = SERVICES.lock().unwrap();
    services.entry(hash).or_insert(descriptor);
}

pub const INTEROP_DESCRIPTORS: &[InteropDescriptor] = &[
    SYSTEM_CRYPTO_CHECK_SIG,
    SYSTEM_CRYPTO_CHECK_MULTISIG,
];


pub const SYSTEM_CRYPTO_CHECK_SIG: InteropDescriptor = InteropDescriptor {
    name: "System.Crypto.CheckSig".to_string(),
    handler: ApplicationEngine::check_sig,
    fixed_price: ApplicationEngine::CHECK_SIG_PRICE,
    required_call_flags: CallFlags::NONE,
};

pub const SYSTEM_CRYPTO_CHECK_MULTISIG: InteropDescriptor = InteropDescriptor {
    name: "System.Crypto.CheckMultisig",
    handler: ApplicationEngine::check_multisig,
    fixed_price: 0,
    required_call_flags: CallFlags::NONE,
};


impl ApplicationEngine {

    pub fn initialize_interop_descriptors() {
        for descriptor in INTEROP_DESCRIPTORS {
            register_interop_descriptor(descriptor.hash(), descriptor.clone());
        }
    }
    pub fn register(name: &str, handler_name: &str, fixed_price: i64, required_call_flags: CallFlags) -> InteropDescriptor {
        let method = METHOD_HANDLERS
            .get(handler_name)
            .expect("Handler not found")
            .clone();

        let descriptor = InteropDescriptor {
            name: name.to_string(),
            handler: method,
            fixed_price,
            required_call_flags,
        };

        let hash = descriptor.hash();

        SERVICES.lock().unwrap().entry(hash).or_insert_with(|| descriptor.clone());

        descriptor
    }

    fn calculate_hash(name: &str) -> u32 {
        // Simple hash function for demonstration
        // In practice, use a proper hashing algorithm
        name.bytes().fold(0u32, |hash, byte| hash.wrapping_add(byte as u32))
    }

    fn register_syscall(&self, name: &str, handler: fn(&mut ApplicationEngine, Vec<StackItem>) -> Result<Option<StackItem>, Box<dyn std::error::Error>>, fixed_price: i64, required_call_flags: CallFlags) -> InteropDescriptor {
        let descriptor = InteropDescriptor {
            name: name.to_string(),
            handler,
            fixed_price,
            required_call_flags,
        };

        let hash = self.calculate_hash(name);
        SERVICES.lock().unwrap().insert(hash, descriptor.clone());

        descriptor
    }

    pub fn run(
        script: Vec<u8>,
        snapshot: DataCache,
        container: Option<Box<dyn IVerifiable>>,
        persisting_block: Option<Block>,
        settings: Arc<ProtocolSettings>,
        offset: usize,
        max_gas: i64,
        diagnostic: Option<Box<dyn IDiagnostic>>,
    ) -> Result<ApplicationEngine, Box<dyn std::error::Error>> {
        let persisting_block = persisting_block.unwrap_or_else(|| Self::create_dummy_block(&snapshot, &settings));
        let mut engine = Self::new(
            TriggerType::APPLICATION,
            container,
            snapshot,
            Some(persisting_block),
            settings,
            max_gas,
            diagnostic,
            None,
        );
        engine.load_script(Script::new(script), -1, offset as i32);
        engine.execute()?;
        Ok(engine)
    }

    pub fn invoke_syscall(&mut self, method: u32, args: Vec<StackItem>) -> Result<Option<StackItem>, Box<dyn std::error::Error>> {
        let descriptor = SERVICES.get(&method).ok_or_else(|| format!("Unknown syscall: {}", method))?;
        self.validate_call_flags(descriptor.required_call_flags)?;
        self.add_fee(descriptor.fixed_price * self.exec_fee_factor as i64)?;
        (descriptor.handler)(self, args)
    }

    pub fn pop(&mut self) -> Result<StackItem, Box<dyn std::error::Error>> {
        self.current_context
            .as_ref()
            .ok_or_else(|| "No current context".to_string())?
            .borrow_mut()
            .evaluation_stack
            .pop()
            .ok_or_else(|| "Stack underflow".into())
    }

    pub fn push(&mut self, item: StackItem) -> Result<(), Box<dyn std::error::Error>> {
        self.current_context
            .as_ref()
            .ok_or_else(|| "No current context".to_string())?
            .borrow_mut()
            .evaluation_stack
            .push(item);
        Ok(())
    }

    pub fn notify(&mut self, script_hash: UInt160, event_name: String, state: StackItem) {
        let args = NotifyEventArgs {
            script_container: Rc::new(()),
            script_hash,
            event_name,
            state,
        };
        self.notifications.push(args);
    }

    pub fn log(&self, message: String) {
        println!("[Log] {}", message);
    }

    pub fn check_witness(&self, hash: &UInt160) -> bool {
        if let Some(container) = &self.script_container {
            container.witnesses().iter().any(|w| w.verification_script_hash() == *hash)
        } else {
            false
        }
    }

    pub fn get_random(&self) -> u64 {
        let mut hasher = Blake2b::new(8);
        hasher.update(&self.nonce_data);
        let result = hasher.finalize();
        u64::from_le_bytes(result.try_into().unwrap())
    }

    pub fn create_contract(
        &mut self,
        script: Vec<u8>,
        manifest: ContractManifest,
        update: bool,
    ) -> Result<ContractState, Box<dyn std::error::Error>> {
        let contract = if update {
            let old_contract = self.snapshot.get_contract(&manifest.name)?;
            ContractState {
                id: old_contract.id,
                update_counter: old_contract.update_counter + 1,
                hash: UInt160::from_slice(&script).unwrap(),
                nef: NefFile::new(script, self.protocol_settings.network)?,
                manifest,
            }
        } else {
            let id = self.snapshot.get_and_change::<ContractManagement>()?.next_available_id()?;
            ContractState {
                id,
                update_counter: 0,
                hash: UInt160::from(&script).unwrap(),
                nef: NefFile::new(script, self.protocol_settings.network)?,
                manifest,
            }
        };

        self.snapshot.put::<ContractManagement>(contract.hash.clone(), contract.clone())?;
        Ok(contract)
    }

    pub fn destroy_contract(&mut self, hash: &UInt160) -> Result<(), Box<dyn std::error::Error>> {
        let contract = self.snapshot.get_contract(hash)?;
        self.snapshot.delete::<ContractManagement>(hash)?;
        self.snapshot.delete::<ContractStorage>(&contract.id.to_be_bytes())?;
        Ok(())
    }

    pub fn call_contract(
        &mut self,
        hash: &UInt160,
        method: &str,
        args: Vec<StackItem>,
    ) -> Result<StackItem, Box<dyn std::error::Error>> {
        let contract = self.snapshot.get_contract(hash)?;
        let md = contract.manifest.abi.get_method(method, args.len() as u32)
            .ok_or_else(|| format!("Method not found: {}", method))?;

        let context = self.call_contract_internal_with_descriptor(contract, md, CallFlags::All, true, args)?;
        self.execute()?;

        self.pop()
    }

    pub fn storage_put(
        &mut self,
        key: &[u8],
        value: &[u8],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let contract_hash = self.current_script_hash().ok_or("No current script hash")?;
        let contract = self.snapshot.get_contract(&contract_hash)?;
        let storage_key = StorageKey::new(contract.id, key);
        let storage_item = StorageItem::new(value.to_vec());
        self.snapshot.put::<ContractStorage>(storage_key, storage_item)?;
        Ok(())
    }

    pub fn storage_get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>> {
        let contract_hash = self.current_script_hash().ok_or("No current script hash")?;
        let contract = self.snapshot.get_contract(&contract_hash)?;
        let storage_key = StorageKey::new(contract.id, key);
        Ok(self.snapshot.get::<ContractStorage>(&storage_key)?.map(|item| item.value))
    }

    pub fn storage_delete(&mut self, key: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        let contract_hash = self.current_script_hash().ok_or("No current script hash")?;
        let contract = self.snapshot.get_contract(&contract_hash)?;
        let storage_key = StorageKey::new(contract.id, key);
        self.snapshot.delete::<ContractStorage>(&storage_key)?;
        Ok(())
    }
}

impl Drop for ApplicationEngine {
    fn drop(&mut self) {
        if let Some(diag) = &self.diagnostic {
            diag.disposed();
        }
        for disposable in self.disposables.drain(..) {
            drop(disposable);
        }
    }
}