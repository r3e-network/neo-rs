use crate::smartcontract;
use crate::smartcontract::manifest;

/// DecimalTokenBase contains methods common to NEP-11 and NEP-17 token standards.
pub static DECIMAL_TOKEN_BASE: &Standard = &Standard {
    manifest: manifest::Manifest {
        abi: manifest::ABI {
            methods: vec![
                manifest::Method {
                    name: "decimals".to_string(),
                    return_type: smartcontract::Type::Integer,
                    safe: true,
                },
                manifest::Method {
                    name: "symbol".to_string(),
                    return_type: smartcontract::Type::String,
                    safe: true,
                },
                manifest::Method {
                    name: "totalSupply".to_string(),
                    return_type: smartcontract::Type::Integer,
                    safe: true,
                },
            ],
        },
    },
};
