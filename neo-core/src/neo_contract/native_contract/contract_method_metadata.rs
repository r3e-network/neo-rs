use std::fmt;
use neo_vm::StackItem;
use crate::hardfork::Hardfork;
use crate::neo_contract::application_engine::ApplicationEngine;
use crate::neo_contract::call_flags::CallFlags;
use crate::neo_contract::contract_parameter_type::ContractParameterType;
use crate::neo_contract::interop_parameter_descriptor::InteropParameterDescriptor;
use crate::neo_contract::manifest::contract_method_descriptor::ContractMethodDescriptor;
use crate::neo_contract::native_contract::ihardfork_activable::IHardforkActivable;

#[derive(Debug)]
pub struct ContractMethodMetadata {
    pub name: String,
    pub handler: fn(&ApplicationEngine) -> Result<StackItem, String>,
    pub parameters: Vec<InteropParameterDescriptor>,
    pub need_application_engine: bool,
    pub need_snapshot: bool,
    pub cpu_fee: i64,
    pub storage_fee: i64,
    pub required_call_flags: CallFlags,
    pub descriptor: ContractMethodDescriptor,
    pub active_in: Option<Hardfork>,
    pub deprecated_in: Option<Hardfork>,
}

impl fmt::Display for ContractMethodMetadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl ContractMethodMetadata {
    pub fn new(
        name: String,
        handler: fn(&ApplicationEngine) -> Result<StackItem, String>,
        parameters: Vec<InteropParameterDescriptor>,
        need_application_engine: bool,
        need_snapshot: bool,
        cpu_fee: i64,
        storage_fee: i64,
        required_call_flags: CallFlags,
        descriptor: ContractMethodDescriptor,
        active_in: Option<Hardfork>,
        deprecated_in: Option<Hardfork>,
    ) -> Self {
        Self {
            name,
            handler,
            parameters,
            need_application_engine,
            need_snapshot,
            cpu_fee,
            storage_fee,
            required_call_flags,
            descriptor,
            active_in,
            deprecated_in,
        }
    }
}

impl IHardforkActivable for ContractMethodMetadata {
    fn active_in(&self) -> Option<Hardfork> {
        self.active_in
    }

    fn deprecated_in(&self) -> Option<Hardfork> {
        self.deprecated_in
    }
}

pub fn to_parameter_type(type_name: &str) -> ContractParameterType {
    match type_name {
        "ContractTask" => ContractParameterType::Void,
        "void" => ContractParameterType::Void,
        "bool" => ContractParameterType::Boolean,
        "i8" | "u8" | "i16" | "u16" | "i32" | "u32" | "i64" | "u64" | "BigInteger" => ContractParameterType::Integer,
        "Vec<u8>" => ContractParameterType::ByteArray,
        "String" => ContractParameterType::String,
        "H160" => ContractParameterType::Hash160,
        "H256" => ContractParameterType::Hash256,
        "ECPoint" => ContractParameterType::PublicKey,
        "Boolean" => ContractParameterType::Boolean,
        "Integer" => ContractParameterType::Integer,
        "ByteString" | "Buffer" => ContractParameterType::ByteArray,
        "Array" | "Struct" => ContractParameterType::Array,
        "Map" => ContractParameterType::Map,
        "StackItem" | "Any" => ContractParameterType::Any,
        _ => {
            if type_name.starts_with("IInteroperable") || type_name.starts_with("ISerializable") {
                ContractParameterType::Array
            } else if type_name.ends_with("[]") {
                ContractParameterType::Array
            } else {
                ContractParameterType::InteropInterface
            }
        }
    }
}
