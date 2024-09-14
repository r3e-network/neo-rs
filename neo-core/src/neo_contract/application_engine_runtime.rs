use num_bigint::{BigInt, Sign};
use neo_vm::vm::script::Script;
use neo_vm::vm_types::stack_item::StackItem;
use crate::block::Block;
use crate::contract::Contract;
use crate::cryptography::{ECCurve, ECPoint};
use crate::hardfork::Hardfork;
use crate::neo_contract::application_engine::ApplicationEngine;
use crate::neo_contract::binary_serializer::BinarySerializer;
use crate::neo_contract::call_flags::CallFlags;
use crate::neo_contract::contract_parameter_type::ContractParameterType;
use crate::neo_contract::execution_context_state::ExecutionContextState;
use crate::neo_contract::interop_descriptor::InteropDescriptor;
use crate::neo_contract::log_event_args::LogEventArgs;
use crate::neo_contract::native_contract::NativeContract;
use crate::neo_contract::notify_event_args::NotifyEventArgs;
use crate::network::payloads::{OracleResponse, Signer, WitnessRuleAction};
use crate::persistence::SnapshotCache;
use crate::protocol_settings::ProtocolSettings;
use crate::uint160::UInt160;
use crate::uint256::UInt256;

impl ApplicationEngine {
    /// The maximum length of event name.
    pub const MAX_EVENT_NAME: usize = 32;

    /// The maximum size of notification objects.
    pub const MAX_NOTIFICATION_SIZE: usize = 1024;

    // InteropDescriptor declarations
    pub const  SYSTEM_RUNTIME_PLATFORM: InteropDescriptor = InteropDescriptor::new("System.Runtime.Platform", get_platform, 1 << 3, CallFlags::None);
    pub const SYSTEM_RUNTIME_GET_NETWORK: InteropDescriptor = InteropDescriptor::new("System.Runtime.GetNetwork", get_network, 1 << 3, CallFlags::None);
    pub const SYSTEM_RUNTIME_GET_ADDRESS_VERSION: InteropDescriptor = InteropDescriptor::new("System.Runtime.GetAddressVersion", get_address_version, 1 << 3, CallFlags::None);
    pub const SYSTEM_RUNTIME_GET_TIME: InteropDescriptor = InteropDescriptor::new("System.Runtime.GetTime", get_time, 1 << 3, CallFlags::None);
    pub const SYSTEM_RUNTIME_GET_SCRIPT_CONTAINER: InteropDescriptor = InteropDescriptor::new("System.Runtime.GetScriptContainer", get_script_container, 1 << 3, CallFlags::None);
    pub const SYSTEM_RUNTIME_LOAD_SCRIPT: InteropDescriptor = InteropDescriptor::new("System.Runtime.LoadScript", runtime_load_script, 1 << 15, CallFlags::None);
    pub const SYSTEM_RUNTIME_CHECK_WITNESS: InteropDescriptor = InteropDescriptor::new("System.Runtime.CheckWitness", check_witness, 1 << 10, CallFlags::None);
    pub const SYSTEM_RUNTIME_GET_INVOCATION_COUNTER: InteropDescriptor = InteropDescriptor::new("System.Runtime.GetInvocationCounter", get_invocation_counter, 1 << 3, CallFlags::None);
    pub const SYSTEM_RUNTIME_GET_RANDOM: InteropDescriptor = InteropDescriptor::new("System.Runtime.GetRandom", get_random, 1 << 13, CallFlags::None);
    pub const SYSTEM_RUNTIME_LOG: InteropDescriptor = InteropDescriptor::new("System.Runtime.Log", runtime_log, 1 << 15, CallFlags::None);
    pub const SYSTEM_RUNTIME_NOTIFY: InteropDescriptor = InteropDescriptor::new("System.Runtime.Notify", runtime_notify, 1 << 15, CallFlags::None);
    pub const SYSTEM_RUNTIME_GET_NOTIFICATIONS: InteropDescriptor = InteropDescriptor::new("System.Runtime.GetNotifications", get_notifications, 1 << 15, CallFlags::None);
    pub const SYSTEM_RUNTIME_BURN_GAS: InteropDescriptor = InteropDescriptor::new("System.Runtime.BurnGas", burn_gas, 1 << 4, CallFlags::None);

    /// The implementation of System.Runtime.Platform.
    /// Gets the name of the current platform.
    pub fn get_platform() -> String {
        "NEO".to_string()
    }

    /// The implementation of System.Runtime.GetNetwork.
    /// Gets the magic number of the current network.
    pub fn get_network(&self) -> u32 {
        self.protocol_settings.network
    }

    /// The implementation of System.Runtime.GetAddressVersion.
    /// Gets the address version of the current network.
    pub fn get_address_version(&self) -> u8 {
        self.protocol_settings.address_version
    }

    /// The implementation of System.Runtime.GetTime.
    /// Gets the timestamp of the current block.
    pub fn get_time(&self) -> u64 {
        self.persisting_block.timestamp
    }

    /// The implementation of System.Runtime.GetScriptContainer.
    /// Gets the current script container.
    pub fn get_script_container(&self) -> Result<StackItem, String> {
        match &self.script_container {
            Some(container) if container.is_interoperable() => {
                Ok(container.to_stack_item(&self.reference_counter))
            },
            _ => Err("Invalid script container".to_string()),
        }
    }

    /// The implementation of System.Runtime.LoadScript.
    /// Loads a script at runtime.
    pub fn runtime_load_script(&mut self, script: Vec<u8>, call_flags: CallFlags, args: Array) -> Result<(), String> {
        if (call_flags & !CallFlags::ALL) != CallFlags::NONE {
            return Err("Invalid call flags".to_string());
        }

        let state = self.current_context.get_state::<ExecutionContextState>();
        let context = self.load_script(Script::new(script, true), |p| {
            p.calling_context = Some(self.current_context.clone());
            p.call_flags = call_flags & state.call_flags & CallFlags::READ_ONLY;
            p.is_dynamic_call = true;
        })?;

        for item in args.iter().rev() {
            context.evaluation_stack.push(item.clone());
        }

        Ok(())
    }

    /// The implementation of System.Runtime.CheckWitness.
    /// Determines whether the specified account has witnessed the current transaction.
    pub fn check_witness(&self, hash_or_pubkey: &[u8]) -> Result<bool, String> {
        let hash = match hash_or_pubkey.len() {
            20 => UInt160::from_slice(hash_or_pubkey),
            33 => {
                let point = ECPoint::decode_point(hash_or_pubkey, ECCurve::secp256r1())?;
                Contract::create_signature_redeem_script(&point).to_script_hash()
            },
            _ => return Err("Invalid hashOrPubkey".to_string()),
        };
        self.check_witness_internal(&hash)
    }

    /// Determines whether the specified account has witnessed the current transaction.
    pub fn check_witness_internal(&self, hash: &UInt160) -> Result<bool, String> {
        if hash == &self.calling_script_hash() {
            return Ok(true);
        }

        if let Some(tx) = self.script_container.as_transaction() {
            let signers = if let Some(response) = tx.get_attribute::<OracleResponse>() {
                let request = NativeContract::Oracle.get_request(&self.snapshot_cache, response.id)?;
                NativeContract::Ledger.get_transaction(&self.snapshot_cache, &request.original_txid)?.signers
            } else {
                tx.signers.clone()
            };

            if let Some(signer) = signers.iter().find(|s| s.account == *hash) {
                for rule in signer.get_all_rules() {
                    if rule.condition.matches(self)? {
                        return Ok(rule.action == WitnessRuleAction::Allow);
                    }
                }
                return Ok(false);
            }
            return Ok(false);
        }

        // If we don't have the ScriptContainer, we consider that there are no script hashes for verifying
        if self.script_container.is_none() {
            return Ok(false);
        }

        // Check allow state callflag
        self.validate_call_flags(CallFlags::READ_STATES)?;

        // only for non-Transaction vm_types (Block, etc)
        Ok(self.script_container.as_ref().unwrap().get_script_hashes_for_verifying(&self.snapshot_cache)?.contains(hash))
    }

    /// The implementation of System.Runtime.GetInvocationCounter.
    /// Gets the number of times the current contract has been called during the execution.
    pub fn get_invocation_counter(&mut self) -> i32 {
        let counter = self.invocation_counter.entry(self.current_script_hash.clone())
            .or_insert(1);
        *counter
    }

    /// The implementation of System.Runtime.GetRandom.
    /// Gets the next random number.
    pub fn get_random(&mut self) -> Result<BigInt, String> {
        let (buffer, price) = if self.is_hardfork_enabled(Hardfork::HF_Aspidochelone) {
            let buffer = Cryptography::helper::murmur128(&self.nonce_data, self.protocol_settings.network + self.random_times);
            self.random_times += 1;
            (buffer, 1 << 13)
        } else {
            self.nonce_data = Cryptography::helper::murmur128(&self.nonce_data, self.protocol_settings.network);
            (self.nonce_data.clone(), 1 << 4)
        };
        self.add_fee(price * self.exec_fee_factor)?;
        Ok(BigInt::from_bytes_be(Sign::Plus, &buffer))
    }

    /// The implementation of System.Runtime.Log.
    /// Writes a log.
    pub fn runtime_log(&self, state: &[u8]) -> Result<(), String> {
        if state.len() > Self::MAX_NOTIFICATION_SIZE {
            return Err("Message is too long".to_string());
        }
        match std::str::from_utf8(state) {
            Ok(message) => {
                if let Some(log) = &self.log {
                    log(LogEventArgs::new(
                        self.script_container.clone(),
                        self.current_script_hash.clone(),
                        message.to_string()
                    ));
                }
                Ok(())
            },
            Err(_) => Err("Failed to convert byte array to string: Invalid or non-printable UTF-8 sequence detected".to_string()),
        }
    }

    /// The implementation of System.Runtime.Notify.
    /// Sends a notification.
    pub fn runtime_notify(&mut self, event_name: &[u8], state: Array) -> Result<(), String> {
        if !self.is_hardfork_enabled(Hardfork::HF_Basilisk) {
            return self.runtime_notify_v1(event_name, state);
        }
        if event_name.len() > Self::MAX_EVENT_NAME {
            return Err("Event name is too long".to_string());
        }
        let name = std::str::from_utf8(event_name)
            .map_err(|_| "Invalid UTF-8 in event name".to_string())?;
        let contract = self.current_context.get_state::<ExecutionContextState>().contract
            .ok_or("Notifications are not allowed in dynamic scripts")?;
        let event = contract.manifest.abi.events.iter()
            .find(|e| e.name == name)
            .ok_or_else(|| format!("Event `{}` does not exist", name))?;
        if event.parameters.len() != state.len() {
            return Err("The number of the arguments does not match the formal parameters of the event".to_string());
        }
        for (i, p) in event.parameters.iter().enumerate() {
            if !Self::check_item_type(&state[i], p.type_) {
                return Err(format!("The type of the argument `{}` does not match the formal parameter", p.name));
            }
        }
        let mut buffer = Vec::with_capacity(Self::MAX_NOTIFICATION_SIZE);
        BinarySerializer::serialize(&mut buffer, &state, Self::MAX_NOTIFICATION_SIZE, Limits::MAX_STACK_SIZE)?;
        self.send_notification(self.current_script_hash.clone(), name.to_string(), state)
    }

    /// Sends a notification for the specified contract.
    pub fn send_notification(&mut self, hash: UInt160, event_name: String, state: Array) -> Result<(), String> {
        let notification = NotifyEventArgs::new(
            self.script_container.clone(),
            hash,
            event_name,
            state.deep_copy(true)
        );
        if let Some(notify) = &self.notify {
            notify(notification.clone());
        }
        self.notifications.get_or_insert_with(Vec::new).push(notification);
        self.current_context.get_state_mut::<ExecutionContextState>().notification_count += 1;
        Ok(())
    }

    /// The implementation of System.Runtime.GetNotifications.
    /// Gets the notifications sent by the specified contract during the execution.
    pub fn get_notifications(&self, hash: Option<&UInt160>) -> Result<Array, String> {
        let notifications = self.notifications.as_ref()
            .map(|n| n.iter())
            .unwrap_or_else(|| [].iter());
        let filtered = if let Some(h) = hash {
            notifications.filter(|n| &n.script_hash == h).collect::<Vec<_>>()
        } else {
            notifications.collect::<Vec<_>>()
        };
        if filtered.len() > Limits::MAX_STACK_SIZE {
            return Err("Too many notifications".to_string());
        }
        let notify_array = Array::new(self.reference_counter.clone());
        for notify in filtered {
            notify_array.add(notify.to_stack_item(&self.reference_counter, self)?);
        }
        Ok(notify_array)
    }

    /// The implementation of System.Runtime.BurnGas.
    /// Burning GAS to benefit the NEO ecosystem.
    pub fn burn_gas(&mut self, datoshi: i64) -> Result<(), String> {
        if datoshi <= 0 {
            return Err("GAS must be positive".to_string());
        }
        self.add_fee(datoshi)
    }

    /// Get the Signers of the current transaction.
    pub fn get_current_signers(&self) -> Option<Vec<Signer>> {
        self.script_container.as_transaction().map(|tx| tx.signers.clone())
    }

    // Helper function to check item type
    fn check_item_type(item: &StackItem, type_: ContractParameterType) -> bool {
        match type_ {
            ContractParameterType::Any => true,
            ContractParameterType::Boolean => matches!(item, StackItem::Boolean(_)),
            ContractParameterType::Integer => matches!(item, StackItem::Integer(_)),
            ContractParameterType::ByteArray => matches!(item, StackItem::ByteString(_) | StackItem::Buffer(_)),
            ContractParameterType::String => {
                if let StackItem::ByteString(bytes) | StackItem::Buffer(bytes) = item {
                    std::str::from_utf8(bytes).is_ok()
                } else {
                    false
                }
            },
            ContractParameterType::Hash160 => {
                matches!(item, StackItem::ByteString(bytes) | StackItem::Buffer(bytes) if bytes.len() == UInt160::LEN)
            },
            ContractParameterType::Hash256 => {
                matches!(item, StackItem::ByteString(bytes) | StackItem::Buffer(bytes) if bytes.len() == UInt256::LEN)
            },
            ContractParameterType::PublicKey => {
                matches!(item, StackItem::ByteString(bytes) | StackItem::Buffer(bytes) if bytes.len() == 33)
            },
            ContractParameterType::Signature => {
                matches!(item, StackItem::ByteString(bytes) | StackItem::Buffer(bytes) if bytes.len() == 64)
            },
            ContractParameterType::Array => matches!(item, StackItem::Array(_) | StackItem::Struct(_)),
            ContractParameterType::Map => matches!(item, StackItem::Map(_)),
            ContractParameterType::InteropInterface => matches!(item, StackItem::InteropInterface(_)),
            _ => false,
        }
    }
}
