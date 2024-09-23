/*
Package actor provides a way to change chain state via RPC client.

This layer builds on top of the basic RPC client and [invoker] package, it
simplifies creating, signing and sending transactions to the network (since
that's the only way chain state is changed). It's generic enough to be used for
any contract that you may want to invoke and contract-specific functions can
build on top of it.
*/

use std::error::Error;
use std::fmt;

use crate::config::netmode;
use crate::core::state;
use crate::core::transaction;
use crate::neorpc::result;
use crate::rpcclient::invoker;
use crate::rpcclient::waiter;
use crate::util;
use crate::vm::vmstate;
use crate::wallet;

#[derive(Debug, Clone)]
pub struct ExecFailedError;

impl fmt::Display for ExecFailedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "execution failed")
    }
}

impl Error for ExecFailedError {}

pub trait RPCActor: invoker::RPCInvoke {
    fn calculate_network_fee(&self, tx: &transaction::Transaction) -> Result<i64, Box<dyn Error>>;
    fn get_block_count(&self) -> Result<u32, Box<dyn Error>>;
    fn get_version(&self) -> Result<result::Version, Box<dyn Error>>;
    fn send_raw_transaction(&self, tx: &transaction::Transaction) -> Result<util::Uint256, Box<dyn Error>>;
}

pub struct SignerAccount {
    pub signer: transaction::Signer,
    pub account: wallet::Account,
}

pub struct Actor {
    invoker: invoker::Invoker,
    waiter: waiter::Waiter,
    client: Box<dyn RPCActor>,
    opts: Options,
    signers: Vec<SignerAccount>,
    tx_signers: Vec<transaction::Signer>,
    version: result::Version,
}

pub struct Options {
    pub attributes: Vec<transaction::Attribute>,
    pub checker_modifier: Option<TransactionCheckerModifier>,
    pub modifier: Option<TransactionModifier>,
    pub waiter_config: waiter::Config,
}

impl Actor {
    pub fn new(ra: Box<dyn RPCActor>, signers: Vec<SignerAccount>) -> Result<Self, Box<dyn Error>> {
        if signers.is_empty() {
            return Err("at least one signer (sender) is required".into());
        }
        let mut inv_signers = Vec::with_capacity(signers.len());
        for signer in &signers {
            if signer.account.contract.is_none() {
                return Err(format!("empty contract for account {}", signer.account.address).into());
            }
            if !signer.account.contract.as_ref().unwrap().deployed && signer.account.contract.as_ref().unwrap().script_hash() != signer.signer.account {
                return Err(format!("signer account doesn't match script hash for signer {}", signer.account.address).into());
            }
            inv_signers.push(signer.signer.clone());
        }
        let inv = invoker::Invoker::new(ra.as_ref(), &inv_signers);
        let version = ra.get_version()?;
        Ok(Self {
            invoker: inv,
            waiter: waiter::Waiter::new(ra.as_ref(), &version),
            client: ra,
            opts: Self::new_default_options(),
            signers,
            tx_signers: inv_signers,
            version,
        })
    }

    pub fn new_simple(ra: Box<dyn RPCActor>, acc: wallet::Account) -> Result<Self, Box<dyn Error>> {
        Self::new(ra, vec![SignerAccount {
            signer: transaction::Signer {
                account: acc.contract.script_hash(),
                scopes: transaction::CalledByEntry,
            },
            account: acc,
        }])
    }

    pub fn new_default_options() -> Options {
        Options {
            attributes: Vec::new(),
            checker_modifier: Some(DefaultCheckerModifier),
            modifier: Some(DefaultModifier),
            waiter_config: waiter::Config::default(),
        }
    }

    pub fn new_tuned(ra: Box<dyn RPCActor>, signers: Vec<SignerAccount>, opts: Options) -> Result<Self, Box<dyn Error>> {
        let mut actor = Self::new(ra, signers)?;
        actor.opts.attributes = opts.attributes;
        if let Some(checker_modifier) = opts.checker_modifier {
            actor.opts.checker_modifier = Some(checker_modifier);
        }
        if let Some(modifier) = opts.modifier {
            actor.opts.modifier = Some(modifier);
        }
        actor.waiter = waiter::Waiter::new_custom(ra.as_ref(), &actor.version, opts.waiter_config);
        Ok(actor)
    }

    pub fn calculate_network_fee(&self, tx: &transaction::Transaction) -> Result<i64, Box<dyn Error>> {
        self.client.calculate_network_fee(tx)
    }

    pub fn get_block_count(&self) -> Result<u32, Box<dyn Error>> {
        self.client.get_block_count()
    }

    pub fn get_network(&self) -> netmode::Magic {
        self.version.protocol.network
    }

    pub fn get_version(&self) -> result::Version {
        self.version.clone()
    }

    pub fn send(&self, tx: &transaction::Transaction) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        let h = self.client.send_raw_transaction(tx)?;
        Ok((h, tx.valid_until_block))
    }

    pub fn sign(&self, tx: &mut transaction::Transaction) -> Result<(), Box<dyn Error>> {
        if tx.signers.len() != self.signers.len() {
            return Err("incorrect number of signers in the transaction".into());
        }
        for (i, signer) in self.signers.iter().enumerate() {
            if let Err(err) = signer.account.sign_tx(self.get_network(), tx) {
                if let Some(param_num) = signer.account.contract.as_ref().map(|c| c.parameters.len()) {
                    if param_num != 0 && signer.account.contract.as_ref().unwrap().deployed {
                        return Err(format!("failed to add contract-based witness for signer #{} ({}): {} parameters must be provided to construct invocation script", i, signer.account.address, param_num).into());
                    }
                }
                return Err(format!("failed to add witness for signer #{} ({}): account should be unlocked to add the signature. Store partially-signed transaction and then use 'wallet sign' command to cosign it", i, signer.account.address).into());
            }
        }
        Ok(())
    }

    pub fn sign_and_send(&self, tx: &mut transaction::Transaction) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        self.send_wrapper(tx, self.sign(tx))
    }

    fn send_wrapper(&self, tx: &mut transaction::Transaction, err: Result<(), Box<dyn Error>>) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        if let Err(e) = err {
            return Err(e);
        }
        self.send(tx)
    }

    pub fn send_call(&self, contract: util::Uint160, method: &str, params: &[impl Any]) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        self.send_wrapper(self.make_call(contract, method, params))
    }

    pub fn send_tuned_call(&self, contract: util::Uint160, method: &str, attrs: &[transaction::Attribute], tx_hook: TransactionCheckerModifier, params: &[impl Any]) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        self.send_wrapper(self.make_tuned_call(contract, method, attrs, tx_hook, params))
    }

    pub fn send_run(&self, script: &[u8]) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        self.send_wrapper(self.make_run(script))
    }

    pub fn send_tuned_run(&self, script: &[u8], attrs: &[transaction::Attribute], tx_hook: TransactionCheckerModifier) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        self.send_wrapper(self.make_tuned_run(script, attrs, tx_hook))
    }

    pub fn send_unchecked_run(&self, script: &[u8], sysfee: i64, attrs: &[transaction::Attribute], tx_hook: TransactionModifier) -> Result<(util::Uint256, u32), Box<dyn Error>> {
        self.send_wrapper(self.make_unchecked_run(script, sysfee, attrs, tx_hook))
    }

    pub fn signer_accounts(&self) -> Vec<SignerAccount> {
        self.signers.iter().map(|s| SignerAccount {
            signer: s.signer.clone(),
            account: s.account.clone(),
        }).collect()
    }

    pub fn sender(&self) -> util::Uint160 {
        self.tx_signers[0].account
    }

    pub fn wait_success(&self, h: util::Uint256, vub: u32, err: Result<(), Box<dyn Error>>) -> Result<state::AppExecResult, Box<dyn Error>> {
        let aer = self.waiter.wait(h, vub, err)?;
        if aer.vm_state != vmstate::Halt {
            return Err(format!("{}: {}", ExecFailedError, aer.fault_exception).into());
        }
        Ok(aer)
    }
}
