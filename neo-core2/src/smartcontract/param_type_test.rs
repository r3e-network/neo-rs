use std::str::FromStr;
use num_bigint::BigInt;
use neo_core2::smartcontract::ParamType;
use neo_core2::util::{Uint160, Uint256};
use neo_core2::vm::stackitem::{Item, Type as StackItemType};

#[test]
fn test_parse_param_type() {
    let inouts = vec![
        ("signature", Ok(ParamType::Signature)),
        ("Signature", Ok(ParamType::Signature)),
        ("SiGnAtUrE", Ok(ParamType::Signature)),
        ("bool", Ok(ParamType::Bool)),
        ("int", Ok(ParamType::Integer)),
        ("hash160", Ok(ParamType::Hash160)),
        ("hash256", Ok(ParamType::Hash256)),
        ("bytes", Ok(ParamType::ByteArray)),
        ("key", Ok(ParamType::PublicKey)),
        ("string", Ok(ParamType::String)),
        ("array", Ok(ParamType::Array)),
        ("map", Ok(ParamType::Map)),
        ("interopinterface", Ok(ParamType::InteropInterface)),
        ("void", Ok(ParamType::Void)),
        ("qwerty", Err(())),
        ("filebytes", Ok(ParamType::ByteArray)),
    ];

    for (input, expected) in inouts {
        let result = ParamType::from_str(input);
        assert_eq!(result, expected, "Unexpected result for input '{}'", input);
    }
}

#[test]
fn test_infer_param_type() {
    let bi = BigInt::from(1) << (stackitem::MAX_BIG_INTEGER_SIZE_BITS - 2);
    let inouts = vec![
        ("42", ParamType::Integer),
        ("-42", ParamType::Integer),
        ("0", ParamType::Integer),
        ("8765432187654321111", ParamType::Integer),
        (&bi.to_string(), ParamType::Integer),
        (&(bi.to_string() + "0"), ParamType::ByteArray),
        ("2e10", ParamType::ByteArray),
        ("true", ParamType::Bool),
        ("false", ParamType::Bool),
        ("truee", ParamType::String),
        ("NPAsqZkx9WhNd4P72uhZxBhLinSuNkxfB8", ParamType::Hash160),
        ("ZK2nJJpJr6o664CWJKi1QRXjqeic2zRp8y", ParamType::String),
        ("50befd26fdf6e4d957c11e078b24ebce6291456f", ParamType::Hash160),
        ("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c", ParamType::PublicKey),
        ("30b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c", ParamType::ByteArray),
        ("602c79718b16e442de58778e148d0b1084e3b2dffd5de6b7b16cee7969282de7", ParamType::Hash256),
        ("602c79718b16e442de58778e148d0b1084e3b2dffd5de6b7b16cee7969282de7da", ParamType::ByteArray),
        ("602c79718b16e442de58778e148d0b1084e3b2dffd5de6b7b16cee7969282de7c56f33fc6ecfcd0c225c4ab356fee59390af8560be0e930faebe74a6daff7c9b", ParamType::Signature),
        ("qwerty", ParamType::String),
        ("ab", ParamType::ByteArray),
        ("az", ParamType::String),
        ("bad", ParamType::String),
        ("фыва", ParamType::String),
        ("dead", ParamType::ByteArray),
        ("nil", ParamType::Any),
    ];

    for (input, expected) in inouts {
        let result = infer_param_type(input);
        assert_eq!(result, expected, "Unexpected result for input '{}'", input);
    }
}

// Additional test functions would be implemented similarly...

fn infer_param_type(input: &str) -> ParamType {
    // Implementation of infer_param_type would go here
    unimplemented!()
}
