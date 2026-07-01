use super::*;

#[test]
fn test_contract_parameter_type_values() {
    assert_eq!(ContractParameterType::Any as u8, 0x00);
    assert_eq!(ContractParameterType::Boolean as u8, 0x10);
    assert_eq!(ContractParameterType::Integer as u8, 0x11);
    assert_eq!(ContractParameterType::ByteArray as u8, 0x12);
    assert_eq!(ContractParameterType::String as u8, 0x13);
    assert_eq!(ContractParameterType::Hash160 as u8, 0x14);
    assert_eq!(ContractParameterType::Hash256 as u8, 0x15);
    assert_eq!(ContractParameterType::PublicKey as u8, 0x16);
    assert_eq!(ContractParameterType::Signature as u8, 0x17);
    assert_eq!(ContractParameterType::Array as u8, 0x20);
    assert_eq!(ContractParameterType::Map as u8, 0x22);
    assert_eq!(ContractParameterType::InteropInterface as u8, 0x30);
    assert_eq!(ContractParameterType::Void as u8, 0xff);
}

#[test]
fn test_contract_parameter_type_as_str() {
    assert_eq!(ContractParameterType::Any.as_str(), "Any");
    assert_eq!(ContractParameterType::Boolean.as_str(), "Boolean");
    assert_eq!(ContractParameterType::Hash160.as_str(), "Hash160");
}

#[test]
fn test_contract_parameter_type_all_values() {
    assert_eq!(ContractParameterType::COUNT, 13);
    assert_eq!(ContractParameterType::all()[0], ContractParameterType::Any);
    assert_eq!(
        ContractParameterType::all()[12],
        ContractParameterType::Void
    );
    assert_eq!(ContractParameterType::ALL, ContractParameterType::all());
}

#[test]
fn test_contract_parameter_type_from_string() {
    assert_eq!(
        ContractParameterType::from_string("Boolean").unwrap(),
        ContractParameterType::Boolean
    );
    assert_eq!(
        ContractParameterType::from_string("bool").unwrap(),
        ContractParameterType::Boolean
    );
    assert_eq!(
        ContractParameterType::from_string("INT").unwrap(),
        ContractParameterType::Integer
    );
    assert_eq!(
        ContractParameterType::from_string("bytes").unwrap(),
        ContractParameterType::ByteArray
    );
    assert!(ContractParameterType::from_string("Invalid").is_err());
}

#[test]
fn test_contract_parameter_type_try_from_u8() {
    assert_eq!(
        ContractParameterType::try_from_u8(0x10),
        Some(ContractParameterType::Boolean)
    );
    assert_eq!(ContractParameterType::try_from_u8(0x99), None);
}

#[test]
fn test_contract_parameter_type_serde() {
    let param_type = ContractParameterType::Hash160;
    let json = serde_json::to_string(&param_type).unwrap();
    assert_eq!(json, "\"Hash160\"");

    let parsed: ContractParameterType = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, ContractParameterType::Hash160);

    let alias: ContractParameterType = serde_json::from_str("\"int\"").unwrap();
    assert_eq!(alias, ContractParameterType::Integer);
}

#[test]
fn test_contract_parameter_type_default() {
    assert_eq!(ContractParameterType::default(), ContractParameterType::Any);
}
