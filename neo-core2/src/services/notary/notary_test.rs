use std::collections::HashMap;
use std::sync::Arc;
use neo_core2::fakechain::FakeChain;
use neo_core2::config::{Config, P2PNotary};
use neo_core2::config::netmode::NetMode;
use neo_core2::mempool::Mempool;
use neo_core2::transaction::{Transaction, Signer, Witness};
use neo_core2::crypto::hash::Hash160;
use neo_core2::crypto::keys::{PrivateKey, PublicKey, Signature};
use neo_core2::smartcontract::SmartContract;
use neo_core2::util::Uint160;
use neo_core2::vm::opcode::Opcode;
use neo_core2::notary::Notary;
use neo_core2::witness_info::WitnessInfo;
use neo_core2::witness_type::WitnessType;
use neo_core2::keys::PublicKeys;
use neo_core2::zaptest::ZapTest;
use neo_core2::require::Require;

#[test]
fn test_wallet() {
    let bc = FakeChain::new();
    let main_cfg = P2PNotary { enabled: true };
    let cfg = Config {
        main_cfg,
        chain: bc.clone(),
        log: ZapTest::new_logger(),
    };

    let mut cfg_clone = cfg.clone();
    cfg_clone.main_cfg.unlock_wallet.path = "./testdata/does_not_exists.json";
    let result = Notary::new(cfg_clone, NetMode::UnitTestNet, Mempool::new(1, 1, true, None), None);
    Require::error(result);

    let mut cfg_clone = cfg.clone();
    cfg_clone.main_cfg.unlock_wallet.path = "./testdata/notary1.json";
    cfg_clone.main_cfg.unlock_wallet.password = "invalid".to_string();
    let result = Notary::new(cfg_clone, NetMode::UnitTestNet, Mempool::new(1, 1, true, None), None);
    Require::error(result);

    let mut cfg_clone = cfg.clone();
    cfg_clone.main_cfg.unlock_wallet.path = "./testdata/notary1.json";
    cfg_clone.main_cfg.unlock_wallet.password = "one".to_string();
    let result = Notary::new(cfg_clone, NetMode::UnitTestNet, Mempool::new(1, 1, true, None), None);
    Require::no_error(result);
}

#[test]
fn test_verify_incomplete_request() {
    let bc = FakeChain::new();
    let notary_contract_hash = Uint160::new([1, 2, 3]);
    bc.set_notary_contract_script_hash(notary_contract_hash);
    let (_, ntr, _) = get_test_notary("./testdata/notary1.json", "one");
    let sig = vec![Opcode::PUSHDATA1 as u8, Signature::len() as u8];
    let acc1 = PrivateKey::new();
    let acc2 = PrivateKey::new();
    let acc3 = PrivateKey::new();
    let sig_script1 = acc1.public_key().get_verification_script();
    let sig_script2 = acc2.public_key().get_verification_script();
    let sig_script3 = acc3.public_key().get_verification_script();
    let multisig_script1 = SmartContract::create_multi_sig_redeem_script(1, PublicKeys::new(vec![acc1.public_key(), acc2.public_key(), acc3.public_key()])).unwrap();
    let multisig_script_hash1 = Hash160::hash160(&multisig_script1);
    let multisig_script2 = SmartContract::create_multi_sig_redeem_script(2, PublicKeys::new(vec![acc1.public_key(), acc2.public_key(), acc3.public_key()])).unwrap();
    let multisig_script_hash2 = Hash160::hash160(&multisig_script2);

    let check_err = |tx: &Transaction, n_keys: u8| {
        let witness_info = ntr.verify_incomplete_witnesses(tx, n_keys);
        Require::error(witness_info);
        Require::is_none(witness_info);
    };

    let err_cases: HashMap<&str, (Transaction, u8)> = [
        ("not enough signers", (Transaction::new(vec![Signer::new(notary_contract_hash)], vec![Witness::new()]), 0)),
        ("missing Notary witness", (Transaction::new(vec![Signer::new(acc1.get_script_hash()), Signer::new(acc2.get_script_hash())], vec![Witness::new(), Witness::new()]), 0)),
        ("bad verification script", (Transaction::new(vec![Signer::new(acc1.public_key().get_script_hash()), Signer::new(notary_contract_hash)], vec![Witness::new_with_scripts(vec![], vec![1, 2, 3]), Witness::new()]), 0)),
        ("sig: bad nKeys", (Transaction::new(vec![Signer::new(acc1.public_key().get_script_hash()), Signer::new(acc2.public_key().get_script_hash()), Signer::new(notary_contract_hash)], vec![Witness::new_with_scripts(sig.clone(), sig_script1.clone()), Witness::new_with_scripts(sig.clone(), sig_script2.clone()), Witness::new()]), 3)),
        ("multisig: bad witnesses count", (Transaction::new(vec![Signer::new(multisig_script_hash1), Signer::new(notary_contract_hash)], vec![Witness::new_with_scripts(sig.clone(), multisig_script1.clone())]), 2)),
        ("multisig: bad nKeys", (Transaction::new(vec![Signer::new(multisig_script_hash1), Signer::new(notary_contract_hash)], vec![Witness::new_with_scripts(sig.clone(), multisig_script1.clone()), Witness::new()]), 2)),
    ].iter().cloned().collect();

    for (name, (tx, n_keys)) in err_cases {
        check_err(&tx, n_keys);
    }

    let test_cases: HashMap<&str, (Transaction, u8, Vec<WitnessInfo>)> = [
        ("single sig", (Transaction::new(vec![Signer::new(acc1.get_script_hash()), Signer::new(notary_contract_hash)], vec![Witness::new_with_scripts(sig.clone(), sig_script1.clone()), Witness::new()]), 1, vec![WitnessInfo::new(WitnessType::Signature, 1, PublicKeys::new(vec![acc1.public_key()])), WitnessInfo::new(WitnessType::Contract, 0, PublicKeys::new(vec![]))])),
        ("multiple sig", (Transaction::new(vec![Signer::new(acc1.get_script_hash()), Signer::new(acc2.get_script_hash()), Signer::new(acc3.get_script_hash()), Signer::new(notary_contract_hash)], vec![Witness::new_with_scripts(sig.clone(), sig_script1.clone()), Witness::new_with_scripts(vec![], sig_script2.clone()), Witness::new_with_scripts(sig.clone(), sig_script3.clone()), Witness::new()]), 3, vec![WitnessInfo::new(WitnessType::Signature, 1, PublicKeys::new(vec![acc1.public_key()])), WitnessInfo::new(WitnessType::Signature, 1, PublicKeys::new(vec![acc2.public_key()])), WitnessInfo::new(WitnessType::Signature, 1, PublicKeys::new(vec![acc3.public_key()])), WitnessInfo::new(WitnessType::Contract, 0, PublicKeys::new(vec![]))])),
        ("single multisig 1 out of 3", (Transaction::new(vec![Signer::new(multisig_script_hash1), Signer::new(notary_contract_hash)], vec![Witness::new_with_scripts(sig.clone(), multisig_script1.clone()), Witness::new()]), 3, vec![WitnessInfo::new(WitnessType::MultiSignature, 1, PublicKeys::new(vec![acc1.public_key(), acc2.public_key(), acc3.public_key()])), WitnessInfo::new(WitnessType::Contract, 0, PublicKeys::new(vec![]))])),
        ("single multisig 2 out of 3", (Transaction::new(vec![Signer::new(multisig_script_hash2), Signer::new(notary_contract_hash)], vec![Witness::new_with_scripts(sig.clone(), multisig_script2.clone()), Witness::new()]), 3, vec![WitnessInfo::new(WitnessType::MultiSignature, 2, PublicKeys::new(vec![acc1.public_key(), acc2.public_key(), acc3.public_key()])), WitnessInfo::new(WitnessType::Contract, 0, PublicKeys::new(vec![]))])),
        ("empty sig + single multisig 1 out of 3", (Transaction::new(vec![Signer::new(acc1.public_key().get_script_hash()), Signer::new(multisig_script_hash1), Signer::new(notary_contract_hash)], vec![Witness::new_with_scripts(vec![], sig_script1.clone()), Witness::new_with_scripts(sig.clone(), multisig_script1.clone()), Witness::new()]), 1 + 3, vec![WitnessInfo::new(WitnessType::Signature, 1, PublicKeys::new(vec![acc1.public_key()])), WitnessInfo::new(WitnessType::MultiSignature, 1, PublicKeys::new(vec![acc1.public_key(), acc2.public_key(), acc3.public_key()])), WitnessInfo::new(WitnessType::Contract, 0, PublicKeys::new(vec![]))])),
        ("single multisig 1 out of 3 + empty single sig", (Transaction::new(vec![Signer::new(multisig_script_hash1), Signer::new(acc1.public_key().get_script_hash()), Signer::new(notary_contract_hash)], vec![Witness::new_with_scripts(sig.clone(), multisig_script1.clone()), Witness::new_with_scripts(vec![], sig_script1.clone()), Witness::new()]), 3 + 1, vec![WitnessInfo::new(WitnessType::MultiSignature, 1, PublicKeys::new(vec![acc1.public_key(), acc2.public_key(), acc3.public_key()])), WitnessInfo::new(WitnessType::Signature, 1, PublicKeys::new(vec![acc1.public_key()])), WitnessInfo::new(WitnessType::Contract, 0, PublicKeys::new(vec![]))])),
        ("several multisig witnesses", (Transaction::new(vec![Signer::new(multisig_script_hash1), Signer::new(multisig_script_hash2), Signer::new(notary_contract_hash)], vec![Witness::new_with_scripts(sig.clone(), multisig_script1.clone()), Witness::new_with_scripts(sig.clone(), multisig_script2.clone()), Witness::new()]), 3 + 3, vec![WitnessInfo::new(WitnessType::MultiSignature, 1, PublicKeys::new(vec![acc1.public_key(), acc2.public_key(), acc3.public_key()])), WitnessInfo::new(WitnessType::MultiSignature, 2, PublicKeys::new(vec![acc1.public_key(), acc2.public_key(), acc3.public_key()])), WitnessInfo::new(WitnessType::Contract, 0, PublicKeys::new(vec![]))])),
        ("multisig + sig", (Transaction::new(vec![Signer::new(multisig_script_hash1), Signer::new(acc1.public_key().get_script_hash()), Signer::new(notary_contract_hash)], vec![Witness::new_with_scripts(sig.clone(), multisig_script1.clone()), Witness::new_with_scripts(sig.clone(), sig_script1.clone()), Witness::new()]), 3 + 1, vec![WitnessInfo::new(WitnessType::MultiSignature, 1, PublicKeys::new(vec![acc1.public_key(), acc2.public_key(), acc3.public_key()])), WitnessInfo::new(WitnessType::Signature, 1, PublicKeys::new(vec![acc1.public_key()])), WitnessInfo::new(WitnessType::Contract, 0, PublicKeys::new(vec![]))])),
        ("sig + multisig", (Transaction::new(vec![Signer::new(acc1.public_key().get_script_hash()), Signer::new(multisig_script_hash1), Signer::new(notary_contract_hash)], vec![Witness::new_with_scripts(sig.clone(), sig_script1.clone()), Witness::new_with_scripts(sig.clone(), multisig_script1.clone()), Witness::new()]), 1 + 3, vec![WitnessInfo::new(WitnessType::Signature, 1, PublicKeys::new(vec![acc1.public_key()])), WitnessInfo::new(WitnessType::MultiSignature, 1, PublicKeys::new(vec![acc1.public_key(), acc2.public_key(), acc3.public_key()])), WitnessInfo::new(WitnessType::Contract, 0, PublicKeys::new(vec![]))])),
        ("empty multisig + sig", (Transaction::new(vec![Signer::new(multisig_script_hash1), Signer::new(acc1.public_key().get_script_hash()), Signer::new(notary_contract_hash)], vec![Witness::new_with_scripts(vec![], multisig_script1.clone()), Witness::new_with_scripts(sig.clone(), sig_script1.clone()), Witness::new()]), 3 + 1, vec![WitnessInfo::new(WitnessType::MultiSignature, 1, PublicKeys::new(vec![acc1.public_key(), acc2.public_key(), acc3.public_key()])), WitnessInfo::new(WitnessType::Signature, 1, PublicKeys::new(vec![acc1.public_key()])), WitnessInfo::new(WitnessType::Contract, 0, PublicKeys::new(vec![]))])),
        ("sig + empty multisig", (Transaction::new(vec![Signer::new(acc1.public_key().get_script_hash()), Signer::new(multisig_script_hash1), Signer::new(notary_contract_hash)], vec![Witness::new_with_scripts(sig.clone(), sig_script1.clone()), Witness::new_with_scripts(vec![], multisig_script1.clone()), Witness::new()]), 1 + 3, vec![WitnessInfo::new(WitnessType::Signature, 1, PublicKeys::new(vec![acc1.public_key()])), WitnessInfo::new(WitnessType::MultiSignature, 1, PublicKeys::new(vec![acc1.public_key(), acc2.public_key(), acc3.public_key()])), WitnessInfo::new(WitnessType::Contract, 0, PublicKeys::new(vec![]))])),
        ("multisig + empty sig", (Transaction::new(vec![Signer::new(multisig_script_hash1), Signer::new(acc1.public_key().get_script_hash()), Signer::new(notary_contract_hash)], vec![Witness::new_with_scripts(sig.clone(), multisig_script1.clone()), Witness::new_with_scripts(vec![], sig_script1.clone()), Witness::new()]), 3 + 1, vec![WitnessInfo::new(WitnessType::MultiSignature, 1, PublicKeys::new(vec![acc1.public_key(), acc2.public_key(), acc3.public_key()])), WitnessInfo::new(WitnessType::Signature, 1, PublicKeys::new(vec![acc1.public_key()])), WitnessInfo::new(WitnessType::Contract, 0, PublicKeys::new(vec![]))])),
        ("empty sig + multisig", (Transaction::new(vec![Signer::new(acc1.public_key().get_script_hash()), Signer::new(multisig_script_hash1), Signer::new(notary_contract_hash)], vec![Witness::new_with_scripts(vec![], sig_script1.clone()), Witness::new_with_scripts(sig.clone(), multisig_script1.clone()), Witness::new()]), 1 + 3, vec![WitnessInfo::new(WitnessType::Signature, 1, PublicKeys::new(vec![acc1.public_key()])), WitnessInfo::new(WitnessType::MultiSignature, 1, PublicKeys::new(vec![acc1.public_key(), acc2.public_key(), acc3.public_key()])), WitnessInfo::new(WitnessType::Contract, 0, PublicKeys::new(vec![]))])),
        ("multiple sigs + multiple multisigs", (Transaction::new(vec![Signer::new(multisig_script_hash1), Signer::new(acc1.public_key().get_script_hash()), Signer::new(acc2.public_key().get_script_hash()), Signer::new(acc3.public_key().get_script_hash()), Signer::new(multisig_script_hash2), Signer::new(notary_contract_hash)], vec![Witness::new_with_scripts(sig.clone(), multisig_script1.clone()), Witness::new_with_scripts(sig.clone(), sig_script1.clone()), Witness::new_with_scripts(vec![], sig_script2.clone()), Witness::new_with_scripts(sig.clone(), sig_script3.clone()), Witness::new_with_scripts(vec![], multisig_script2.clone()), Witness::new()]), 3 + 1 + 1 + 1 + 3, vec![WitnessInfo::new(WitnessType::MultiSignature, 1, PublicKeys::new(vec![acc1.public_key(), acc2.public_key(), acc3.public_key()])), WitnessInfo::new(WitnessType::Signature, 1, PublicKeys::new(vec![acc1.public_key()])), WitnessInfo::new(WitnessType::Signature, 1, PublicKeys::new(vec![acc2.public_key()])), WitnessInfo::new(WitnessType::Signature, 1, PublicKeys::new(vec![acc3.public_key()])), WitnessInfo::new(WitnessType::MultiSignature, 2, PublicKeys::new(vec![acc1.public_key(), acc2.public_key(), acc3.public_key()])), WitnessInfo::new(WitnessType::Contract, 0, PublicKeys::new(vec![]))])),
    ].iter().cloned().collect();

    for (name, (tx, n_keys, expected_info)) in test_cases {
        let actual_info = ntr.verify_incomplete_witnesses(&tx, n_keys).unwrap();
        Require::equal(expected_info.len(), actual_info.len());
        for (i, expected) in expected_info.iter().enumerate() {
            let actual = &actual_info[i];
            Require::equal(expected.typ, actual.typ);
            Require::equal(expected.n_sigs_left, actual.n_sigs_left);
            Require::elements_match(&expected.pubs, &actual.pubs);
            Require::is_none(&actual.sigs);
        }
    }
}
