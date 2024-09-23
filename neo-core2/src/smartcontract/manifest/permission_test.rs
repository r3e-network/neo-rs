use std::str::FromStr;
use neo_core2::crypto::keys::{PrivateKey, PublicKey};
use neo_core2::util::Uint160;
use neo_core2::vm::stackitem::{StackItem, Struct, Array, ByteArray, Null, Bool};
use neo_core2::smartcontract::manifest::{Permission, PermissionDesc, PermissionType, WildStrings, Permissions};
use rand::Rng;

#[test]
fn test_new_permission() {
    assert!(std::panic::catch_unwind(|| Permission::new(PermissionType::Wildcard, Uint160::default())).is_err());
    assert!(std::panic::catch_unwind(|| Permission::new(PermissionType::Hash)).is_err());
    assert!(std::panic::catch_unwind(|| Permission::new(PermissionType::Hash, 1)).is_err());
    assert!(std::panic::catch_unwind(|| Permission::new(PermissionType::Group)).is_err());
    assert!(std::panic::catch_unwind(|| Permission::new(PermissionType::Group, Uint160::default())).is_err());
}

#[test]
fn test_permission_is_valid() {
    let mut p = Permission::default();
    assert!(p.is_valid().is_ok());

    p.methods.add("");
    assert!(p.is_valid().is_err());

    p.methods.value = None;
    p.methods.add("qwerty");
    assert!(p.is_valid().is_ok());

    p.methods.add("poiuyt");
    assert!(p.is_valid().is_ok());

    p.methods.add("qwerty");
    assert!(p.is_valid().is_err());
}

#[test]
fn test_permissions_are_valid() {
    let mut p = Permissions::new();
    assert!(p.are_valid().is_ok());

    p.push(Permission { methods: WildStrings { value: Some(vec!["".to_string()]) }, ..Default::default() });
    assert!(p.are_valid().is_err());

    p.clear();
    p.push(Permission::new(PermissionType::Hash, Uint160::from_str("0102030000000000000000000000000000000000").unwrap()));
    assert!(p.are_valid().is_ok());

    let priv0 = PrivateKey::new().unwrap();
    let priv1 = PrivateKey::new().unwrap();

    p.push(Permission::new(PermissionType::Group, priv0.public_key()));
    assert!(p.are_valid().is_ok());

    p.push(Permission::new(PermissionType::Group, priv1.public_key()));
    assert!(p.are_valid().is_ok());

    p.push(Permission::new(PermissionType::Wildcard));
    assert!(p.are_valid().is_ok());

    p.push(Permission::new(PermissionType::Hash, Uint160::from_str("0302010000000000000000000000000000000000").unwrap()));
    assert!(p.are_valid().is_ok());

    p.push(Permission::new(PermissionType::Wildcard));
    assert!(p.are_valid().is_err());

    p.pop();
    p.push(Permission::new(PermissionType::Hash, Uint160::from_str("0102030000000000000000000000000000000000").unwrap()));
    assert!(p.are_valid().is_err());

    p.pop();
    p.push(Permission::new(PermissionType::Group, priv0.public_key()));
    assert!(p.are_valid().is_err());
}

#[test]
fn test_permission_serialize_deserialize() {
    // Wildcard
    let expected = {
        let mut p = Permission::new(PermissionType::Wildcard);
        p.methods.restrict();
        p
    };
    test_serialize_deserialize(&expected, Permission::new(PermissionType::Wildcard));

    // Group
    let expected = {
        let mut p = Permission::new(PermissionType::Wildcard);
        p.contract.permission_type = PermissionType::Group;
        let priv_key = PrivateKey::new().unwrap();
        p.contract.value = PermissionDesc::PublicKey(priv_key.public_key());
        p.methods.add("method1");
        p.methods.add("method2");
        p
    };
    test_serialize_deserialize(&expected, Permission::new(PermissionType::Wildcard));

    // Hash
    let expected = {
        let mut p = Permission::new(PermissionType::Wildcard);
        p.contract.permission_type = PermissionType::Hash;
        p.contract.value = PermissionDesc::Hash(Uint160::random());
        p
    };
    test_serialize_deserialize(&expected, Permission::new(PermissionType::Wildcard));
}

#[test]
fn test_permission_desc_serialize_deserialize() {
    // Uint160 with 0x
    let u = Uint160::random();
    let s = format!("0x{}", u.to_string());
    let d: PermissionDesc = serde_json::from_str(&format!("\"{}\"", s)).unwrap();
    assert_eq!(u, d.into_hash().unwrap());

    // Invalid Uint160
    let s = rand::thread_rng().gen::<[u8; 20]>();
    let s = hex::encode(s);
    assert!(serde_json::from_str::<PermissionDesc>(&format!("\"ok{}\"", s)).is_err());
    assert!(serde_json::from_str::<PermissionDesc>(&format!("\"{}\"", s)).is_err());

    // Invalid public key
    let s = rand::thread_rng().gen::<[u8; 65]>();
    let s = format!("k{}", hex::encode(s)); // not a hex
    assert!(serde_json::from_str::<PermissionDesc>(&format!("\"{}\"", s)).is_err());

    // Not a string
    assert!(serde_json::from_str::<PermissionDesc>("123").is_err());

    // Invalid string
    assert!(serde_json::from_str::<PermissionDesc>("\"invalid length\"").is_err());
}

fn test_serialize_deserialize<T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug>(expected: &T, actual: T) {
    let data = serde_json::to_string(expected).unwrap();
    let deserialized: T = serde_json::from_str(&data).unwrap();
    assert_eq!(*expected, deserialized);
}

#[test]
fn test_permission_to_from_stack_item() {
    // Wildcard
    let p = Permission::new(PermissionType::Wildcard);
    let expected = Struct(vec![Null, Null]);
    check_to_from_stack_item(&p, &expected);

    // Hash
    let p = {
        let mut p = Permission::new(PermissionType::Hash, Uint160::from_str("0102030000000000000000000000000000000000").unwrap());
        p.methods = WildStrings { value: Some(vec!["a".to_string()]) };
        p
    };
    let expected = Struct(vec![
        ByteArray(Uint160::from_str("0102030000000000000000000000000000000000").unwrap().to_be_bytes().to_vec()),
        Array(vec![ByteArray(b"a".to_vec())]),
    ]);
    check_to_from_stack_item(&p, &expected);

    // Group
    let pk = PrivateKey::new().unwrap();
    let p = Permission::new(PermissionType::Group, pk.public_key());
    let expected = Struct(vec![
        ByteArray(pk.public_key().to_bytes()),
        Null,
    ]);
    check_to_from_stack_item(&p, &expected);
}

fn check_to_from_stack_item(source: &Permission, expected: &StackItem) {
    let actual = source.to_stack_item();
    assert_eq!(&actual, expected);
    let mut actual_source = Permission::default();
    actual_source.from_stack_item(&actual).unwrap();
    assert_eq!(source, &actual_source);
}

#[test]
fn test_permission_from_stack_item_errors() {
    let err_cases = vec![
        ("not a struct", Array(vec![])),
        ("invalid length", Struct(vec![])),
        ("invalid contract type", Struct(vec![Array(vec![]), Bool(false)])),
        ("invalid contract length", Struct(vec![ByteArray(vec![1, 2, 3]), Bool(false)])),
        ("invalid contract pubkey", Struct(vec![ByteArray(vec![0; 33]), Bool(false)])),
        ("invalid methods type", Struct(vec![Null, Bool(false)])),
        ("invalid method name", Struct(vec![Null, Array(vec![Array(vec![])])])),
    ];

    for (name, err_case) in err_cases {
        let mut p = Permission::default();
        assert!(p.from_stack_item(&err_case).is_err(), "{}", name);
    }
}
