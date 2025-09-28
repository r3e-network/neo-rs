//! ApplicationEngine.Runtime - matches C# Neo.SmartContract.ApplicationEngine.Runtime.cs exactly

use crate::hardfork::Hardfork;
use crate::neo_config::{ADDRESS_SIZE, HASH_SIZE};
use crate::network::p2p::payloads::{Signer, Transaction};
use crate::smart_contract::application_engine::{
    ApplicationEngine, MAX_EVENT_NAME, MAX_NOTIFICATION_SIZE,
};
use crate::smart_contract::contract_parameter_type::ContractParameterType;
use crate::smart_contract::execution_context_state::ExecutionContextState;
use crate::smart_contract::log_event_args::LogEventArgs;
use crate::smart_contract::notify_event_args::NotifyEventArgs;
use crate::smart_contract::trigger_type::TriggerType;
use crate::smart_contract::ContractParameterDefinition;
use crate::witness_rule::{WitnessCondition, WitnessConditionType, WitnessRule, WitnessRuleAction};
use crate::witness_scope::WitnessScope;
use crate::{UInt160, UInt256};
use murmur3::murmur3_x64_128;
use neo_vm::call_flags::CallFlags;
use neo_vm::{ExecutionEngine, Script, StackItem, StackItemType, VmError, VmResult};
use num_bigint::{BigInt, Sign};
use std::convert::TryFrom;
use std::io::Cursor;
use std::sync::Arc;

impl ApplicationEngine {
    /// Gets the platform name
    pub fn runtime_platform(&mut self) -> Result<(), String> {
        self.push_string("NEO".to_string())
    }

    /// Gets the trigger type
    pub fn runtime_get_trigger(&mut self) -> Result<(), String> {
        self.push_integer(self.trigger as i64)
    }

    /// Gets the network magic number
    pub fn runtime_get_network(&mut self) -> Result<(), String> {
        self.push_integer(self.protocol_settings.network as i64)
    }

    /// Gets the address version of the current network
    pub fn runtime_get_address_version(&mut self) -> Result<(), String> {
        self.push_integer(self.protocol_settings.address_version as i64)
    }

    /// Gets the current block time
    pub fn runtime_get_time(&mut self) -> Result<(), String> {
        let time = self.get_current_block_time()?;
        let big = BigInt::from(time);
        self.push(StackItem::from_int(big))
            .map_err(|e| e.to_string())
    }

    /// Gets the script container
    pub fn runtime_get_script_container(&mut self) -> Result<(), String> {
        let container = self
            .get_script_container()
            .cloned()
            .ok_or_else(|| "No script container".to_string())?;
        self.push_interop_container(container)
    }

    /// Gets the executing script hash
    pub fn runtime_get_executing_script_hash(&mut self) -> Result<(), String> {
        let hash = self.current_script_hash().ok_or("No executing script")?;
        self.push_bytes(hash.to_bytes())
    }

    /// Gets the calling script hash
    pub fn runtime_get_calling_script_hash(&mut self) -> Result<(), String> {
        let hash = self.calling_script_hash().unwrap_or_default();
        self.push_bytes(hash.to_bytes())
    }

    /// Gets the entry script hash
    pub fn runtime_get_entry_script_hash(&mut self) -> Result<(), String> {
        let hash = self.entry_script_hash().ok_or("No entry script")?;
        self.push_bytes(hash.to_bytes())
    }

    /// Checks witness
    pub fn runtime_check_witness(&mut self) -> Result<(), String> {
        let hash_or_pubkey = self.pop_bytes()?;

        // Check if it's a hash (20 bytes) or public key (33 bytes)
        let result = match hash_or_pubkey.len() {
            20 => {
                let hash = UInt160::from_bytes(&hash_or_pubkey).map_err(|e| e.to_string())?;
                self.check_witness_hash(&hash)
            }
            33 => {
                // Convert public key to hash
                let hash = self.pubkey_to_hash(&hash_or_pubkey);
                self.check_witness_hash(&hash)
            }
            _ => false,
        };

        self.push_boolean(result)
    }

    /// Gets invocation counter
    pub fn runtime_get_invocation_counter(&mut self) -> Result<(), String> {
        let hash = self.current_script_hash().ok_or("No current script")?;

        let counter = self.get_invocation_counter(&hash);
        self.push_integer(counter as i64)
    }

    /// Logs a message
    pub fn runtime_log(&mut self) -> Result<(), String> {
        let message_bytes = self.pop_bytes()?;

        if message_bytes.len() > MAX_NOTIFICATION_SIZE {
            return Err(format!(
                "Notification size {} exceeds maximum allowed size of {} bytes",
                message_bytes.len(),
                MAX_NOTIFICATION_SIZE
            ));
        }

        let message = String::from_utf8(message_bytes).map_err(|_| {
            "Failed to convert byte array to string: Invalid UTF-8 sequence".to_string()
        })?;

        let container = self
            .script_container
            .as_ref()
            .ok_or_else(|| "No script container".to_string())?;

        let script_hash = self.current_script_hash().copied().unwrap_or_default();

        let event = LogEventArgs::new(Arc::clone(container), script_hash, message);

        self.emit_log_event(event);
        Ok(())
    }

    /// Sends a notification
    pub fn runtime_notify(&mut self) -> Result<(), String> {
        let state = self.pop_array()?;
        let event_name = self.pop_bytes()?;

        if event_name.len() > MAX_EVENT_NAME {
            return Err(format!(
                "Event name size {} exceeds maximum allowed size of {} bytes",
                event_name.len(),
                MAX_EVENT_NAME
            ));
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
    ) -> Result<(), String> {
        let event_name = String::from_utf8(event_name_bytes)
            .map_err(|_| "Failed to convert event name to UTF-8 string".to_string())?;

        {
            let state_arc = self.current_execution_state().map_err(|e| e.to_string())?;
            let state_guard = state_arc
                .lock()
                .map_err(|_| "Execution context state lock poisoned".to_string())?;
            if state_guard.contract.is_none() {
                return Err("Notifications are not allowed in dynamic scripts.".to_string());
            }
        }

        self.ensure_notification_size(&state)?;
        self.reserve_notification_slot()?;

        let script_hash = self
            .current_script_hash()
            .copied()
            .unwrap_or_else(UInt160::zero);

        self.send_notification(script_hash, event_name, state)
    }

    fn runtime_notify_basilisk(
        &mut self,
        event_name_bytes: Vec<u8>,
        state: Vec<StackItem>,
    ) -> Result<(), String> {
        let event_name = String::from_utf8(event_name_bytes)
            .map_err(|_| "Failed to convert event name to UTF-8 string".to_string())?;

        let script_hash = self
            .current_script_hash()
            .copied()
            .unwrap_or_else(UInt160::zero);

        let parameters = {
            let state_arc = self.current_execution_state().map_err(|e| e.to_string())?;
            let guard = state_arc
                .lock()
                .map_err(|_| "Execution context state lock poisoned".to_string())?;
            let contract = guard
                .contract
                .clone()
                .ok_or_else(|| "Notifications are not allowed in dynamic scripts.".to_string())?;
            let event = contract
                .manifest
                .abi
                .events
                .iter()
                .find(|descriptor| descriptor.name == event_name)
                .cloned()
                .ok_or_else(|| format!("Event `{}` does not exist.", event_name))?;
            event.parameters
        };

        if parameters.len() != state.len() {
            return Err(
                "The number of the arguments does not match the formal parameters of the event."
                    .to_string(),
            );
        }

        validate_event_parameters(&state, &parameters)?;
        self.ensure_notification_size(&state)?;
        self.reserve_notification_slot()?;

        self.send_notification(script_hash, event_name, state)
    }

    /// Gets notifications
    pub fn runtime_get_notifications(&mut self) -> Result<(), String> {
        let hash = if self.peek_is_null(0)? {
            self.pop()?;
            None
        } else {
            let bytes = self.pop_bytes()?;
            if bytes.len() != 20 {
                return Err("Invalid hash length".to_string());
            }
            Some(UInt160::from_bytes(&bytes).map_err(|e| e.to_string())?)
        };

        // Get notifications for the specified contract (or all if None)
        let notifications = self.get_notifications(hash)?;

        self.push_array(notifications)
    }

    /// Burns gas
    pub fn runtime_burn_gas(&mut self, amount: i64) -> Result<(), String> {
        if amount <= 0 {
            return Err("GAS must be positive.".to_string());
        }

        let datoshi = u64::try_from(amount).map_err(|_| "GAS amount overflow".to_string())?;
        self.add_fee(datoshi).map_err(|e| e.to_string())?;
        Ok(())
    }
}
fn map_runtime_result(service: &str, result: Result<(), String>) -> VmResult<()> {
    result.map_err(|error| VmError::InteropService {
        service: service.to_string(),
        error,
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
        .as_int()?
        .to_i64()
        .ok_or_else(|| VmError::InteropService {
            service: "System.Runtime.BurnGas".to_string(),
            error: "Gas amount does not fit into i64".to_string(),
        })?;
    map_runtime_result("System.Runtime.BurnGas", app.runtime_burn_gas(amount))
}

pub(crate) fn register_runtime_interops(engine: &mut ApplicationEngine) -> VmResult<()> {
    engine.register_host_service(
        "System.Runtime.Platform",
        1 << 3,
        CallFlags::NONE,
        runtime_platform_handler,
    )?;
    engine.register_host_service(
        "System.Runtime.GetTrigger",
        1 << 3,
        CallFlags::NONE,
        runtime_get_trigger_handler,
    )?;
    engine.register_host_service(
        "System.Runtime.GetNetwork",
        1 << 3,
        CallFlags::NONE,
        runtime_get_network_handler,
    )?;
    engine.register_host_service(
        "System.Runtime.GetTime",
        1 << 3,
        CallFlags::NONE,
        runtime_get_time_handler,
    )?;
    engine.register_host_service(
        "System.Runtime.GetScriptContainer",
        1 << 3,
        CallFlags::NONE,
        runtime_get_script_container_handler,
    )?;
    engine.register_host_service(
        "System.Runtime.GetExecutingScriptHash",
        1 << 4,
        CallFlags::NONE,
        runtime_get_executing_script_hash_handler,
    )?;
    engine.register_host_service(
        "System.Runtime.GetCallingScriptHash",
        1 << 4,
        CallFlags::NONE,
        runtime_get_calling_script_hash_handler,
    )?;
    engine.register_host_service(
        "System.Runtime.GetEntryScriptHash",
        1 << 4,
        CallFlags::NONE,
        runtime_get_entry_script_hash_handler,
    )?;
    engine.register_host_service(
        "System.Runtime.CheckWitness",
        1 << 10,
        CallFlags::NONE,
        runtime_check_witness_handler,
    )?;
    engine.register_host_service(
        "System.Runtime.GetInvocationCounter",
        1 << 4,
        CallFlags::NONE,
        runtime_get_invocation_counter_handler,
    )?;
    engine.register_host_service(
        "System.Runtime.Log",
        1 << 15,
        CallFlags::ALLOW_NOTIFY,
        runtime_log_handler,
    )?;
    engine.register_host_service(
        "System.Runtime.Notify",
        1 << 15,
        CallFlags::ALLOW_NOTIFY,
        runtime_notify_handler,
    )?;
    engine.register_host_service(
        "System.Runtime.GetNotifications",
        1 << 12,
        CallFlags::NONE,
        runtime_get_notifications_handler,
    )?;
    engine.register_host_service(
        "System.Runtime.BurnGas",
        1 << 4,
        CallFlags::NONE,
        runtime_burn_gas_handler,
    )?;
    Ok(())
}

fn validate_event_parameters(
    state: &[StackItem],
    parameters: &[ContractParameterDefinition],
) -> Result<(), String> {
    for (item, parameter) in state.iter().zip(parameters.iter()) {
        if !matches_parameter_type(item, parameter.param_type) {
            return Err(format!(
                "The type of the argument `{}` does not match the formal parameter.",
                parameter.name
            ));
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
                item.get_span()
                    .ok()
                    .and_then(|bytes| String::from_utf8(bytes).ok())
                    .is_some()
            } else {
                false
            }
        }
        Hash160 => match item_type {
            StackItemType::Any => true,
            StackItemType::ByteString | StackItemType::Buffer => item
                .get_span()
                .map(|bytes| bytes.len() == ADDRESS_SIZE)
                .unwrap_or(false),
            _ => false,
        },
        Hash256 => match item_type {
            StackItemType::Any => true,
            StackItemType::ByteString | StackItemType::Buffer => item
                .get_span()
                .map(|bytes| bytes.len() == HASH_SIZE)
                .unwrap_or(false),
            _ => false,
        },
        PublicKey => match item_type {
            StackItemType::Any => true,
            StackItemType::ByteString | StackItemType::Buffer => item
                .get_span()
                .map(|bytes| bytes.len() == 33)
                .unwrap_or(false),
            _ => false,
        },
        Signature => match item_type {
            StackItemType::Any => true,
            StackItemType::ByteString | StackItemType::Buffer => item
                .get_span()
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
