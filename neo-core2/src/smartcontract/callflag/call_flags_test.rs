use crate::smartcontract::callflag::*;
use std::str::FromStr;
use serde_yaml;

#[test]
fn test_call_flag_has() {
    assert!(CallFlag::AllowCall.has(CallFlag::AllowCall));
    assert!((CallFlag::AllowCall | CallFlag::AllowNotify).has(CallFlag::AllowCall));
    assert!(!(CallFlag::AllowCall).has(CallFlag::AllowCall | CallFlag::AllowNotify));
    assert!(CallFlag::All.has(CallFlag::ReadOnly));
}

#[test]
fn test_call_flag_string() {
    let cases = vec![
        (CallFlag::NoneFlag, "None"),
        (CallFlag::All, "All"),
        (CallFlag::ReadStates, "ReadStates"),
        (CallFlag::States, "States"),
        (CallFlag::ReadOnly, "ReadOnly"),
        (CallFlag::States | CallFlag::AllowCall, "ReadOnly, WriteStates"),
        (CallFlag::ReadOnly | CallFlag::AllowNotify, "ReadOnly, AllowNotify"),
        (CallFlag::States | CallFlag::AllowNotify, "States, AllowNotify"),
    ];
    for (flag, expected) in cases {
        assert_eq!(expected, flag.to_string());
    }
}

#[test]
fn test_from_string() {
    let cases = vec![
        ("None", Ok(CallFlag::NoneFlag)),
        ("All", Ok(CallFlag::All)),
        ("ReadStates", Ok(CallFlag::ReadStates)),
        ("States", Ok(CallFlag::States)),
        ("ReadOnly", Ok(CallFlag::ReadOnly)),
        ("ReadOnly, WriteStates", Ok(CallFlag::States | CallFlag::AllowCall)),
        ("States, AllowCall", Ok(CallFlag::States | CallFlag::AllowCall)),
        ("AllowCall, States", Ok(CallFlag::States | CallFlag::AllowCall)),
        ("States, ReadOnly", Ok(CallFlag::States | CallFlag::AllowCall)),
        (" AllowCall,AllowNotify", Ok(CallFlag::AllowNotify | CallFlag::AllowCall)),
        ("BlahBlah", Err(())),
        ("States, All", Err(())),
        ("ReadStates,,AllowCall", Err(())),
        ("ReadStates;AllowCall", Err(())),
        ("readstates", Err(())),
        ("  All", Err(())),
        ("None, All", Err(())),
    ];
    for (input, expected) in cases {
        let result = CallFlag::from_str(input);
        assert_eq!(expected.is_ok(), result.is_ok(), "Input: '{}'", input);
        if let Ok(flag) = result {
            assert_eq!(expected.unwrap(), flag);
        }
    }
}

#[test]
fn test_serialize_deserialize_json() {
    let f = CallFlag::States;
    let serialized = serde_json::to_string(&f).unwrap();
    let deserialized: CallFlag = serde_json::from_str(&serialized).unwrap();
    assert_eq!(f, deserialized);

    let f = CallFlag::States | CallFlag::AllowNotify;
    let serialized = serde_json::to_string(&f).unwrap();
    let deserialized: CallFlag = serde_json::from_str(&serialized).unwrap();
    assert_eq!(f, deserialized);

    assert!(serde_json::from_str::<CallFlag>("42").is_err());
    assert!(serde_json::from_str::<CallFlag>("\"State\"").is_err());
}

#[test]
fn test_serialize_deserialize_yaml() {
    let cases = vec![CallFlag::States, CallFlag::States | CallFlag::AllowNotify];
    for expected in cases {
        let serialized = serde_yaml::to_string(&expected).unwrap();
        let deserialized: CallFlag = serde_yaml::from_str(&serialized).unwrap();
        assert_eq!(expected, deserialized);
    }

    assert!(serde_yaml::from_str::<CallFlag>("[]").is_err());
}
