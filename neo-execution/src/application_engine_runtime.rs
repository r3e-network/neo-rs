//! ApplicationEngine.Runtime - matches C# Neo.SmartContract.ApplicationEngine.Runtime.cs exactly

use crate::Interoperable;
use crate::application_engine::{ApplicationEngine, MAX_EVENT_NAME, MAX_NOTIFICATION_SIZE};
use neo_config::hardfork::Hardfork;
use neo_crypto::Murmur3;
use neo_error::{CoreError, CoreResult};
use neo_manifest::CallFlags;
use neo_manifest::ContractParameterDefinition;
use neo_primitives::ContractParameterType;
use neo_primitives::LogEventArgs;
use neo_primitives::UInt160;
use neo_primitives::constants::{ADDRESS_SIZE, HASH_SIZE};
use neo_vm::{ExecutionEngine, StackItem, VmError, VmResult};
use neo_vm_rs::StackItemType;
use num_bigint::{BigInt, Sign};
use num_traits::ToPrimitive;
use std::convert::TryFrom;
use std::string::String as StdString;

impl ApplicationEngine {
    /// Gets the platform name
    pub fn runtime_platform(&mut self) -> CoreResult<()> {
        self.push_string("NEO".to_string())
    }

    /// Gets the trigger type
    pub fn runtime_get_trigger(&mut self) -> CoreResult<()> {
        self.push_integer(i64::from(self.trigger_type().bits()))
    }

    /// Gets the network magic number
    pub fn runtime_get_network(&mut self) -> CoreResult<()> {
        self.push_integer(self.protocol_settings().network as i64)
    }

    /// Gets the address version of the current network
    pub fn runtime_get_address_version(&mut self) -> CoreResult<()> {
        self.push_integer(self.protocol_settings().address_version as i64)
    }

    /// Gets the current block time
    pub fn runtime_get_time(&mut self) -> CoreResult<()> {
        let time = self.get_current_block_time()?;
        let big = BigInt::from(time);
        self.push(StackItem::from_int(big))
    }

    /// Gets the script container
    pub fn runtime_get_script_container(&mut self) -> CoreResult<()> {
        let container = self
            .get_script_container()
            .cloned()
            .ok_or_else(|| CoreError::other("No script container"))?;

        if let Some(transaction) = container
            .as_any()
            .downcast_ref::<neo_payloads::Transaction>()
        {
            let sv =
                <neo_payloads::Transaction as neo_vm::Interoperable>::to_stack_value(transaction)
                    .map_err(|e| CoreError::other(e.to_string()))?;
            self.push(StackItem::try_from(sv).map_err(|e| CoreError::other(e.to_string()))?)
        } else {
            Err(CoreError::other(
                "Script container does not implement Interoperable",
            ))
        }
    }

    /// Loads a script at runtime (matches C# RuntimeLoadScript).
    pub fn runtime_load_script(
        &mut self,
        script: Vec<u8>,
        call_flags: CallFlags,
        args: Vec<StackItem>,
    ) -> CoreResult<()> {
        if call_flags.bits() & !CallFlags::ALL.bits() != 0 {
            return Err(CoreError::other(format!(
                "Invalid call flags: {call_flags:?}"
            )));
        }

        let calling_context = self
            .current_context()
            .cloned()
            .ok_or_else(|| CoreError::other("No current execution context"))?;

        let state_call_flags = self.get_current_call_flags()?;

        let effective_flags = call_flags & state_call_flags & CallFlags::READ_ONLY;

        self.load_script_with_state(script, -1, 0, move |state| {
            state.calling_context = Some(calling_context);
            state.call_flags = effective_flags;
            state.is_dynamic_call = true;
        })?;

        for item in args.into_iter().rev() {
            self.push(item)?;
        }

        Ok(())
    }

    /// Gets the executing script hash
    pub fn runtime_get_executing_script_hash(&mut self) -> CoreResult<()> {
        if let Some(hash) = self.current_script_hash() {
            self.push_bytes(hash.to_bytes())
        } else {
            self.push_null()
        }
    }

    /// Gets the calling script hash
    pub fn runtime_get_calling_script_hash(&mut self) -> CoreResult<()> {
        let hash = self.get_calling_script_hash();
        if let Some(hash) = hash {
            self.push_bytes(hash.to_bytes())
        } else {
            self.push_null()
        }
    }

    /// Gets the entry script hash
    pub fn runtime_get_entry_script_hash(&mut self) -> CoreResult<()> {
        if let Some(hash) = self.entry_script_hash() {
            self.push_bytes(hash.to_bytes())
        } else {
            self.push_null()
        }
    }

    /// Checks witness
    pub fn runtime_check_witness(&mut self) -> CoreResult<()> {
        let hash_or_pubkey = self.pop_bytes()?;

        // Check if it's a hash (20 bytes) or public key (33 bytes)
        let result = match hash_or_pubkey.len() {
            20 => {
                let hash = UInt160::from_bytes(&hash_or_pubkey)
                    .map_err(|e| CoreError::other(e.to_string()))?;
                self.check_witness_hash(&hash)?
            }
            33 => {
                // C# decodes the ECPoint before hashing the signature redeem script.
                let hash = self.pubkey_to_hash(&hash_or_pubkey)?;
                self.check_witness_hash(&hash)?
            }
            _ => {
                return Err(CoreError::other("Invalid hashOrPubkey length"));
            }
        };

        self.push_boolean(result)
    }

    /// Gets invocation counter
    pub fn runtime_get_invocation_counter(&mut self) -> CoreResult<()> {
        let hash = self
            .current_script_hash()
            .ok_or_else(|| CoreError::other("No current script"))?;

        let counter = self.get_or_init_invocation_counter(&hash);
        self.push_integer(counter as i64)
    }

    /// Gets the next random number derived from VRF-like construction (matches C# GetRandom exactly).
    pub fn runtime_get_random(&mut self) -> CoreResult<()> {
        let network = self.protocol_settings().network;
        let aspid_enabled = self.is_hardfork_enabled(Hardfork::HfAspidochelone);

        let buffer = if aspid_enabled {
            let seed = network.wrapping_add(self.random_counter());
            self.increment_random_counter();
            Murmur3::murmur128(self.nonce_bytes(), seed)
        } else {
            let bytes = Murmur3::murmur128(self.nonce_bytes(), network);
            self.set_nonce_bytes(bytes);
            bytes
        };

        // C# v3.9.1 ApplicationEngine.Runtime.cs:315-324:
        // HF_Aspidochelone enabled → price 1<<13 (new per-call counter, more work)
        // HF_Aspidochelone disabled → price 1<<4 (legacy single-shot path)
        let price: i64 = if aspid_enabled { 1 << 13 } else { 1 << 4 };
        self.add_cpu_fee(price)?;

        let bigint = BigInt::from_bytes_le(Sign::Plus, &buffer);
        self.push(StackItem::from_int(bigint))
    }

    /// Gets the remaining GAS available for execution (matches C# GasLeft).
    pub fn runtime_gas_left(&mut self) -> CoreResult<()> {
        self.push_integer(self.gas_left())
    }

    /// Logs a message
    pub fn runtime_log(&mut self) -> CoreResult<()> {
        let message_bytes = self.pop_bytes()?;

        if message_bytes.len() > MAX_NOTIFICATION_SIZE {
            return Err(CoreError::other(format!(
                "Notification size {} exceeds maximum allowed size of {} bytes",
                message_bytes.len(),
                MAX_NOTIFICATION_SIZE
            )));
        }

        let message = StdString::from_utf8(message_bytes).map_err(|_| {
            CoreError::other("Failed to convert byte array to string: Invalid UTF-8 sequence")
        })?;

        let script_hash = self.current_script_hash().unwrap_or_else(UInt160::zero);

        let event = LogEventArgs::new(self.script_container().cloned(), script_hash, message);

        self.emit_log_event(event);
        Ok(())
    }

    /// Sends a notification
    pub fn runtime_notify(&mut self) -> CoreResult<()> {
        // Match C# interop argument binding order:
        // System.Runtime.Notify(eventName, state)
        // OnSysCall pops arguments from stack in declaration order, so event name
        // is popped first, then state.
        let event_name = self.pop_bytes()?;
        let state = self.pop_array()?;

        if event_name.len() > MAX_EVENT_NAME {
            return Err(CoreError::other(format!(
                "Event name size {} exceeds maximum allowed size of {} bytes",
                event_name.len(),
                MAX_EVENT_NAME
            )));
        }

        if !self.is_hardfork_enabled(Hardfork::HfBasilisk) {
            return self.runtime_notify_legacy(event_name, state);
        }

        self.runtime_notify_basilisk(event_name, state)
    }

    fn runtime_notify_legacy(
        &mut self,
        event_name_bytes: Vec<u8>,
        state: Vec<StackItem>,
    ) -> CoreResult<()> {
        let event_name = StdString::from_utf8(event_name_bytes)
            .map_err(|_| CoreError::other("Failed to convert event name to UTF-8 string"))?;

        {
            let state_arc = self.current_execution_state()?;
            let state_guard = state_arc.lock();
            if state_guard.contract.is_none() {
                return Err(CoreError::other(
                    "Notifications are not allowed in dynamic scripts.",
                ));
            }
        }

        self.ensure_notification_size(&state)?;

        let script_hash = self.current_script_hash().unwrap_or_else(UInt160::zero);

        self.send_notification(script_hash, event_name, state)
    }

    fn runtime_notify_basilisk(
        &mut self,
        event_name_bytes: Vec<u8>,
        state: Vec<StackItem>,
    ) -> CoreResult<()> {
        let event_name = StdString::from_utf8(event_name_bytes)
            .map_err(|_| CoreError::other("Failed to convert event name to UTF-8 string"))?;

        let script_hash = self.current_script_hash().unwrap_or_else(UInt160::zero);

        let parameters = {
            let state_arc = self.current_execution_state()?;
            let guard = state_arc.lock();
            let contract = guard.contract.clone().ok_or_else(|| {
                CoreError::other("Notifications are not allowed in dynamic scripts.")
            })?;
            let event = contract
                .manifest
                .abi
                .events
                .iter()
                .find(|descriptor| descriptor.name == event_name)
                .cloned()
                .ok_or_else(|| {
                    CoreError::other(format!("Event `{}` does not exist.", event_name))
                })?;
            event.parameters
        };

        if parameters.len() != state.len() {
            return Err(CoreError::other(
                "The number of the arguments does not match the formal parameters of the event.",
            ));
        }

        validate_event_parameters(&state, &parameters)?;
        self.ensure_notification_size(&state)?;

        self.send_notification(script_hash, event_name, state)
    }

    /// Gets notifications
    pub fn runtime_get_notifications(&mut self) -> CoreResult<()> {
        let hash = if self.peek_is_null(0)? {
            self.pop()?;
            None
        } else {
            let bytes = self.pop_bytes()?;
            if bytes.len() != 20 {
                return Err(CoreError::other("Invalid hash length"));
            }
            Some(UInt160::from_bytes(&bytes).map_err(|e| CoreError::other(e.to_string()))?)
        };

        // Get notifications for the specified contract (or all if None)
        let notifications = self.get_notifications(hash)?;

        self.push_array(notifications)
    }

    /// Burns gas
    pub fn runtime_burn_gas(&mut self, amount: i64) -> CoreResult<()> {
        if amount <= 0 {
            return Err(CoreError::other("GAS must be positive."));
        }

        let datoshi = u64::try_from(amount).map_err(|_| CoreError::other("GAS amount overflow"))?;
        self.charge_execution_fee(datoshi)?;
        Ok(())
    }

    /// Gets the signers of the current transaction (matches C# GetCurrentSigners).
    pub fn runtime_current_signers(&mut self) -> CoreResult<()> {
        let Some(container) = self.get_script_container() else {
            return self.push_null();
        };

        let Some(tx) = container
            .as_ref()
            .as_any()
            .downcast_ref::<neo_payloads::Transaction>()
        else {
            return self.push_null();
        };

        let items = tx
            .signers()
            .iter()
            .map(|s| {
                let sv = <neo_payloads::Signer as neo_vm::Interoperable>::to_stack_value(s)
                    .map_err(|e| CoreError::other(e.to_string()))?;
                StackItem::try_from(sv).map_err(|e| CoreError::other(e.to_string()))
            })
            .collect::<CoreResult<Vec<_>>>()?;

        self.push_array(items)
    }
}

fn map_runtime_result(service: &str, result: CoreResult<()>) -> VmResult<()> {
    result.map_err(|error| VmError::InteropService {
        service: service.to_string(),
        error: error.to_string(),
    })
}

fn runtime_platform_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    map_runtime_result("System.Runtime.Platform", app.runtime_platform())
}

fn runtime_get_trigger_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    map_runtime_result("System.Runtime.GetTrigger", app.runtime_get_trigger())
}

fn runtime_get_network_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    map_runtime_result("System.Runtime.GetNetwork", app.runtime_get_network())
}

fn runtime_get_address_version_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    map_runtime_result(
        "System.Runtime.GetAddressVersion",
        app.runtime_get_address_version(),
    )
}

fn runtime_get_time_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    map_runtime_result("System.Runtime.GetTime", app.runtime_get_time())
}

fn runtime_get_script_container_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    map_runtime_result(
        "System.Runtime.GetScriptContainer",
        app.runtime_get_script_container(),
    )
}

fn runtime_load_script_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let args = app.pop_array().map_err(|e| VmError::InteropService {
        service: "System.Runtime.LoadScript".to_string(),
        error: e.to_string(),
    })?;

    let call_flags_value = app.pop_integer().map_err(|e| VmError::InteropService {
        service: "System.Runtime.LoadScript".to_string(),
        error: e.to_string(),
    })?;

    let script = app.pop_bytes().map_err(|e| VmError::InteropService {
        service: "System.Runtime.LoadScript".to_string(),
        error: e.to_string(),
    })?;

    let result = (|| -> CoreResult<()> {
        if call_flags_value < 0 || call_flags_value > u8::MAX as i64 {
            return Err(CoreError::other("Invalid call flags value"));
        }

        let raw = call_flags_value as u8;
        let Some(call_flags) = CallFlags::from_bits(raw) else {
            return Err(CoreError::other(format!(
                "Invalid call flags: {call_flags_value}"
            )));
        };

        app.runtime_load_script(script, call_flags, args)
    })();

    map_runtime_result("System.Runtime.LoadScript", result)
}

fn runtime_get_executing_script_hash_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    map_runtime_result(
        "System.Runtime.GetExecutingScriptHash",
        app.runtime_get_executing_script_hash(),
    )
}

fn runtime_get_calling_script_hash_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    map_runtime_result(
        "System.Runtime.GetCallingScriptHash",
        app.runtime_get_calling_script_hash(),
    )
}

fn runtime_get_entry_script_hash_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    map_runtime_result(
        "System.Runtime.GetEntryScriptHash",
        app.runtime_get_entry_script_hash(),
    )
}

fn runtime_check_witness_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    map_runtime_result("System.Runtime.CheckWitness", app.runtime_check_witness())
}

fn runtime_get_invocation_counter_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    map_runtime_result(
        "System.Runtime.GetInvocationCounter",
        app.runtime_get_invocation_counter(),
    )
}

fn runtime_get_random_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    map_runtime_result("System.Runtime.GetRandom", app.runtime_get_random())
}

fn runtime_gas_left_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    map_runtime_result("System.Runtime.GasLeft", app.runtime_gas_left())
}

fn runtime_log_handler(app: &mut ApplicationEngine, _engine: &mut ExecutionEngine) -> VmResult<()> {
    map_runtime_result("System.Runtime.Log", app.runtime_log())
}

fn runtime_notify_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    map_runtime_result("System.Runtime.Notify", app.runtime_notify())
}

fn runtime_get_notifications_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    map_runtime_result(
        "System.Runtime.GetNotifications",
        app.runtime_get_notifications(),
    )
}

fn runtime_burn_gas_handler(
    app: &mut ApplicationEngine,
    engine: &mut ExecutionEngine,
) -> VmResult<()> {
    let amount_item = engine.pop()?;
    let amount = amount_item
        .into_int()?
        .to_i64()
        .ok_or_else(|| VmError::InteropService {
            service: "System.Runtime.BurnGas".to_string(),
            error: "Gas amount does not fit into i64".to_string(),
        })?;
    map_runtime_result("System.Runtime.BurnGas", app.runtime_burn_gas(amount))
}

fn runtime_current_signers_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    map_runtime_result(
        "System.Runtime.CurrentSigners",
        app.runtime_current_signers(),
    )
}

impl ApplicationEngine {
    pub(crate) fn register_runtime_interops(&mut self) -> VmResult<()> {
        self.register_host_service(
            "System.Runtime.Platform",
            1 << 3,
            CallFlags::NONE,
            runtime_platform_handler,
        )?;
        self.register_host_service(
            "System.Runtime.GetTrigger",
            1 << 3,
            CallFlags::NONE,
            runtime_get_trigger_handler,
        )?;
        self.register_host_service(
            "System.Runtime.GetNetwork",
            1 << 3,
            CallFlags::NONE,
            runtime_get_network_handler,
        )?;
        self.register_host_service(
            "System.Runtime.GetAddressVersion",
            1 << 3,
            CallFlags::NONE,
            runtime_get_address_version_handler,
        )?;
        self.register_host_service(
            "System.Runtime.GetTime",
            1 << 3,
            CallFlags::NONE,
            runtime_get_time_handler,
        )?;
        self.register_host_service(
            "System.Runtime.GetScriptContainer",
            1 << 3,
            CallFlags::NONE,
            runtime_get_script_container_handler,
        )?;
        self.register_host_service(
            "System.Runtime.LoadScript",
            1 << 15,
            CallFlags::ALLOW_CALL,
            runtime_load_script_handler,
        )?;
        self.register_host_service(
            "System.Runtime.GetExecutingScriptHash",
            1 << 4,
            CallFlags::NONE,
            runtime_get_executing_script_hash_handler,
        )?;
        self.register_host_service(
            "System.Runtime.GetCallingScriptHash",
            1 << 4,
            CallFlags::NONE,
            runtime_get_calling_script_hash_handler,
        )?;
        self.register_host_service(
            "System.Runtime.GetEntryScriptHash",
            1 << 4,
            CallFlags::NONE,
            runtime_get_entry_script_hash_handler,
        )?;
        self.register_host_service(
            "System.Runtime.CheckWitness",
            1 << 10,
            CallFlags::NONE,
            runtime_check_witness_handler,
        )?;
        self.register_host_service(
            "System.Runtime.GetInvocationCounter",
            1 << 4,
            CallFlags::NONE,
            runtime_get_invocation_counter_handler,
        )?;
        self.register_host_service(
            "System.Runtime.GetRandom",
            0,
            CallFlags::NONE,
            runtime_get_random_handler,
        )?;
        self.register_host_service(
            "System.Runtime.GasLeft",
            1 << 4,
            CallFlags::NONE,
            runtime_gas_left_handler,
        )?;
        self.register_host_service(
            "System.Runtime.Log",
            1 << 15,
            CallFlags::ALLOW_NOTIFY,
            runtime_log_handler,
        )?;
        self.register_host_service(
            "System.Runtime.Notify",
            1 << 15,
            CallFlags::ALLOW_NOTIFY,
            runtime_notify_handler,
        )?;
        self.register_host_service(
            "System.Runtime.GetNotifications",
            1 << 12,
            CallFlags::NONE,
            runtime_get_notifications_handler,
        )?;
        self.register_host_service(
            "System.Runtime.BurnGas",
            1 << 4,
            CallFlags::NONE,
            runtime_burn_gas_handler,
        )?;
        self.register_host_service(
            "System.Runtime.CurrentSigners",
            1 << 4,
            CallFlags::NONE,
            runtime_current_signers_handler,
        )?;
        Ok(())
    }
}

fn validate_event_parameters(
    state: &[StackItem],
    parameters: &[ContractParameterDefinition],
) -> CoreResult<()> {
    for (item, parameter) in state.iter().zip(parameters.iter()) {
        if !matches_parameter_type(item, parameter.param_type) {
            return Err(CoreError::other(format!(
                "The type of the argument `{}` does not match the formal parameter.",
                parameter.name
            )));
        }
    }
    Ok(())
}

fn matches_parameter_type(item: &StackItem, expected: ContractParameterType) -> bool {
    use ContractParameterType::*;

    let item_type = item.stack_item_type();
    if matches!(item_type, StackItemType::Pointer) {
        return false;
    }
    match expected {
        Any => true,
        Boolean => matches!(item_type, StackItemType::Boolean),
        Integer => matches!(item_type, StackItemType::Integer),
        ByteArray => {
            matches!(item_type, StackItemType::Any)
                || matches!(item_type, StackItemType::ByteString | StackItemType::Buffer)
        }
        String => {
            if matches!(item_type, StackItemType::ByteString | StackItemType::Buffer) {
                item.as_bytes()
                    .ok()
                    .and_then(|bytes| StdString::from_utf8(bytes).ok())
                    .is_some()
            } else {
                false
            }
        }
        Hash160 => match item_type {
            StackItemType::Any => true,
            StackItemType::ByteString | StackItemType::Buffer => item
                .as_bytes()
                .map(|bytes| bytes.len() == ADDRESS_SIZE)
                .unwrap_or(false),
            _ => false,
        },
        Hash256 => match item_type {
            StackItemType::Any => true,
            StackItemType::ByteString | StackItemType::Buffer => item
                .as_bytes()
                .map(|bytes| bytes.len() == HASH_SIZE)
                .unwrap_or(false),
            _ => false,
        },
        PublicKey => match item_type {
            StackItemType::Any => true,
            StackItemType::ByteString | StackItemType::Buffer => item
                .as_bytes()
                .map(|bytes| bytes.len() == 33)
                .unwrap_or(false),
            _ => false,
        },
        Signature => match item_type {
            StackItemType::Any => true,
            StackItemType::ByteString | StackItemType::Buffer => item
                .as_bytes()
                .map(|bytes| bytes.len() == 64)
                .unwrap_or(false),
            _ => false,
        },
        Array => matches!(
            item_type,
            StackItemType::Any | StackItemType::Array | StackItemType::Struct
        ),
        Map => matches!(item_type, StackItemType::Any | StackItemType::Map),
        InteropInterface => matches!(
            item_type,
            StackItemType::Any | StackItemType::InteropInterface
        ),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application_engine::TEST_MODE_GAS;
    use neo_config::ProtocolSettings;
    use neo_primitives::TriggerType;
    use neo_storage::DataCache;
    use neo_vm::Script;
    use neo_vm::script_builder::ScriptBuilder;
    use neo_vm_rs::OpCode;
    use neo_vm_rs::VmState as VMState;
    use std::sync::Arc;

    #[test]
    fn notification_parameter_type_check_matches_csharp() {
        assert!(!matches_parameter_type(
            &StackItem::null(),
            ContractParameterType::String
        ));
        assert!(matches_parameter_type(
            &StackItem::from_byte_string(b"neo".to_vec()),
            ContractParameterType::String
        ));
        assert!(!matches_parameter_type(
            &StackItem::from_byte_string(vec![0xff]),
            ContractParameterType::String
        ));

        let pointer = StackItem::from_pointer(Arc::new(Script::new_from_bytes(vec![])), 0);
        for expected in [
            ContractParameterType::Any,
            ContractParameterType::ByteArray,
            ContractParameterType::InteropInterface,
        ] {
            assert!(
                !matches_parameter_type(&pointer, expected),
                "C# CheckItemType rejects Pointer before matching {expected:?}"
            );
        }
    }

    #[test]
    fn runtime_log_allows_dynamic_script_without_container_like_csharp() {
        let mut builder = ScriptBuilder::new();
        builder.emit_push_string("dynamic log");
        builder
            .emit_syscall("System.Runtime.Log")
            .expect("emit Runtime.Log");
        builder.emit_opcode(OpCode::RET);

        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            None,
            Arc::new(DataCache::new(false)),
            None,
            ProtocolSettings::default(),
            TEST_MODE_GAS,
            None,
        )
        .expect("application engine");
        engine
            .load_script(builder.to_array(), CallFlags::ALLOW_NOTIFY, None)
            .expect("load script");

        assert_eq!(engine.execute_allow_fault(), VMState::HALT);

        let log = engine.logs().first().expect("log event");
        assert!(log.script_container.is_none());
        assert_eq!(log.message, "dynamic log");
    }

    #[test]
    fn send_notification_enforces_echidna_cap_for_native_paths_like_csharp() {
        let mut settings = ProtocolSettings::default();
        settings.hardforks.clear();
        settings.hardforks.insert(Hardfork::HfEchidna, 0);

        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            None,
            Arc::new(DataCache::new(false)),
            None,
            settings,
            TEST_MODE_GAS,
            None,
        )
        .expect("application engine");
        engine
            .load_script(vec![OpCode::RET.byte()], CallFlags::ALLOW_NOTIFY, None)
            .expect("load script");

        for _ in 0..crate::application_engine::MAX_NOTIFICATION_COUNT {
            engine
                .send_notification(UInt160::zero(), "Native".to_string(), Vec::new())
                .expect("notification below cap");
        }

        let err = engine
            .send_notification(UInt160::zero(), "Native".to_string(), Vec::new())
            .expect_err("513th application notification must fault after Echidna");
        assert!(err.to_string().contains("Maximum number of notifications"));
        assert_eq!(
            engine.notifications().len(),
            crate::application_engine::MAX_NOTIFICATION_COUNT
        );
    }

    #[test]
    fn get_notifications_deep_copies_domovoi_state_like_csharp() {
        let mut settings = ProtocolSettings::default();
        settings.hardforks.clear();
        settings.hardforks.insert(Hardfork::HfDomovoi, 0);

        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            None,
            Arc::new(DataCache::new(false)),
            None,
            settings,
            TEST_MODE_GAS,
            None,
        )
        .expect("application engine");
        engine
            .load_script(vec![OpCode::RET.byte()], CallFlags::ALLOW_NOTIFY, None)
            .expect("load script");

        let nested_arg = StackItem::from_array(vec![StackItem::from_i64(1)]);
        engine
            .send_notification(UInt160::zero(), "Native".to_string(), vec![nested_arg])
            .expect("send notification");

        let StackItem::Array(stored_nested) = &engine.notifications()[0].state[0] else {
            panic!("stored notification argument should be an array");
        };
        assert!(stored_nested.is_read_only());
        assert!(stored_nested.push(StackItem::from_i64(2)).is_err());

        let notifications = engine.get_notifications(None).expect("get notifications");
        let StackItem::Array(notification) = &notifications[0] else {
            panic!("notification should project as an array");
        };
        let fields = notification.items();
        assert_eq!(fields.len(), 3);

        let StackItem::Array(state) = &fields[2] else {
            panic!("notification state should project as an array");
        };
        assert!(state.is_read_only());

        let state_items = state.items();
        let StackItem::Array(returned_nested) = &state_items[0] else {
            panic!("returned notification argument should be an array");
        };
        assert!(returned_nested.is_read_only());
        assert_ne!(returned_nested.id(), stored_nested.id());
        assert!(returned_nested.push(StackItem::from_i64(3)).is_err());
    }

    #[test]
    fn notification_size_check_uses_stack_value_serializer() {
        let source = include_str!("application_engine_helper.rs");
        let start = source
            .find("pub fn ensure_notification_size")
            .expect("ensure_notification_size exists");
        let end = source[start..]
            .find("pub fn send_notification")
            .map(|offset| start + offset)
            .expect("send_notification follows size check");
        let helper = &source[start..end];

        assert!(helper.contains("notification_state_to_stack_value"));
        assert!(helper.contains("serialize_stack_value_with_limits"));
        assert!(!helper.contains("StackItem::from_array(state.to_vec())"));
        assert!(!helper.contains("BinarySerializer::serialize(&StackItem"));
    }

    #[test]
    fn get_notifications_non_domovoi_projection_uses_stack_value_adapter() {
        let source = include_str!("application_engine_helper.rs");
        let start = source
            .find("fn notification_to_stack_item")
            .expect("notification projection helper exists");
        let end = source[start..]
            .find("fn notification_state_to_stack_value")
            .map(|offset| start + offset)
            .expect("stack value helper follows notification projection");
        let helper = &source[start..end];

        assert!(helper.contains("readonly_array_stack_item"));
        assert!(helper.contains("notification_state_to_stack_value(&notification.state)"));
        assert!(helper.contains("StackItem::try_from(value)"));
        assert!(!helper.contains("StackItem::from_array(notification.state.to_vec())"));
    }

    #[test]
    fn runtime_check_witness_faults_on_invalid_public_key_like_csharp() {
        let invalid_public_key = [0x05; 33];
        let mut builder = ScriptBuilder::new();
        builder.emit_push(&invalid_public_key);
        builder
            .emit_syscall("System.Runtime.CheckWitness")
            .expect("emit Runtime.CheckWitness");
        builder.emit_opcode(OpCode::RET);

        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            None,
            Arc::new(DataCache::new(false)),
            None,
            ProtocolSettings::default(),
            TEST_MODE_GAS,
            None,
        )
        .expect("application engine");
        engine
            .load_script(builder.to_array(), CallFlags::NONE, None)
            .expect("load script");

        assert_eq!(engine.execute_allow_fault(), VMState::FAULT);
    }

    #[test]
    fn verification_trigger_without_persisting_block_uses_configured_hardforks_like_csharp() {
        let settings = ProtocolSettings::default();
        assert!(settings.hardforks.contains_key(&Hardfork::HfAspidochelone));

        let mut builder = ScriptBuilder::new();
        builder
            .emit_syscall("System.Runtime.GetRandom")
            .expect("emit Runtime.GetRandom");
        builder.emit_opcode(OpCode::RET);

        let mut engine = ApplicationEngine::new(
            TriggerType::Verification,
            None,
            Arc::new(DataCache::new(false)),
            None,
            settings,
            TEST_MODE_GAS,
            None,
        )
        .expect("application engine");
        engine
            .load_script(builder.to_array(), CallFlags::NONE, None)
            .expect("load script");

        assert_eq!(engine.execute_allow_fault(), VMState::HALT);
        assert_eq!(engine.fee_consumed(), (1 << 13) * 30);
    }

    #[test]
    fn invocation_counter_uses_explicit_context_script_hash_like_csharp() {
        let logical_hash = UInt160::from_bytes(&[0x42; 20]).expect("logical hash");

        let mut builder = ScriptBuilder::new();
        builder
            .emit_syscall("System.Runtime.GetInvocationCounter")
            .expect("emit Runtime.GetInvocationCounter");
        builder.emit_opcode(OpCode::RET);

        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            None,
            Arc::new(DataCache::new(false)),
            None,
            ProtocolSettings::default(),
            TEST_MODE_GAS,
            None,
        )
        .expect("application engine");
        engine
            .load_script(builder.to_array(), CallFlags::NONE, Some(logical_hash))
            .expect("load script with logical hash");

        assert_eq!(engine.execute_allow_fault(), VMState::HALT);

        let result = engine
            .result_stack()
            .peek(0)
            .expect("invocation counter result")
            .as_int()
            .expect("integer result");
        assert_eq!(result, 1.into());
    }
}
