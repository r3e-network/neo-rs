use std::cmp::Ordering;
use std::panic;
use std::slice;

use bytes::Bytes;
use require::require;
use crate::config::netmode;
use crate::core::transaction;
use crate::crypto::hash;
use crate::crypto::keys;
use crate::encoding::address;
use crate::io;
use crate::util;
use crate::vm;
use crate::vm::emit;
use crate::vm::opcode;
use crate::wallet;

// Signer is a generic interface which can be either a simple- or multi-signature signer.
pub trait Signer {
    // Script returns a signer verification script.
    fn script(&self) -> Bytes;
    // ScriptHash returns a signer script hash.
    fn script_hash(&self) -> util::Uint160;
    // SignHashable returns an invocation script for signing an item.
    fn sign_hashable(&self, magic: u32, item: &dyn hash::Hashable) -> Bytes;
    // SignTx signs a transaction.
    fn sign_tx(&self, magic: netmode::Magic, tx: &mut transaction::Transaction) -> Result<(), String>;
}

// SingleSigner is a generic interface for a simple one-signature signer.
pub trait SingleSigner: Signer {
    // Account returns the underlying account which can be used to
    // get a public key and/or sign arbitrary things.
    fn account(&self) -> &wallet::Account;
}

// MultiSigner is an interface for multisignature signing account.
pub trait MultiSigner: Signer {
    // Single returns a simple-signature signer for the n-th account in a list.
    fn single(&self, n: usize) -> Box<dyn SingleSigner>;
}

// signer represents a simple-signature signer.
pub struct SignerImpl(wallet::Account);

// multiSigner represents a single multi-signature signer consisting of the provided accounts.
pub struct MultiSignerImpl {
    accounts: Vec<wallet::Account>,
    m: usize,
}

// NewSingleSigner creates a [SingleSigner] from the provided account. It has
// just one key, see [NewMultiSigner] for multisignature accounts.
pub fn new_single_signer(acc: wallet::Account) -> Box<dyn SingleSigner> {
    if !vm::is_signature_contract(&acc.contract.script) {
        panic!("account must have simple-signature verification script");
    }
    Box::new(SignerImpl(acc))
}

// Implementing Signer trait for SignerImpl
impl Signer for SignerImpl {
    fn script(&self) -> Bytes {
        self.0.contract.script.clone()
    }

    fn script_hash(&self) -> util::Uint160 {
        self.0.contract.script_hash()
    }

    fn sign_hashable(&self, magic: u32, item: &dyn hash::Hashable) -> Bytes {
        let mut script = vec![opcode::PUSHDATA1 as u8, keys::SIGNATURE_LEN];
        script.extend(self.0.sign_hashable(netmode::Magic(magic), item));
        script.into()
    }

    fn sign_tx(&self, magic: netmode::Magic, tx: &mut transaction::Transaction) -> Result<(), String> {
        self.0.sign_tx(magic, tx)
    }
}

// Implementing SingleSigner trait for SignerImpl
impl SingleSigner for SignerImpl {
    fn account(&self) -> &wallet::Account {
        &self.0
    }
}

// NewMultiSigner returns a multi-signature signer for the provided account.
// It must contain at least as many accounts as needed to sign the script.
pub fn new_multi_signer(accs: Vec<wallet::Account>) -> Box<dyn MultiSigner> {
    if accs.is_empty() {
        panic!("empty account list");
    }
    let script = &accs[0].contract.script;
    let (m, _, ok) = vm::parse_multi_sig_contract(script);
    if !ok {
        panic!("all accounts must have multi-signature verification script");
    }
    if accs.len() < m {
        panic!("verification script requires {} signatures, but only {} accounts were provided", m, accs.len());
    }
    let mut sorted_accs = accs.clone();
    sorted_accs.sort_by(|a, b| a.public_key().cmp(&b.public_key()));
    for acc in &sorted_accs {
        if script != &acc.contract.script {
            panic!("all accounts must have equal verification script");
        }
    }

    Box::new(MultiSignerImpl { accounts: sorted_accs, m })
}

// Implementing Signer trait for MultiSignerImpl
impl Signer for MultiSignerImpl {
    fn script(&self) -> Bytes {
        self.accounts[0].contract.script.clone()
    }

    fn script_hash(&self) -> util::Uint160 {
        self.accounts[0].contract.script_hash()
    }

    fn sign_hashable(&self, magic: u32, item: &dyn hash::Hashable) -> Bytes {
        let mut script = vec![];
        for i in 0..self.m {
            let sign = self.accounts[i].sign_hashable(netmode::Magic(magic), item);
            script.push(opcode::PUSHDATA1 as u8);
            script.push(keys::SIGNATURE_LEN);
            script.extend(sign);
        }
        script.into()
    }

    fn sign_tx(&self, magic: netmode::Magic, tx: &mut transaction::Transaction) -> Result<(), String> {
        let invoc = self.sign_hashable(magic as u32, tx);
        let verif = self.script();
        for script in &mut tx.scripts {
            if script.verification_script == verif {
                script.invocation_script = invoc.clone();
                return Ok(());
            }
        }
        tx.scripts.push(transaction::Witness {
            invocation_script: invoc,
            verification_script: verif,
        });
        Ok(())
    }
}

// Implementing MultiSigner trait for MultiSignerImpl
impl MultiSigner for MultiSignerImpl {
    fn single(&self, n: usize) -> Box<dyn SingleSigner> {
        if self.accounts.len() <= n {
            panic!("invalid index");
        }
        new_single_signer(wallet::new_account_from_private_key(self.accounts[n].private_key()))
    }
}

pub fn check_multi_signer(t: &mut dyn testing::TB, s: &dyn Signer) {
    let ms = s.as_any().downcast_ref::<MultiSignerImpl>().expect("expected to be a multi-signer");

    let accs = &ms.accounts;
    require!(accs.len() > 0, "empty multi-signer");

    let m = accs[0].contract.parameters.len();
    require!(m <= accs.len(), "honest not count is too big for a multi-signer");

    let h = accs[0].contract.script_hash();
    for i in 1..accs.len() {
        require!(m == accs[i].contract.parameters.len(), "inconsistent multi-signer accounts");
        require!(h == accs[i].contract.script_hash(), "inconsistent multi-signer accounts");
    }
}

pub struct ContractSigner(wallet::Account);

// NewContractSigner returns a contract signer for the provided contract hash.
// get_inv_params must return params to be used as invocation script for contract-based witness.
pub fn new_contract_signer(h: util::Uint160, get_inv_params: fn(&transaction::Transaction) -> Vec<any>) -> Box<dyn SingleSigner> {
    Box::new(ContractSigner {
        address: address::uint160_to_string(h),
        contract: wallet::Contract {
            deployed: true,
            invocation_builder: Some(Box::new(move |tx: &transaction::Transaction| -> Result<Bytes, String> {
                let params = get_inv_params(tx);
                let mut script = io::BufBinWriter::new();
                for param in params {
                    emit::any(&mut script, param);
                }
                if script.err().is_some() {
                    return Err(script.err().unwrap().to_string());
                }
                Ok(script.bytes())
            })),
        },
    })
}

// Implementing Signer trait for ContractSigner
impl Signer for ContractSigner {
    fn script(&self) -> Bytes {
        vec![].into()
    }

    fn script_hash(&self) -> util::Uint160 {
        self.account().script_hash()
    }

    fn sign_hashable(&self, _: u32, _: &dyn hash::Hashable) -> Bytes {
        panic!("not supported")
    }

    fn sign_tx(&self, magic: netmode::Magic, tx: &mut transaction::Transaction) -> Result<(), String> {
        self.account().sign_tx(magic, tx)
    }
}

// Implementing SingleSigner trait for ContractSigner
impl SingleSigner for ContractSigner {
    fn account(&self) -> &wallet::Account {
        &self.0
    }
}
