use std::collections::HashMap;
use std::str::FromStr;

use neo_crypto::keys::{PublicKey, Secp256r1PrivateKey};
use neo_types::{ContractParameter, ContractParameterType, Hash160};
use serde_json::json;

use crate::smartcontract::manifest::{ABI, Event, Group, Manifest, Method, Parameter, Permission, PermissionDesc};
use crate::smartcontract::MethodToken;

#[test]
fn test_manifest_marshal_json() {
    // Test vectors are taken from the main NEO repo
    // https://github.com/neo-project/neo/blob/master/tests/neo.UnitTests/SmartContract/Manifest/UT_ContractManifest.cs#L10

    #[test]
    fn default() {
        let s = r#"{"groups":[],"features":{},"supportedstandards":[],"name":"Test","abi":{"methods":[],"events":[]},"permissions":[{"contract":"*","methods":"*"}],"trusts":[],"extra":null}"#;
        let m = test_unmarshal_marshal_manifest(s);
        assert_eq!(Manifest::default("Test"), m);
    }

    #[test]
    fn permissions() {
        let s = r#"{"groups":[],"features":{},"supportedstandards":[],"name":"Test","abi":{"methods":[],"events":[]},"permissions":[{"contract":"0x0000000000000000000000000000000000000000","methods":["method1","method2"]}],"trusts":[],"extra":null}"#;
        test_unmarshal_marshal_manifest(s);
    }

    #[test]
    fn safe_methods() {
        let s = r#"{"groups":[],"features":{},"supportedstandards":[],"name":"Test","abi":{"methods":[{"name":"safeMet","offset":123,"parameters":[],"returntype":"Integer","safe":true}],"events":[]},"permissions":[{"contract":"*","methods":"*"}],"trusts":[],"extra":null}"#;
        test_unmarshal_marshal_manifest(s);
    }

    #[test]
    fn trust() {
        let s = r#"{"groups":[],"features":{},"supportedstandards":[],"name":"Test","abi":{"methods":[],"events":[]},"permissions":[{"contract":"*","methods":"*"}],"trusts":["0x0000000000000000000000000000000000000001"],"extra":null}"#;
        test_unmarshal_marshal_manifest(s);
    }

    #[test]
    fn groups() {
        let s = r#"{"groups":[{"pubkey":"03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c","signature":"QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQQ=="}],"features":{},"supportedstandards":[],"name":"Test","abi":{"methods":[],"events":[]},"permissions":[{"contract":"*","methods":"*"}],"trusts":[],"extra":null}"#;
        test_unmarshal_marshal_manifest(s);
    }

    #[test]
    fn extra() {
        let s = r#"{"groups":[],"features":{},"supportedstandards":[],"name":"Test","abi":{"methods":[],"events":[]},"permissions":[{"contract":"*","methods":"*"}],"trusts":[],"extra":{"key":"value"}}"#;
        test_unmarshal_marshal_manifest(s);
    }
}

fn test_unmarshal_marshal_manifest(s: &str) -> Manifest {
    let c: Manifest = serde_json::from_str(s).unwrap();
    let data = serde_json::to_string(&c).unwrap();
    assert_eq!(s, data);
    c
}

#[test]
fn test_manifest_can_call() {
    let man1 = Manifest::default("Test1");
    let man2 = Manifest::default("Test2");
    assert!(man1.can_call(&Hash160::default(), &man2, "method1"));
}

#[test]
fn test_permission_is_allowed() {
    let manifest = Manifest::default("Test");

    #[test]
    fn wildcard() {
        let h = Hash160::random();

        let mut perm = Permission::new_wildcard();
        assert!(perm.is_allowed(&h, &manifest, "AAA"));

        perm.methods.restrict();
        assert!(!perm.is_allowed(&h, &manifest, "AAA"));

        perm.methods.add("AAA");
        assert!(perm.is_allowed(&h, &manifest, "AAA"));
    }

    #[test]
    fn hash() {
        let mut perm = Permission::new_hash(Hash160::default());
        assert!(perm.is_allowed(&Hash160::default(), &manifest, "AAA"));

        #[test]
        fn restrict_methods() {
            perm.methods.restrict();
            assert!(!perm.is_allowed(&Hash160::default(), &manifest, "AAA"));
            perm.methods.add("AAA");
            assert!(perm.is_allowed(&Hash160::default(), &manifest, "AAA"));
        }
    }

    #[test]
    fn invalid_hash() {
        let perm = Permission::new_hash(Hash160::from_str("0100000000000000000000000000000000000000").unwrap());
        assert!(!perm.is_allowed(&Hash160::default(), &manifest, "AAA"));
    }

    let priv_key = Secp256r1PrivateKey::new().unwrap();
    manifest.groups = vec![Group {
        public_key: priv_key.public_key(),
        signature: vec![],
    }];

    #[test]
    fn group() {
        let perm = Permission::new_group(priv_key.public_key());
        assert!(perm.is_allowed(&Hash160::default(), &manifest, "AAA"));

        let priv_key2 = Secp256r1PrivateKey::new().unwrap();

        let perm = Permission::new_group(priv_key2.public_key());
        assert!(!perm.is_allowed(&Hash160::default(), &manifest, "AAA"));

        manifest.groups.push(Group {
            public_key: priv_key2.public_key(),
            signature: vec![],
        });
        let perm = Permission::new_group(priv_key2.public_key());
        assert!(perm.is_allowed(&Hash160::default(), &manifest, "AAA"));
    }
}

// ... (remaining tests follow a similar pattern of conversion)
