use std::fs;
use std::path::Path;
use std::error::Error;

use neo_core2::crypto::keys::{PublicKey, PrivateKey, ScryptParams};
use neo_core2::wallet::{Account, Wallet, Contract, ContractParam};
use neo_core2::core::state;
use neo_core2::encoding::address;
use neo_core2::smartcontract::{manifest, nef};

const REGENERATE: bool = false;

const DOCKER_WALLET_DIR: &str = "../../.docker/wallets/";

// privNetKeys is a list of unencrypted WIFs sorted by wallet number.
const PRIVNET_WIFS: [&str; 4] = [
    "KxyjQ8eUa4FHt3Gvioyt1Wz29cTUrE4eTqX3yFSk1YFCsPL8uNsY",
    "KzfPUYDC9n2yf4fK5ro4C8KMcdeXtFuEnStycbZgX3GomiUsvX6W",
    "L2oEXKRAAMiPEZukwR5ho2S6SMeQLhcK9mF71ZnF7GvT8dU4Kkgz",
    "KzgWE3u3EDp13XPXXuTKZxeJ3Gi8Bsm8f9ijY3ZsCKKRvZUo1Cdn",
];

const PASSWORDS: [&str; 4] = ["one", "two", "three", "four"];

// naiveScrypt is naive scrypt parameters used for testing wallets to save
// time on accounts decryption.
const NAIVE_SCRYPT: ScryptParams = ScryptParams { n: 2, r: 1, p: 1 };

fn get_keys() -> Vec<PublicKey> {
    PRIVNET_WIFS.iter()
        .map(|wif| PrivateKey::from_wif(wif).unwrap().public_key())
        .collect()
}

fn get_nep2_account(wif: &str, pass: &str) -> Account {
    get_account_with_scrypt(wif, pass, ScryptParams::nep2())
}

fn get_testing_account(wif: &str, pass: &str) -> Account {
    get_account_with_scrypt(wif, pass, NAIVE_SCRYPT)
}

fn get_account_with_scrypt(wif: &str, pass: &str, scrypt: ScryptParams) -> Account {
    let mut acc = Account::from_wif(wif).unwrap();
    acc.encrypt(pass, scrypt).unwrap();
    acc
}

#[test]
fn test_regenerate_solo_wallet() {
    if !REGENERATE {
        return;
    }
    let wallet_path = Path::new(DOCKER_WALLET_DIR).join("wallet1_solo.json");
    let wif = PRIVNET_WIFS[0];
    let mut acc1 = get_nep2_account(wif, "one");
    let mut acc2 = get_nep2_account(wif, "one");
    acc2.convert_multisig(3, &get_keys()).unwrap();

    let mut acc3 = get_nep2_account(wif, "one");
    acc3.convert_multisig(1, &[get_keys()[0].clone()]).unwrap();

    create_nep2_wallet(&wallet_path, &[acc1, acc2, acc3]);
}

fn regenerate_wallets(dir: &Path) {
    let pubs = get_keys();
    for (i, wif) in PRIVNET_WIFS.iter().enumerate() {
        let mut acc1 = get_nep2_account(wif, PASSWORDS[i]);
        let mut acc2 = get_nep2_account(wif, PASSWORDS[i]);
        acc2.convert_multisig(3, &pubs).unwrap();

        create_nep2_wallet(&dir.join(format!("wallet{}.json", i + 1)), &[acc1, acc2]);
    }
}

#[test]
fn test_regenerate_privnet_wallets() {
    if !REGENERATE {
        return;
    }
    let dirs = [
        DOCKER_WALLET_DIR,
        "../consensus/testdata",
    ];
    for dir in dirs.iter() {
        regenerate_wallets(Path::new(dir));
    }
}

// ... (other test functions follow the same pattern)

fn create_nep2_wallet(path: &Path, accs: &[Account]) {
    create_wallet(path, ScryptParams::nep2(), accs);
}

fn create_testing_wallet(path: &Path, accs: &[Account]) {
    create_wallet(path, NAIVE_SCRYPT, accs);
}

fn create_wallet(path: &Path, scrypt_params: ScryptParams, accs: &[Account]) {
    let mut w = Wallet::new(path).unwrap();
    w.set_scrypt(scrypt_params);

    if accs.is_empty() {
        panic!("provide at least 1 account");
    }
    for acc in accs {
        w.add_account(acc.clone());
    }
    w.save_pretty().unwrap();
}

// ... (remaining test functions follow the same pattern)
