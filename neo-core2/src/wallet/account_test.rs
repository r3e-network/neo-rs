use std::str::FromStr;
use neo_core2::{
    wallet::{Account, Contract},
    internal::keytestcases,
    core::transaction,
    crypto::{hash, keys},
    encoding::address,
    smartcontract,
};
use serde_json;

#[test]
fn test_new_account() {
    let acc = Account::new().unwrap();
    assert_eq!(acc.address, address::uint160_to_string(&acc.script_hash()));
}

#[test]
fn test_decrypt_account() {
    for test_case in keytestcases::ARR.iter() {
        let mut acc = Account {
            encrypted_wif: Some(test_case.encrypted_wif.clone()),
            ..Default::default()
        };
        assert!(acc.private_key().is_none());
        let result = acc.decrypt(&test_case.passphrase, &keys::NEP2_SCRYPT_PARAMS);
        if test_case.invalid {
            assert!(result.is_err());
            continue;
        }

        assert!(result.is_ok());
        assert!(acc.private_key().is_some());
        assert_eq!(test_case.private_key, acc.private_key().unwrap().to_string());
    }
    // No encrypted key.
    let acc = Account::default();
    assert!(acc.decrypt("qwerty", &keys::NEP2_SCRYPT_PARAMS).is_err());
}

#[test]
fn test_new_from_wif() {
    for test_case in keytestcases::ARR.iter() {
        let result = Account::new_from_wif(&test_case.wif);
        if test_case.invalid {
            assert!(result.is_err());
            continue;
        }

        let acc = result.unwrap();
        compare_fields(test_case, &acc);
    }
}

#[test]
fn test_new_account_from_encrypted_wif() {
    for tc in keytestcases::ARR.iter() {
        let result = Account::new_from_encrypted_wif(&tc.encrypted_wif, &tc.passphrase, &keys::NEP2_SCRYPT_PARAMS);
        if tc.invalid {
            assert!(result.is_err());
            continue;
        }

        let acc = result.unwrap();
        compare_fields(tc, &acc);
    }
}

#[test]
fn test_contract_serialize_deserialize() {
    let data = r#"{"script":"AQI=","parameters":[{"name":"name0", "type":"Signature"}],"deployed":false}"#;
    let c: Contract = serde_json::from_str(data).unwrap();
    assert_eq!(c.script, vec![1, 2]);

    let result = serde_json::to_string(&c).unwrap();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&result).unwrap(), serde_json::from_str::<serde_json::Value>(data).unwrap());

    assert!(serde_json::from_str::<Contract>("1").is_err());

    let invalid_data = r#"{"script":"ERROR","parameters":[1],"deployed":false}"#;
    assert!(serde_json::from_str::<Contract>(invalid_data).is_err());
}

#[test]
fn test_contract_sign_tx() {
    let acc = Account::new().unwrap();
    assert!(acc.can_sign());

    let mut acc_no_contr = acc.clone();
    acc_no_contr.contract = None;
    let tx = transaction::Transaction {
        script: vec![1, 2, 3],
        signers: vec![transaction::Signer {
            account: acc.contract.as_ref().unwrap().script_hash(),
            scopes: transaction::WitnessScope::CalledByEntry,
        }],
        ..Default::default()
    };
    assert!(acc_no_contr.sign_tx(0, &mut tx.clone()).is_err());

    let acc2 = Account::new().unwrap();
    assert!(acc2.can_sign());

    assert!(acc2.sign_tx(0, &mut tx.clone()).is_err());

    let pubs = vec![acc.public_key().unwrap(), acc2.public_key().unwrap()];
    let multi_s = smartcontract::create_default_multi_sig_redeem_script(&pubs).unwrap();
    let mut multi_acc = Account::new_from_private_key(acc.private_key().unwrap());
    multi_acc.convert_multisig(2, &pubs).unwrap();
    let mut multi_acc2 = Account::new_from_private_key(acc2.private_key().unwrap());
    multi_acc2.convert_multisig(2, &pubs).unwrap();

    let mut tx = transaction::Transaction {
        script: vec![1, 2, 3],
        signers: vec![
            transaction::Signer {
                account: acc2.contract.as_ref().unwrap().script_hash(),
                scopes: transaction::WitnessScope::CalledByEntry,
            },
            transaction::Signer {
                account: acc.contract.as_ref().unwrap().script_hash(),
                scopes: transaction::WitnessScope::None,
            },
            transaction::Signer {
                account: hash::hash160(&multi_s),
                scopes: transaction::WitnessScope::None,
            },
        ],
        ..Default::default()
    };
    assert!(acc.sign_tx(0, &mut tx).is_err()); // Can't append, no witness for acc2.

    assert!(acc2.sign_tx(0, &mut tx).is_ok()); // Append script for acc2.
    assert_eq!(tx.scripts.len(), 1);
    assert_eq!(tx.scripts[0].invocation_script.len(), 66);

    assert!(acc2.sign_tx(0, &mut tx).is_ok()); // Sign again, effectively a no-op.
    assert_eq!(tx.scripts.len(), 1);
    assert_eq!(tx.scripts[0].invocation_script.len(), 66);

    acc2.locked = true;
    assert!(!acc2.can_sign());
    assert!(acc2.sign_tx(0, &mut tx).is_err());     // Locked account.
    assert!(acc2.public_key().is_none());         // Locked account.
    assert!(acc2.sign_hashable(0, &tx).is_err()); // Locked account.

    acc2.locked = false;
    acc2.close();
    assert!(!acc2.can_sign());
    assert!(acc2.sign_tx(0, &mut tx).is_err()); // No private key.
    acc2.close();                         // No-op.
    assert!(!acc2.can_sign());

    tx.scripts.push(transaction::Witness {
        verification_script: acc.contract.as_ref().unwrap().script.clone(),
        ..Default::default()
    });
    assert!(acc.sign_tx(0, &mut tx).is_ok()); // Add invocation script for existing witness.
    assert_eq!(tx.scripts[1].invocation_script.len(), 66);
    assert!(acc.sign_hashable(0, &tx).is_ok()); // Works via Hashable too.

    assert!(multi_acc.sign_tx(0, &mut tx).is_ok());
    assert_eq!(tx.scripts.len(), 3);
    assert_eq!(tx.scripts[2].invocation_script.len(), 66);

    assert!(multi_acc2.sign_tx(0, &mut tx).is_ok()); // Append to existing script.
    assert_eq!(tx.scripts.len(), 3);
    assert_eq!(tx.scripts[2].invocation_script.len(), 132);
}

#[test]
fn test_contract_script_hash() {
    let script = vec![0, 1, 2, 3];
    let c = Contract { script: script.clone(), ..Default::default() };

    assert_eq!(hash::hash160(&script), c.script_hash());
}

#[test]
fn test_account_convert_multisig() {
    // test is based on a wallet1_solo.json accounts from neo-local
    let mut a = Account::new_from_wif("KxyjQ8eUa4FHt3Gvioyt1Wz29cTUrE4eTqX3yFSk1YFCsPL8uNsY").unwrap();

    let hexs = vec![
        "02b3622bf4017bdfe317c58aed5f4c753f206b7db896046fa7d774bbc4bf7f8dc2", // <- this is our key
        "02103a7f7dd016558597f7960d27c516a4394fd968b9e65155eb4b013e4040406e",
        "02a7bc55fe8684e0119768d104ba30795bdcc86619e864add26156723ed185cd62",
        "03d90c07df63e690ce77912e10ab51acc944b66860237b608c4f8f8309e71ee699",
    ];

    // Locked
    {
        a.locked = true;
        let pubs = convert_pubs(&hexs);
        assert!(a.convert_multisig(1, &pubs).is_err());
        a.locked = false;
    }

    // No private key
    {
        let pk = a.private_key;
        a.private_key = None;
        let pubs = convert_pubs(&hexs);
        assert!(a.convert_multisig(0, &pubs).is_err());
        a.private_key = pk;
    }

    // Invalid number of signatures
    {
        let pubs = convert_pubs(&hexs);
        assert!(a.convert_multisig(0, &pubs).is_err());
    }

    // Account key is missing from multisig
    {
        let pubs = convert_pubs(&hexs[1..]);
        assert!(a.convert_multisig(1, &pubs).is_err());
    }

    // 1/1 multisig
    {
        let pubs = convert_pubs(&hexs[..1]);
        assert!(a.convert_multisig(1, &pubs).is_ok());
        assert_eq!("NfgHwwTi3wHAS8aFAN243C5vGbkYDpqLHP", a.address);
    }

    // 3/4 multisig
    {
        let pubs = convert_pubs(&hexs);
        assert!(a.convert_multisig(3, &pubs).is_ok());
        assert_eq!("NVTiAjNgagDkTr5HTzDmQP9kPwPHN5BgVq", a.address);
    }
}

fn convert_pubs(hex_keys: &[&str]) -> Vec<keys::PublicKey> {
    hex_keys.iter().map(|&hex| keys::PublicKey::from_str(hex).unwrap()).collect()
}

fn compare_fields(tk: &keytestcases::Ktype, acc: &Account) {
    assert_eq!(tk.address, acc.address, "expected address {} got {}", tk.address, acc.address);
    assert_eq!(tk.wif, acc.private_key().unwrap().wif(), "expected wif {} got {}", tk.wif, acc.private_key().unwrap().wif());
    assert_eq!(tk.public_key, acc.public_key().unwrap().to_string_compressed(), "expected pub key {} got {}", tk.public_key, acc.public_key().unwrap().to_string_compressed());
    assert_eq!(tk.private_key, acc.private_key().unwrap().to_string(), "expected priv key {} got {}", tk.private_key, acc.private_key().unwrap().to_string());
}
