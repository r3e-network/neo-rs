use crate::smartcontract;
use crate::smartcontract::manifest;

/// Nep17 is a NEP-17 Standard.
pub static NEP17: &Standard = &Standard {
    base: DECIMAL_TOKEN_BASE,
    manifest: manifest::Manifest {
        abi: manifest::ABI {
            methods: vec![
                manifest::Method {
                    name: "balanceOf".to_string(),
                    parameters: vec![
                        manifest::Parameter {
                            name: "account".to_string(),
                            type_: smartcontract::Hash160Type,
                        },
                    ],
                    return_type: smartcontract::IntegerType,
                    safe: true,
                },
                manifest::Method {
                    name: "transfer".to_string(),
                    parameters: vec![
                        manifest::Parameter {
                            name: "from".to_string(),
                            type_: smartcontract::Hash160Type,
                        },
                        manifest::Parameter {
                            name: "to".to_string(),
                            type_: smartcontract::Hash160Type,
                        },
                        manifest::Parameter {
                            name: "amount".to_string(),
                            type_: smartcontract::IntegerType,
                        },
                        manifest::Parameter {
                            name: "data".to_string(),
                            type_: smartcontract::AnyType,
                        },
                    ],
                    return_type: smartcontract::BoolType,
                    safe: false,
                },
            ],
            events: vec![
                manifest::Event {
                    name: "Transfer".to_string(),
                    parameters: vec![
                        manifest::Parameter {
                            name: "from".to_string(),
                            type_: smartcontract::Hash160Type,
                        },
                        manifest::Parameter {
                            name: "to".to_string(),
                            type_: smartcontract::Hash160Type,
                        },
                        manifest::Parameter {
                            name: "amount".to_string(),
                            type_: smartcontract::IntegerType,
                        },
                    ],
                },
            ],
        },
    },
};
