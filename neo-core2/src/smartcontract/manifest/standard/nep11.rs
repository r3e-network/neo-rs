use crate::smartcontract::{self, manifest};

// Nep11Base is a Standard containing common NEP-11 methods.
pub static NEP11_BASE: Standard = Standard {
    base: &DECIMAL_TOKEN_BASE,
    manifest: manifest::Manifest {
        abi: manifest::ABI {
            methods: vec![
                manifest::Method {
                    name: "balanceOf".to_string(),
                    parameters: vec![
                        manifest::Parameter {
                            name: "owner".to_string(),
                            type_: smartcontract::Hash160Type,
                        },
                    ],
                    return_type: smartcontract::IntegerType,
                    safe: true,
                },
                manifest::Method {
                    name: "tokensOf".to_string(),
                    parameters: vec![
                        manifest::Parameter {
                            name: "owner".to_string(),
                            type_: smartcontract::Hash160Type,
                        },
                    ],
                    return_type: smartcontract::InteropInterfaceType,
                    safe: true,
                },
                manifest::Method {
                    name: "transfer".to_string(),
                    parameters: vec![
                        manifest::Parameter {
                            name: "to".to_string(),
                            type_: smartcontract::Hash160Type,
                        },
                        manifest::Parameter {
                            name: "tokenId".to_string(),
                            type_: smartcontract::ByteArrayType,
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
                        manifest::Parameter {
                            name: "tokenId".to_string(),
                            type_: smartcontract::ByteArrayType,
                        },
                    ],
                },
            ],
        },
    },
    optional: vec![
        manifest::Method {
            name: "properties".to_string(),
            parameters: vec![
                manifest::Parameter {
                    name: "tokenId".to_string(),
                    type_: smartcontract::ByteArrayType,
                },
            ],
            return_type: smartcontract::MapType,
            safe: true,
        },
        manifest::Method {
            name: "tokens".to_string(),
            parameters: vec![],
            return_type: smartcontract::InteropInterfaceType,
            safe: true,
        },
    ],
};

// Nep11NonDivisible is a NEP-11 non-divisible Standard.
pub static NEP11_NON_DIVISIBLE: Standard = Standard {
    base: &NEP11_BASE,
    manifest: manifest::Manifest {
        abi: manifest::ABI {
            methods: vec![
                manifest::Method {
                    name: "ownerOf".to_string(),
                    parameters: vec![
                        manifest::Parameter {
                            name: "tokenId".to_string(),
                            type_: smartcontract::ByteArrayType,
                        },
                    ],
                    return_type: smartcontract::Hash160Type,
                    safe: true,
                },
            ],
            events: vec![],
        },
    },
    optional: vec![],
};

// Nep11Divisible is a NEP-11 divisible Standard.
pub static NEP11_DIVISIBLE: Standard = Standard {
    base: &NEP11_BASE,
    manifest: manifest::Manifest {
        abi: manifest::ABI {
            methods: vec![
                manifest::Method {
                    name: "balanceOf".to_string(),
                    parameters: vec![
                        manifest::Parameter {
                            name: "owner".to_string(),
                            type_: smartcontract::Hash160Type,
                        },
                        manifest::Parameter {
                            name: "tokenId".to_string(),
                            type_: smartcontract::ByteArrayType,
                        },
                    ],
                    return_type: smartcontract::IntegerType,
                    safe: true,
                },
                manifest::Method {
                    name: "ownerOf".to_string(),
                    parameters: vec![
                        manifest::Parameter {
                            name: "tokenId".to_string(),
                            type_: smartcontract::ByteArrayType,
                        },
                    ],
                    return_type: smartcontract::InteropInterfaceType, // iterator
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
                            name: "tokenId".to_string(),
                            type_: smartcontract::ByteArrayType,
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
            events: vec![],
        },
    },
    optional: vec![],
};
