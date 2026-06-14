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
use std::sync::Arc;

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
            self.push(
                transaction
                    .to_stack_item()
                    .map_err(|e| CoreError::other(e.to_string()))?,
            )
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
                // Convert public key to hash
                let hash = self.pubkey_to_hash(&hash_or_pubkey);
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

        let counter = self.get_invocation_counter(&hash);
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

        let container = self
            .script_container()
            .ok_or_else(|| CoreError::other("No script container"))?;

        let script_hash = self.current_script_hash().unwrap_or_else(UInt160::zero);

        let event = LogEventArgs::new(Arc::clone(container), script_hash, message);

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
        self.reserve_notification_slot()?;

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
        self.reserve_notification_slot()?;

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
                s.to_stack_item()
                    .map_err(|e| CoreError::other(e.to_string()))
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
    match expected {
        Any => true,
        Boolean => matches!(item_type, StackItemType::Boolean),
        Integer => matches!(item_type, StackItemType::Integer),
        ByteArray => {
            matches!(item_type, StackItemType::Any)
                || matches!(item_type, StackItemType::ByteString | StackItemType::Buffer)
        }
        String => {
            if matches!(item_type, StackItemType::Any) {
                true
            } else if matches!(item_type, StackItemType::ByteString | StackItemType::Buffer) {
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
