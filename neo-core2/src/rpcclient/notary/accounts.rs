use neo_core2::crypto::{hash, keys};
use neo_core2::encoding::address;
use neo_core2::smartcontract;
use neo_core2::util;
use neo_core2::wallet;

// FakeSimpleAccount creates a fake account belonging to the given public key.
// It uses a simple signature contract and this account has SignTx that
// returns no error, but at the same time adds no signature (it obviously can't
// do that, so CanSign() returns false for it). Use this account for Actor when
// simple signatures are needed to be collected.
pub fn fake_simple_account(k: &keys::PublicKey) -> wallet::Account {
    wallet::Account {
        address: k.address(),
        contract: wallet::Contract {
            script: k.get_verification_script(),
            ..Default::default()
        },
        ..Default::default()
    }
}

// FakeMultisigAccount creates a fake account belonging to the given "m out of
// len(pkeys)" account for the given set of keys. The account returned has SignTx
// that returns no error, but at the same time adds no signatures (it can't
// do that, so CanSign() returns false for it). Use this account for Actor when
// multisignature account needs to be added into a notary transaction, but you
// have no keys at all for it (if you have at least one (which usually is the
// case) ordinary multisig account works fine already).
pub fn fake_multisig_account(m: usize, pkeys: keys::PublicKeys) -> Result<wallet::Account, Box<dyn std::error::Error>> {
    let script = smartcontract::create_multi_sig_redeem_script(m, pkeys)?;
    Ok(wallet::Account {
        address: address::uint160_to_string(&hash::hash160(&script)),
        contract: wallet::Contract {
            script,
            ..Default::default()
        },
        ..Default::default()
    })
}

// FakeContractAccount creates a fake account belonging to some deployed contract.
// SignTx can be called on this account with no error, but at the same time it
// adds no signature or other data into the invocation script (it obviously can't
// do that, so CanSign() returns false for it). Use this account for Actor when
// one of the signers is a contract and it doesn't need a signature or you can
// provide it externally.
pub fn fake_contract_account(hash: util::Uint160) -> wallet::Account {
    wallet::Account {
        address: address::uint160_to_string(&hash),
        contract: wallet::Contract {
            deployed: true,
            ..Default::default()
        },
        ..Default::default()
    }
}
