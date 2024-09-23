use crate::smartcontract;
use crate::smartcontract::manifest;

/// Nep11Payable contains NEP-11's onNEP11Payment method definition.
pub static NEP11_PAYABLE: &Standard = &Standard {
    manifest: manifest::Manifest {
        abi: manifest::ABI {
            methods: vec![manifest::Method {
                name: manifest::METHOD_ON_NEP11_PAYMENT.to_string(),
                parameters: vec![
                    manifest::Parameter { name: "from".to_string(), type_: smartcontract::Hash160Type },
                    manifest::Parameter { name: "amount".to_string(), type_: smartcontract::IntegerType },
                    manifest::Parameter { name: "tokenid".to_string(), type_: smartcontract::ByteArrayType },
                    manifest::Parameter { name: "data".to_string(), type_: smartcontract::AnyType },
                ],
                return_type: smartcontract::VoidType,
            }],
        },
    },
};

/// Nep17Payable contains NEP-17's onNEP17Payment method definition.
pub static NEP17_PAYABLE: &Standard = &Standard {
    manifest: manifest::Manifest {
        abi: manifest::ABI {
            methods: vec![manifest::Method {
                name: manifest::METHOD_ON_NEP17_PAYMENT.to_string(),
                parameters: vec![
                    manifest::Parameter { name: "from".to_string(), type_: smartcontract::Hash160Type },
                    manifest::Parameter { name: "amount".to_string(), type_: smartcontract::IntegerType },
                    manifest::Parameter { name: "data".to_string(), type_: smartcontract::AnyType },
                ],
                return_type: smartcontract::VoidType,
            }],
        },
    },
};
