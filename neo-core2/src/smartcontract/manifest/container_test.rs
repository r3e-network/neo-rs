use std::str::FromStr;
use neo_core2::crypto::keys::{PrivateKey, PublicKey};
use neo_core2::types::Uint160;
use neo_core2::smartcontract::manifest::{WildStrings, WildPermissionDescs, PermissionDesc, PermissionType};
use rand::Rng;
use serde_json;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_container_restrict() {
        // String test
        {
            let mut c = WildStrings::default();
            assert!(c.is_wildcard());
            assert!(c.contains("abc"));
            c.restrict();
            assert!(!c.is_wildcard());
            assert!(!c.contains("abc"));
            assert_eq!(c.value().len(), 0);
        }

        // PermissionDesc test
        {
            fn check(u: PermissionDesc) {
                let mut c = WildPermissionDescs::default();
                assert!(!c.is_wildcard());
                assert!(!c.contains(&u));
                c.set_wildcard(true);
                assert!(c.is_wildcard());
                assert!(c.contains(&u));
                c.restrict();
                assert!(!c.is_wildcard());
                assert!(!c.contains(&u));
                assert_eq!(c.value().len(), 0);
            }

            // Hash subtest
            {
                let random_hash = Uint160::random();
                check(PermissionDesc {
                    permission_type: PermissionType::Hash,
                    value: random_hash.into(),
                });
            }

            // Group subtest
            {
                let pk = PrivateKey::new().expect("Failed to generate private key");
                check(PermissionDesc {
                    permission_type: PermissionType::Group,
                    value: pk.public_key().into(),
                });
            }
        }
    }

    #[test]
    fn test_container_add() {
        // String test
        {
            let mut c = WildStrings::default();
            assert_eq!(c.value(), &Vec::<String>::new());

            c.add("abc".to_string());
            assert!(c.contains("abc"));
            assert!(!c.contains("aaa"));
        }

        // Uint160 test
        {
            let mut c = WildPermissionDescs::default();
            assert_eq!(c.value(), &Vec::<PermissionDesc>::new());
            let pk = PrivateKey::new().expect("Failed to generate private key");
            let exp = vec![
                PermissionDesc {
                    permission_type: PermissionType::Hash,
                    value: Uint160::random().into(),
                },
                PermissionDesc {
                    permission_type: PermissionType::Group,
                    value: pk.public_key().into(),
                },
            ];
            for e in &exp {
                c.add(e.clone());
            }
            for e in &exp {
                assert!(c.contains(e));
            }
            let pk_rand = PrivateKey::new().expect("Failed to generate private key");
            assert!(!c.contains(&PermissionDesc {
                permission_type: PermissionType::Hash,
                value: Uint160::random().into(),
            }));
            assert!(!c.contains(&PermissionDesc {
                permission_type: PermissionType::Group,
                value: pk_rand.public_key().into(),
            }));
        }

        // From wildcard test
        {
            let mut c = WildPermissionDescs {
                value: Vec::new(),
                wildcard: true,
            };
            assert!(c.is_wildcard());

            c.add(PermissionDesc {
                permission_type: PermissionType::Hash,
                value: Uint160::random().into(),
            });
            assert!(!c.is_wildcard());
        }
    }

    #[test]
    fn test_container_marshal_json() {
        // String tests
        {
            // Wildcard
            {
                let expected = WildStrings::default();
                let json = serde_json::to_string(&expected).unwrap();
                let deserialized: WildStrings = serde_json::from_str(&json).unwrap();
                assert_eq!(expected, deserialized);
            }

            // Empty
            {
                let mut expected = WildStrings::default();
                expected.restrict();
                let json = serde_json::to_string(&expected).unwrap();
                let deserialized: WildStrings = serde_json::from_str(&json).unwrap();
                assert_eq!(expected, deserialized);
            }

            // Non-empty
            {
                let mut expected = WildStrings::default();
                expected.add("string1".to_string());
                expected.add("string2".to_string());
                let json = serde_json::to_string(&expected).unwrap();
                let deserialized: WildStrings = serde_json::from_str(&json).unwrap();
                assert_eq!(expected, deserialized);
            }

            // Invalid
            {
                let js = "[123]";
                let result: Result<WildStrings, _> = serde_json::from_str(js);
                assert!(result.is_err());
            }
        }

        // PermissionDesc tests
        {
            // Wildcard
            {
                let expected = WildPermissionDescs::default();
                let json = serde_json::to_string(&expected).unwrap();
                let deserialized: WildPermissionDescs = serde_json::from_str(&json).unwrap();
                assert_eq!(expected, deserialized);
            }

            // Empty
            {
                let mut expected = WildPermissionDescs::default();
                expected.restrict();
                let json = serde_json::to_string(&expected).unwrap();
                let deserialized: WildPermissionDescs = serde_json::from_str(&json).unwrap();
                assert_eq!(expected, deserialized);
            }

            // Non-empty
            {
                let mut expected = WildPermissionDescs::default();
                expected.add(PermissionDesc {
                    permission_type: PermissionType::Hash,
                    value: Uint160::random().into(),
                });
                let json = serde_json::to_string(&expected).unwrap();
                let deserialized: WildPermissionDescs = serde_json::from_str(&json).unwrap();
                assert_eq!(expected, deserialized);
            }

            // Invalid
            {
                let js = r#"["notahex"]"#;
                let result: Result<WildPermissionDescs, _> = serde_json::from_str(js);
                assert!(result.is_err());
            }
        }
    }
}
