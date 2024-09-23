use neo_rs::transaction::{Transaction, Attribute, NotaryAssisted, Signer, Witness};
use neo_rs::crypto::keys;
use neo_rs::util;
use neo_rs::vm::opcode;
use proptest::prelude::*;
use std::collections::HashMap;

#[test]
fn test_notary_request_is_valid() {
    let main_tx = Transaction {
        attributes: vec![Attribute::NotaryAssisted(NotaryAssisted { n_keys: 1 })],
        script: vec![0, 1, 2],
        valid_until_block: 123,
        ..Default::default()
    };
    let empty_single_invocation = {
        let mut script = vec![opcode::PUSHDATA1 as u8, keys::SIGNATURE_LEN as u8];
        script.extend(vec![0; keys::SIGNATURE_LEN]);
        script
    };
    let error_cases: HashMap<&str, P2PNotaryRequest> = [
        ("main tx: missing NotaryAssisted attribute", P2PNotaryRequest {
            main_transaction: Transaction::default(),
            ..Default::default()
        }),
        ("main tx: zero NKeys", P2PNotaryRequest {
            main_transaction: Transaction {
                attributes: vec![Attribute::NotaryAssisted(NotaryAssisted { n_keys: 0 })],
                ..Default::default()
            },
            ..Default::default()
        }),
        ("fallback transaction: invalid signers count", P2PNotaryRequest {
            main_transaction: main_tx.clone(),
            fallback_transaction: Transaction {
                signers: vec![Signer { account: util::random_uint160() }],
                ..Default::default()
            },
            ..Default::default()
        }),
        ("fallback transaction: invalid witnesses count", P2PNotaryRequest {
            main_transaction: main_tx.clone(),
            fallback_transaction: Transaction {
                signers: vec![Signer { account: util::random_uint160() }],
                ..Default::default()
            },
            ..Default::default()
        }),
        ("fallback tx: invalid dummy Notary witness (bad witnesses length)", P2PNotaryRequest {
            main_transaction: main_tx.clone(),
            fallback_transaction: Transaction {
                signers: vec![Signer { account: util::random_uint160() }, Signer { account: util::random_uint160() }],
                scripts: vec![Witness::default()],
                ..Default::default()
            },
            ..Default::default()
        }),
        ("fallback tx: invalid dummy Notary witness (bad invocation script length)", P2PNotaryRequest {
            main_transaction: main_tx.clone(),
            fallback_transaction: Transaction {
                signers: vec![Signer { account: util::random_uint160() }, Signer { account: util::random_uint160() }],
                scripts: vec![Witness::default(), Witness::default()],
                ..Default::default()
            },
            ..Default::default()
        }),
        ("fallback tx: invalid dummy Notary witness (bad invocation script prefix)", P2PNotaryRequest {
            main_transaction: main_tx.clone(),
            fallback_transaction: Transaction {
                signers: vec![Signer { account: util::random_uint160() }, Signer { account: util::random_uint160() }],
                scripts: vec![Witness {
                    invocation_script: {
                        let mut script = vec![opcode::PUSHDATA1 as u8, 65];
                        script.extend(vec![0; keys::SIGNATURE_LEN]);
                        script
                    },
                    ..Default::default()
                }, Witness::default()],
                ..Default::default()
            },
            ..Default::default()
        }),
        ("fallback tx: invalid dummy Notary witness (non-empty verification script))", P2PNotaryRequest {
            main_transaction: main_tx.clone(),
            fallback_transaction: Transaction {
                signers: vec![Signer { account: util::random_uint160() }, Signer { account: util::random_uint160() }],
                scripts: vec![Witness {
                    invocation_script: empty_single_invocation.clone(),
                    verification_script: vec![1],
                }, Witness::default()],
                ..Default::default()
            },
            ..Default::default()
        }),
        ("fallback tx: missing NotValidBefore attribute", P2PNotaryRequest {
            main_transaction: main_tx.clone(),
            fallback_transaction: Transaction {
                signers: vec![Signer { account: util::random_uint160() }, Signer { account: util::random_uint160() }],
                scripts: vec![Witness {
                    invocation_script: empty_single_invocation.clone(),
                    verification_script: vec![],
                }, Witness::default()],
                ..Default::default()
            },
            ..Default::default()
        }),
        ("fallback tx: invalid number of Conflicts attributes", P2PNotaryRequest {
            main_transaction: main_tx.clone(),
            fallback_transaction: Transaction {
                attributes: vec![Attribute::NotValidBefore { height: 123 }],
                signers: vec![Signer { account: util::random_uint160() }, Signer { account: util::random_uint160() }],
                scripts: vec![Witness {
                    invocation_script: empty_single_invocation.clone(),
                    verification_script: vec![],
                }, Witness::default()],
                ..Default::default()
            },
            ..Default::default()
        }),
        ("fallback tx: does not conflicts with main tx", P2PNotaryRequest {
            main_transaction: main_tx.clone(),
            fallback_transaction: Transaction {
                attributes: vec![
                    Attribute::NotValidBefore { height: 123 },
                    Attribute::Conflicts { hash: util::Uint256::default() },
                ],
                signers: vec![Signer { account: util::random_uint160() }, Signer { account: util::random_uint160() }],
                scripts: vec![Witness {
                    invocation_script: empty_single_invocation.clone(),
                    verification_script: vec![],
                }, Witness::default()],
                ..Default::default()
            },
            ..Default::default()
        }),
        ("fallback tx: missing NotaryAssisted attribute", P2PNotaryRequest {
            main_transaction: main_tx.clone(),
            fallback_transaction: Transaction {
                attributes: vec![
                    Attribute::NotValidBefore { height: 123 },
                    Attribute::Conflicts { hash: main_tx.hash() },
                ],
                signers: vec![Signer { account: util::random_uint160() }, Signer { account: util::random_uint160() }],
                scripts: vec![Witness {
                    invocation_script: empty_single_invocation.clone(),
                    verification_script: vec![],
                }, Witness::default()],
                ..Default::default()
            },
            ..Default::default()
        }),
        ("fallback tx: non-zero NKeys", P2PNotaryRequest {
            main_transaction: main_tx.clone(),
            fallback_transaction: Transaction {
                attributes: vec![
                    Attribute::NotValidBefore { height: 123 },
                    Attribute::Conflicts { hash: main_tx.hash() },
                    Attribute::NotaryAssisted(NotaryAssisted { n_keys: 1 }),
                ],
                signers: vec![Signer { account: util::random_uint160() }, Signer { account: util::random_uint160() }],
                scripts: vec![Witness {
                    invocation_script: empty_single_invocation.clone(),
                    verification_script: vec![],
                }, Witness::default()],
                ..Default::default()
            },
            ..Default::default()
        }),
        ("fallback tx: ValidUntilBlock mismatch", P2PNotaryRequest {
            main_transaction: main_tx.clone(),
            fallback_transaction: Transaction {
                valid_until_block: 321,
                attributes: vec![
                    Attribute::NotValidBefore { height: 123 },
                    Attribute::Conflicts { hash: main_tx.hash() },
                    Attribute::NotaryAssisted(NotaryAssisted { n_keys: 0 }),
                ],
                signers: vec![Signer { account: util::random_uint160() }, Signer { account: util::random_uint160() }],
                scripts: vec![Witness {
                    invocation_script: empty_single_invocation.clone(),
                    verification_script: vec![],
                }, Witness::default()],
                ..Default::default()
            },
            ..Default::default()
        }),
    ].iter().cloned().collect();

    for (name, err_case) in error_cases {
        assert!(err_case.is_valid().is_err(), "{}", name);
    }

    let p = P2PNotaryRequest {
        main_transaction: main_tx.clone(),
        fallback_transaction: Transaction {
            valid_until_block: 123,
            attributes: vec![
                Attribute::NotValidBefore { height: 123 },
                Attribute::Conflicts { hash: main_tx.hash() },
                Attribute::NotaryAssisted(NotaryAssisted { n_keys: 0 }),
            ],
            signers: vec![Signer { account: util::random_uint160() }, Signer { account: util::random_uint160() }],
            scripts: vec![Witness { invocation_script: empty_single_invocation.clone(), verification_script: vec![] }, Witness::default()],
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(p.is_valid().is_ok());
}

#[test]
fn test_notary_request_bytes_from_bytes() {
    let main_tx = Transaction {
        attributes: vec![Attribute::NotaryAssisted(NotaryAssisted { n_keys: 1 })],
        script: vec![0, 1, 2],
        valid_until_block: 123,
        signers: vec![Signer { account: util::Uint160::from([1, 5, 9]) }],
        scripts: vec![Witness {
            invocation_script: vec![1, 4, 7],
            verification_script: vec![3, 6, 9],
        }],
        ..Default::default()
    };
    let fallback_tx = Transaction {
        script: vec![3, 2, 1],
        valid_until_block: 123,
        attributes: vec![
            Attribute::NotValidBefore { height: 123 },
            Attribute::Conflicts { hash: main_tx.hash() },
            Attribute::NotaryAssisted(NotaryAssisted { n_keys: 0 }),
        ],
        signers: vec![Signer { account: util::Uint160::from([1, 4, 7]) }, Signer { account: util::Uint160::from([9, 8, 7]) }],
        scripts: vec![
            Witness { invocation_script: {
                let mut script = vec![opcode::PUSHDATA1 as u8, keys::SIGNATURE_LEN as u8];
                script.extend(vec![0; keys::SIGNATURE_LEN]);
                script
            }, verification_script: vec![] },
            Witness { invocation_script: vec![1, 2, 3], verification_script: vec![1, 2, 3] },
        ],
        ..Default::default()
    };
    let p = P2PNotaryRequest {
        main_transaction: main_tx.clone(),
        fallback_transaction: fallback_tx.clone(),
        witness: Witness {
            invocation_script: vec![1, 2, 3],
            verification_script: vec![7, 8, 9],
        },
        ..Default::default()
    };

    p.hash(); // initialize hash caches
    let bytes = p.bytes().unwrap();
    let actual = P2PNotaryRequest::from_bytes(&bytes).unwrap();
    assert_eq!(p, actual);
}

#[test]
fn test_p2p_notary_request_copy() {
    let priv_key = keys::PrivateKey::new().unwrap();
    let orig = P2PNotaryRequest {
        main_transaction: Transaction {
            network_fee: 2000,
            system_fee: 500,
            nonce: 12345678,
            valid_until_block: 100,
            version: 1,
            signers: vec![
                Signer {
                    account: util::random_uint160(),
                    scopes: transaction::Global,
                    allowed_contracts: vec![util::random_uint160()],
                    allowed_groups: vec![priv_key.public_key()],
                    rules: vec![transaction::WitnessRule {
                        action: 0x01,
                        condition: transaction::Condition::CalledByEntry,
                    }],
                },
                Signer {
                    account: util::random_uint160(),
                    scopes: transaction::CalledByEntry,
                },
            ],
            attributes: vec![Attribute::HighPriority(transaction::OracleResponse {
                id: 0,
                code: transaction::Success,
                result: vec![4, 8, 15, 16, 23, 42],
            })],
            scripts: vec![Witness {
                invocation_script: vec![0x04, 0x05],
                verification_script: vec![0x06, 0x07],
            }],
            ..Default::default()
        },
        fallback_transaction: Transaction {
            version: 2,
            system_fee: 200,
            network_fee: 100,
            script: vec![3, 2, 1],
            signers: vec![Signer { account: util::Uint160::from([4, 5, 6]) }],
            attributes: vec![Attribute::NotValidBefore { height: 123 }],
            scripts: vec![Witness {
                invocation_script: vec![0x0D, 0x0E],
                verification_script: vec![0x0F, 0x10],
            }],
            ..Default::default()
        },
        witness: Witness {
            invocation_script: vec![0x11, 0x12],
            verification_script: vec![0x13, 0x14],
        },
        ..Default::default()
    };

    let mut p2p_copy = orig.clone();

    assert_eq!(orig, p2p_copy);
    assert_ne!(orig as *const _, &p2p_copy as *const _);

    assert_eq!(orig.main_transaction, p2p_copy.main_transaction);
    assert_eq!(orig.fallback_transaction, p2p_copy.fallback_transaction);
    assert_eq!(orig.witness, p2p_copy.witness);

    p2p_copy.main_transaction.version = 3;
    p2p_copy.fallback_transaction.script[0] = 0x1F;
    p2p_copy.witness.verification_script[1] = 0x22;

    assert_ne!(orig.main_transaction.version, p2p_copy.main_transaction.version);
    assert_ne!(orig.fallback_transaction.script[0], p2p_copy.fallback_transaction.script[0]);
    assert_ne!(orig.witness.verification_script[1], p2p_copy.witness.verification_script[1]);
}
