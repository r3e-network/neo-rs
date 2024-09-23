use std::path::PathBuf;
use std::fs;
use std::io;
use serde_json;

use neo_core2::wallet::{Wallet, Account, Contract, Token};
use neo_core2::encoding::address;
use neo_core2::smartcontract::manifest;
use neo_core2::util::Uint160;

const WALLET_TEMPLATE: &str = "testWallet";

#[test]
fn test_new_wallet() {
    let wallet = check_wallet_constructor().unwrap();
    assert!(wallet.is_some());
}

#[test]
fn test_new_wallet_from_file_negative_empty_file() {
    let _ = check_wallet_constructor().unwrap();
    let wallet_from_file = Wallet::new_from_file(WALLET_TEMPLATE);
    assert!(wallet_from_file.is_err());
    assert_eq!(wallet_from_file.unwrap_err().to_string(), "EOF");
}

#[test]
fn test_new_wallet_from_file_negative_no_file() {
    let result = Wallet::new_from_file(WALLET_TEMPLATE);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().to_string(), "No such file or directory (os error 2)");
}

#[test]
fn test_create_account_and_close() {
    let mut wallet = check_wallet_constructor().unwrap();
    
    wallet.create_account("testName", "testPass").unwrap();
    assert_eq!(wallet.accounts.len(), 1);
    assert!(wallet.accounts[0].can_sign());
    wallet.close();
    assert!(!wallet.accounts[0].can_sign());
}

#[test]
fn test_add_account() {
    let wallets = vec![
        check_wallet_constructor().unwrap(),
        Wallet::new_in_memory(),
    ];

    for mut w in wallets {
        w.add_account(Account {
            private_key: None,
            address: "real".to_string(),
            encrypted_wif: "".to_string(),
            label: "".to_string(),
            contract: None,
            locked: false,
            default: false,
        });
        assert_eq!(w.accounts.len(), 1);

        assert!(w.remove_account("abc").is_err());
        assert_eq!(w.accounts.len(), 1);
        assert!(w.remove_account("real").is_ok());
        assert_eq!(w.accounts.len(), 0);
    }
}

#[test]
fn test_path() {
    let wallet = check_wallet_constructor().unwrap();
    assert!(!wallet.path().is_empty());
}

#[test]
fn test_save() {
    let mut in_mem_wallet = Wallet::new_in_memory();
    
    let tmp_dir = tempfile::tempdir().unwrap();
    let file = tmp_dir.path().join(WALLET_TEMPLATE);
    in_mem_wallet.set_path(file.clone());

    let wallets = vec![
        check_wallet_constructor().unwrap(),
        in_mem_wallet,
    ];

    for mut w in wallets {
        w.add_account(Account {
            private_key: None,
            address: "".to_string(),
            encrypted_wif: "".to_string(),
            label: "".to_string(),
            contract: None,
            locked: false,
            default: false,
        });

        w.save().unwrap();

        let opened_wallet = Wallet::new_from_file(&w.path()).unwrap();
        assert_eq!(w.accounts, opened_wallet.accounts);

        // change and rewrite
        {
            opened_wallet.create_account("test", "pass").unwrap();

            let w2 = Wallet::new_from_file(&opened_wallet.path()).unwrap();
            assert_eq!(w2.accounts.len(), 2);
            w2.accounts[1].decrypt("pass", &w2.scrypt).unwrap();
            let _ = w2.accounts[1].script_hash(); // opened_wallet has it for acc 1.
            assert_eq!(opened_wallet.accounts, w2.accounts);
        }
    }
}

#[test]
fn test_json_marshall_unmarshal() {
    let wallet = check_wallet_constructor().unwrap();

    let bytes = wallet.to_json().unwrap();
    assert!(!bytes.is_empty());

    let unmarshalled_wallet: Wallet = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(wallet.version, unmarshalled_wallet.version);
    assert_eq!(wallet.accounts, unmarshalled_wallet.accounts);
    assert_eq!(wallet.scrypt, unmarshalled_wallet.scrypt);
}

fn check_wallet_constructor() -> io::Result<Wallet> {
    let tmp_dir = tempfile::tempdir()?;
    let file = tmp_dir.path().join(WALLET_TEMPLATE);
    Wallet::new(&file)
}

#[test]
fn test_wallet_add_token() {
    let mut w = check_wallet_constructor().unwrap();
    let tok = Token::new(Uint160::from_slice(&[1, 2, 3]), "Rubl", "RUB", 2, manifest::NEP17_STANDARD_NAME);
    assert_eq!(w.extra.tokens.len(), 0);
    w.add_token(tok.clone());
    assert_eq!(w.extra.tokens.len(), 1);
    assert!(w.remove_token(&Uint160::from_slice(&[4, 5, 6])).is_err());
    assert_eq!(w.extra.tokens.len(), 1);
    assert!(w.remove_token(&tok.hash).is_ok());
    assert_eq!(w.extra.tokens.len(), 0);
}

#[test]
fn test_wallet_get_account() {
    let mut wallet = check_wallet_constructor().unwrap();
    let accounts = vec![
        Account {
            contract: Some(Contract {
                script: vec![0, 1, 2, 3],
                ..Default::default()
            }),
            ..Default::default()
        },
        Account {
            contract: Some(Contract {
                script: vec![3, 2, 1, 0],
                ..Default::default()
            }),
            ..Default::default()
        },
    ];

    for acc in &mut accounts {
        acc.address = address::uint160_to_string(&acc.contract.as_ref().unwrap().script_hash());
        wallet.add_account(acc.clone());
    }

    for (i, acc) in accounts.iter().enumerate() {
        let h = acc.contract.as_ref().unwrap().script_hash();
        assert_eq!(acc, &wallet.get_account(&h).unwrap(), "can't get {} account", i);
    }
}

#[test]
fn test_wallet_get_change_address() {
    let w1 = Wallet::new_from_file("testdata/wallet1.json").unwrap();
    let sh = w1.get_change_address();
    // No default address, the first one is used.
    assert_eq!("Nhfg3TbpwogLvDGVvAvqyThbsHgoSUKwtn", address::uint160_to_string(&sh));
    let w2 = Wallet::new_from_file("testdata/wallet2.json").unwrap();
    let sh = w2.get_change_address();
    // Default address.
    assert_eq!("NMUedC8TSV2rE17wGguSvPk9XcmHSaT275", address::uint160_to_string(&sh));
}

#[test]
fn test_wallet_for_examples() {
    const EXAMPLES_DIR: &str = "../../examples";
    const WALLET_FILE: &str = "my_wallet.json";
    const WALLET_PASS: &str = "qwerty";
    const ACCOUNT_LABEL: &str = "my_account";

    let w = Wallet::new_from_file(&PathBuf::from(EXAMPLES_DIR).join(WALLET_FILE)).unwrap();
    assert_eq!(w.accounts.len(), 1);
    assert_eq!(w.accounts[0].label, ACCOUNT_LABEL);
    w.accounts[0].decrypt(WALLET_PASS, &w.scrypt).unwrap();

    // we need to keep the owner of the example contracts the same as the wallet account
    assert_eq!(w.accounts[0].address, "NbrUYaZgyhSkNoRo9ugRyEMdUZxrhkNaWB", "need to change `owner` in the example contracts");
}

#[test]
fn test_from_bytes() {
    let wallet = check_wallet_constructor().unwrap();
    let bts = wallet.to_json().unwrap();

    let w = Wallet::new_from_bytes(&bts).unwrap();

    assert!(w.path().is_empty());
    w.set_path(wallet.path());

    assert_eq!(wallet, w);
}
