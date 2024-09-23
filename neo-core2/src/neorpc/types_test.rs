use serde_json;
use std::fmt;
use std::any::Any;
use crate::testserdes;
use crate::transaction;
use crate::keys;
use crate::util;
use crate::require;

#[test]
fn test_signer_with_witness_marshal_unmarshal_json() {
    let s = SignerWithWitness {
        signer: transaction::Signer {
            account: util::Uint160([1, 2, 3]),
            scopes: transaction::CalledByEntry | transaction::CustomContracts,
            allowed_contracts: vec![util::Uint160([1, 2, 3, 4])],
        },
        witness: transaction::Witness {
            invocation_script: vec![1, 2, 3],
            verification_script: vec![4, 5, 6],
        },
    };
    testserdes::marshal_unmarshal_json(&s, &SignerWithWitness::default());

    // Check marshalling separately to ensure Scopes are marshalled OK.
    let expected = r#"{"account":"0xcadb3dc2faa3ef14a13b619c9a43124755aa2569","scopes":"CalledByEntry, CustomContracts"}"#;
    let acc = util::Uint160::decode_string_le("cadb3dc2faa3ef14a13b619c9a43124755aa2569").unwrap();
    let s = SignerWithWitness {
        signer: transaction::Signer {
            account: acc,
            scopes: transaction::CalledByEntry | transaction::CustomContracts,
        },
        witness: transaction::Witness::default(),
    };
    let actual = serde_json::to_string(&s).unwrap();
    require::equal(expected, &actual);

    fn check_subitems(t: &str, bad: &dyn Any) {
        let data = serde_json::to_string(bad).unwrap();
        let result: Result<SignerWithWitness, _> = serde_json::from_str(&data);
        require::error(result.is_err());
        require::contains(&result.unwrap_err().to_string(), &format!("got {}, allowed {} at max", transaction::MAX_ATTRIBUTES + 1, transaction::MAX_ATTRIBUTES));
    }

    #[test]
    fn subitems_overflow() {
        #[test]
        fn groups() {
            let pk = keys::PrivateKey::new().unwrap();
            let mut bad = SignerWithWitness {
                signer: transaction::Signer {
                    allowed_groups: vec![None; transaction::MAX_ATTRIBUTES + 1],
                    ..Default::default()
                },
                ..Default::default()
            };
            for i in 0..bad.signer.allowed_groups.len() {
                bad.signer.allowed_groups[i] = Some(pk.public_key());
            }
            check_subitems("groups", &bad);
        }

        #[test]
        fn contracts() {
            let bad = SignerWithWitness {
                signer: transaction::Signer {
                    allowed_contracts: vec![util::Uint160::default(); transaction::MAX_ATTRIBUTES + 1],
                    ..Default::default()
                },
                ..Default::default()
            };
            check_subitems("contracts", &bad);
        }

        #[test]
        fn rules() {
            let mut bad = SignerWithWitness {
                signer: transaction::Signer {
                    rules: vec![transaction::WitnessRule::default(); transaction::MAX_ATTRIBUTES + 1],
                    ..Default::default()
                },
                ..Default::default()
            };
            for i in 0..bad.signer.rules.len() {
                bad.signer.rules[i] = transaction::WitnessRule {
                    action: transaction::WitnessAllow,
                    condition: transaction::Condition::ScriptHash(util::Uint160::default()),
                };
            }
            check_subitems("rules", &bad);
        }
    }
}
