use neo_base::AddressVersion;
use neo_wallet::{Nep6Account, Nep6Contract, Nep6Parameter, Nep6Wallet, Wallet};

#[test]
fn parse_nep6_wallet_fixture() {
    let json = include_str!("../tests/fixtures/ut_nep6_wallet.json");
    let wallet: Nep6Wallet = serde_json::from_str(json).expect("parse wallet");
    assert_eq!(wallet.name, "name");
    assert_eq!(wallet.version, "1.0");
    assert_eq!(wallet.scrypt.n, 2);
    assert_eq!(wallet.accounts.len(), 1);
    let account = &wallet.accounts[0];
    assert_eq!(account.address, "NdtB8RXRmJ7Nhw1FPTm7E6HoDZGnDw37nf");
    assert!(account.contract.is_some());
}

#[test]
fn serialize_nep6_account_contract() {
    let contract = Nep6Contract {
        script: "IQNgPziA63rqCtRQCJOSXkpC/qSKRO5viYoQs8fOBdKiZ6w=".to_string(),
        parameters: vec![Nep6Parameter {
            name: "Sig".to_string(),
            type_id: 0,
        }],
        deployed: false,
    };
    let account = Nep6Account {
        address: "NdtB8RXRmJ7Nhw1FPTm7E6HoDZGnDw37nf".to_string(),
        label: None,
        is_default: false,
        lock: false,
        key: None,
        contract: Some(contract),
        extra: None,
    };
    let serialized = serde_json::to_string(&account).expect("serialize");
    assert!(serialized.contains("\"address\":\"NdtB8R"));
}

#[test]
fn wallet_imports_watch_only_accounts() {
    let json = include_str!("../tests/fixtures/ut_nep6_wallet.json");
    let wallet_json: Nep6Wallet = serde_json::from_str(json).expect("parse wallet");
    let wallet = Wallet::from_nep6_wallet(&wallet_json, None, AddressVersion::MAINNET)
        .expect("wallet import");
    assert_eq!(wallet.len(), 1);
}
