use std::fmt;
use crate::hardfork::Hardfork;
use crate::neo_contract::contract_parameter::ContractParameterType;
use crate::neo_contract::manifest::contract_event_descriptor::ContractEventDescriptor;

#[derive(Clone, Debug)]
pub struct ContractEventAttribute {
    pub order: i32,
    pub descriptor: ContractEventDescriptor,
    pub active_in: Option<Hardfork>,
    pub deprecated_in: Option<Hardfork>,
}

impl fmt::Display for ContractEventAttribute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.descriptor.name)
    }
}

impl ContractEventAttribute {
    pub fn new(order: i32, name: String, arg1_name: String, arg1_value: ContractParameterType) -> Self {
        Self {
            order,
            descriptor: ContractEventDescriptor {
                name,
                parameters: vec![
                    ContractParameterDefinition {
                        name: arg1_name,
                        parameter_type: arg1_value,
                    }
                ],
            },
            active_in: None,
            deprecated_in: None,
        }
    }

    pub fn new_with_active(active_in: Hardfork, order: i32, name: String, arg1_name: String, arg1_value: ContractParameterType) -> Self {
        let mut attr = Self::new(order, name, arg1_name, arg1_value);
        attr.active_in = Some(active_in);
        attr
    }

    pub fn new_with_active_and_deprecated(active_in: Hardfork, order: i32, name: String, arg1_name: String, arg1_value: ContractParameterType, deprecated_in: Hardfork) -> Self {
        let mut attr = Self::new_with_active(active_in, order, name, arg1_name, arg1_value);
        attr.deprecated_in = Some(deprecated_in);
        attr
    }

    pub fn new_with_deprecated(order: i32, name: String, arg1_name: String, arg1_value: ContractParameterType, deprecated_in: Hardfork) -> Self {
        let mut attr = Self::new(order, name, arg1_name, arg1_value);
        attr.deprecated_in = Some(deprecated_in);
        attr
    }

    pub fn new_with_two_args(order: i32, name: String, arg1_name: String, arg1_value: ContractParameterType, arg2_name: String, arg2_value: ContractParameterType) -> Self {
        Self {
            order,
            descriptor: ContractEventDescriptor {
                name,
                parameters: vec![
                    ContractParameterDefinition {
                        name: arg1_name,
                        parameter_type: arg1_value,
                    },
                    ContractParameterDefinition {
                        name: arg2_name,
                        parameter_type: arg2_value,
                    }
                ],
            },
            active_in: None,
            deprecated_in: None,
        }
    }

    pub fn new_with_two_args_active(active_in: Hardfork, order: i32, name: String, arg1_name: String, arg1_value: ContractParameterType, arg2_name: String, arg2_value: ContractParameterType) -> Self {
        let mut attr = Self::new_with_two_args(order, name, arg1_name, arg1_value, arg2_name, arg2_value);
        attr.active_in = Some(active_in);
        attr
    }

    pub fn new_with_two_args_active_and_deprecated(active_in: Hardfork, order: i32, name: String, arg1_name: String, arg1_value: ContractParameterType, arg2_name: String, arg2_value: ContractParameterType, deprecated_in: Hardfork) -> Self {
        let mut attr = Self::new_with_two_args_active(active_in, order, name, arg1_name, arg1_value, arg2_name, arg2_value);
        attr.deprecated_in = Some(deprecated_in);
        attr
    }

    pub fn new_with_two_args_deprecated(order: i32, name: String, arg1_name: String, arg1_value: ContractParameterType, arg2_name: String, arg2_value: ContractParameterType, deprecated_in: Hardfork) -> Self {
        let mut attr = Self::new_with_two_args(order, name, arg1_name, arg1_value, arg2_name, arg2_value);
        attr.deprecated_in = Some(deprecated_in);
        attr
    }

    pub fn new_with_three_args(order: i32, name: String, arg1_name: String, arg1_value: ContractParameterType, arg2_name: String, arg2_value: ContractParameterType, arg3_name: String, arg3_value: ContractParameterType) -> Self {
        Self {
            order,
            descriptor: ContractEventDescriptor {
                name,
                parameters: vec![
                    ContractParameterDefinition {
                        name: arg1_name,
                        parameter_type: arg1_value,
                    },
                    ContractParameterDefinition {
                        name: arg2_name,
                        parameter_type: arg2_value,
                    },
                    ContractParameterDefinition {
                        name: arg3_name,
                        parameter_type: arg3_value,
                    }
                ],
            },
            active_in: None,
            deprecated_in: None,
        }
    }

    pub fn new_with_three_args_active(active_in: Hardfork, order: i32, name: String, arg1_name: String, arg1_value: ContractParameterType, arg2_name: String, arg2_value: ContractParameterType, arg3_name: String, arg3_value: ContractParameterType) -> Self {
        let mut attr = Self::new_with_three_args(order, name, arg1_name, arg1_value, arg2_name, arg2_value, arg3_name, arg3_value);
        attr.active_in = Some(active_in);
        attr
    }

    pub fn new_with_four_args(order: i32, name: String, arg1_name: String, arg1_value: ContractParameterType, arg2_name: String, arg2_value: ContractParameterType, arg3_name: String, arg3_value: ContractParameterType, arg4_name: String, arg4_value: ContractParameterType) -> Self {
        Self {
            order,
            descriptor: ContractEventDescriptor {
                name,
                parameters: vec![
                    ContractParameterDefinition {
                        name: arg1_name,
                        parameter_type: arg1_value,
                    },
                    ContractParameterDefinition {
                        name: arg2_name,
                        parameter_type: arg2_value,
                    },
                    ContractParameterDefinition {
                        name: arg3_name,
                        parameter_type: arg3_value,
                    },
                    ContractParameterDefinition {
                        name: arg4_name,
                        parameter_type: arg4_value,
                    }
                ],
            },
            active_in: None,
            deprecated_in: None,
        }
    }

    pub fn new_with_four_args_active(active_in: Hardfork, order: i32, name: String, arg1_name: String, arg1_value: ContractParameterType, arg2_name: String, arg2_value: ContractParameterType, arg3_name: String, arg3_value: ContractParameterType, arg4_name: String, arg4_value: ContractParameterType) -> Self {
        let mut attr = Self::new_with_four_args(order, name, arg1_name, arg1_value, arg2_name, arg2_value, arg3_name, arg3_value, arg4_name, arg4_value);
        attr.active_in = Some(active_in);
        attr
    }
}

impl IHardforkActivable for ContractEventAttribute {
    fn active_in(&self) -> Option<Hardfork> {
        self.active_in
    }

    fn deprecated_in(&self) -> Option<Hardfork> {
        self.deprecated_in
    }
}
