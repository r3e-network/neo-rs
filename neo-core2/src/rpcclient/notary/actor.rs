use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::sync::Mutex;

use crate::core::state::AppExecResult;
use crate::core::transaction::{Transaction, Attribute, Signer};
use crate::crypto::keys::Signature;
use crate::neorpc::result::Invoke;
use crate::network::payload::P2PNotaryRequest;
use crate::rpcclient::actor::{self, RPCActor as ActorRPCActor, SignerAccount, TransactionCheckerModifier, TransactionModifier};
use crate::rpcclient::invoker::Invoker;
use crate::util::Uint256;
use crate::vm::{self, opcode, vmstate};
use crate::wallet::Account;

#[derive(Debug, Clone)]
pub struct ErrFallbackAccepted;

impl fmt::Display for ErrFallbackAccepted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fallback transaction accepted")
    }
}

impl Error for ErrFallbackAccepted {}

pub struct Actor {
    actor: actor::Actor,
    fb_actor: actor::Actor,
    fb_script: Vec<u8>,
    reader: Arc<Mutex<ContractReader>>,
    sender: Arc<Account>,
    rpc: Arc<dyn RPCActor>,
}

pub struct ActorOptions {
    fb_attributes: Vec<Attribute>,
    fb_script: Vec<u8>,
    fb_signer: SignerAccount,
    main_attributes: Vec<Attribute>,
    main_checker_modifier: TransactionCheckerModifier,
    main_modifier: TransactionModifier,
}

pub trait RPCActor: ActorRPCActor {
    fn submit_p2p_notary_request(&self, req: &P2PNotaryRequest) -> Result<Uint256, Box<dyn Error>>;
}

pub fn new_default_actor_options(reader: Arc<Mutex<ContractReader>>, acc: Arc<Account>) -> ActorOptions {
    let fb_script = vec![opcode::RET as u8];
    let fb_signer = SignerAccount {
        signer: Signer {
            account: acc.contract.script_hash(),
            scopes: transaction::None,
        },
        account: acc.clone(),
    };
    let main_modifier = move |t: &mut Transaction| -> Result<(), Box<dyn Error>> {
        let nvb_delta = reader.lock().unwrap().get_max_not_valid_before_delta()?;
        t.valid_until_block += nvb_delta;
        Ok(())
    };
    let main_checker_modifier = move |r: &Invoke, t: &mut Transaction| -> Result<(), Box<dyn Error>> {
        actor::default_checker_modifier(r, t)?;
        main_modifier(t)
    };

    ActorOptions {
        fb_attributes: vec![],
        fb_script,
        fb_signer,
        main_attributes: vec![],
        main_checker_modifier,
        main_modifier,
    }
}

pub fn new_actor(
    c: Arc<dyn RPCActor>,
    signers: Vec<SignerAccount>,
    simple_acc: Arc<Account>,
) -> Result<Actor, Box<dyn Error>> {
    new_tuned_actor(c, signers, simple_acc, None)
}

pub fn new_tuned_actor(
    c: Arc<dyn RPCActor>,
    signers: Vec<SignerAccount>,
    simple_acc: Arc<Account>,
    opts: Option<ActorOptions>,
) -> Result<Actor, Box<dyn Error>> {
    if signers.is_empty() {
        return Err("at least one signer (sender) is required".into());
    }

    let mut n_keys = 0;
    for sa in &signers {
        if sa.account.contract.is_none() {
            return Err(format!("empty contract for account {}", sa.account.address).into());
        }
        if sa.account.contract.as_ref().unwrap().deployed {
            continue;
        }
        if vm::is_signature_contract(&sa.account.contract.as_ref().unwrap().script) {
            n_keys += 1;
            continue;
        }
        let (_, pubs, ok) = vm::parse_multi_sig_contract(&sa.account.contract.as_ref().unwrap().script);
        if !ok {
            return Err(format!("signer {} is not a contract- or signature-based", sa.account.address).into());
        }
        n_keys += pubs.len();
    }
    if n_keys > 255 {
        return Err("notary subsystem can't handle more than 255 signatures".into());
    }
    if simple_acc.contract.is_none() {
        return Err("bad simple account: no contract".into());
    }
    if !simple_acc.can_sign() {
        return Err("bad simple account: can't sign".into());
    }
    if !vm::is_signature_contract(&simple_acc.contract.as_ref().unwrap().script) && !simple_acc.contract.as_ref().unwrap().deployed {
        return Err("bad simple account: neither plain signature, nor contract".into());
    }

    let reader = Arc::new(Mutex::new(ContractReader::new(Invoker::new(c.clone(), None))));
    let opts = opts.unwrap_or_else(|| new_default_actor_options(reader.clone(), simple_acc.clone()));

    let notary_sa = SignerAccount {
        signer: Signer {
            account: Hash,
            scopes: transaction::None,
        },
        account: fake_contract_account(Hash),
    };

    let mut main_signers = signers.clone();
    main_signers.push(notary_sa.clone());

    let main_opts = actor::Options {
        attributes: vec![Attribute {
            r#type: transaction::NotaryAssistedT,
            value: transaction::NotaryAssisted { n_keys: n_keys as u8 },
        }],
        checker_modifier: opts.main_checker_modifier,
        modifier: opts.main_modifier,
    };
    main_opts.attributes.extend(opts.main_attributes.clone());

    let main_actor = actor::new_tuned(c.clone(), main_signers, main_opts)?;

    let fb_signers = vec![notary_sa.clone(), opts.fb_signer.clone()];
    let fb_opts = actor::Options {
        attributes: vec![
            Attribute {
                r#type: transaction::NotaryAssistedT,
                value: transaction::NotaryAssisted { n_keys: 0 },
            },
            Attribute {
                r#type: transaction::NotValidBeforeT,
                value: transaction::NotValidBefore {},
            },
            Attribute {
                r#type: transaction::ConflictsT,
                value: transaction::Conflicts {},
            },
        ],
    };
    fb_opts.attributes.extend(opts.fb_attributes.clone());

    let fb_actor = actor::new_tuned(c.clone(), fb_signers, fb_opts)?;

    Ok(Actor {
        actor: main_actor,
        fb_actor,
        fb_script: opts.fb_script,
        reader,
        sender: simple_acc,
        rpc: c,
    })
}

impl Actor {
    pub fn notarize(
        &self,
        main_tx: &Transaction,
        err: Result<(), Box<dyn Error>>,
    ) -> Result<(Uint256, Uint256, u32), Box<dyn Error>> {
        let fb_tx = self.fb_actor.make_unsigned_run(&self.fb_script, None)?;
        self.send_request(main_tx, &fb_tx)
    }

    pub fn send_request(
        &self,
        main_tx: &Transaction,
        fb_tx: &Transaction,
    ) -> Result<(Uint256, Uint256, u32), Box<dyn Error>> {
        let main_hash = main_tx.hash();
        let vub = main_tx.valid_until_block;

        if fb_tx.attributes.len() < 3 {
            return Err("invalid fallback: missing required attributes".into());
        }
        if fb_tx.attributes[1].r#type != transaction::NotValidBeforeT {
            return Err("invalid fallback: NotValidBefore is missing where expected".into());
        }
        if fb_tx.attributes[2].r#type != transaction::ConflictsT {
            return Err("invalid fallback: Conflicts is missing where expected".into());
        }

        let height = self.get_block_count()?;
        fb_tx.attributes[1].value = transaction::NotValidBefore { height: (height + vub) / 2 };
        fb_tx.attributes[2].value = transaction::Conflicts { hash: main_hash };
        fb_tx.valid_until_block = vub;

        self.fb_actor.sign(fb_tx)?;
        fb_tx.scripts[0].invocation_script = vec![opcode::PUSHDATA1 as u8, keys::SIGNATURE_LEN as u8];
        self.send_request_exactly(main_tx, fb_tx)
    }

    pub fn send_request_exactly(
        &self,
        main_tx: &Transaction,
        fb_tx: &Transaction,
    ) -> Result<(Uint256, Uint256, u32), Box<dyn Error>> {
        let fb_hash = fb_tx.hash();
        let main_hash = main_tx.hash();
        let vub = main_tx.valid_until_block;

        let req = P2PNotaryRequest {
            main_transaction: main_tx.clone(),
            fallback_transaction: fb_tx.clone(),
        };
        req.witness = transaction::Witness {
            invocation_script: vec![opcode::PUSHDATA1 as u8, keys::SIGNATURE_LEN as u8],
            verification_script: self.sender.get_verification_script(),
        };

        let actual_hash = self.rpc.submit_p2p_notary_request(&req)?;
        if actual_hash != fb_hash {
            return Err(format!(
                "sent and actual fallback tx hashes mismatch: {} vs {}",
                fb_hash.to_string_le(),
                actual_hash.to_string_le()
            )
            .into());
        }
        Ok((main_hash, fb_hash, vub))
    }

    pub fn wait(
        &self,
        main_hash: Uint256,
        fb_hash: Uint256,
        vub: u32,
        err: Result<(), Box<dyn Error>>,
    ) -> Result<AppExecResult, Box<dyn Error>> {
        if let Err(e) = err {
            if !e.to_string().to_lowercase().contains("already exists")
                && !e.to_string().to_lowercase().contains("already on chain")
            {
                return Err(e);
            }
        }
        self.wait_any(vub, main_hash, fb_hash)
    }

    pub fn wait_success(
        &self,
        main_hash: Uint256,
        fb_hash: Uint256,
        vub: u32,
        err: Result<(), Box<dyn Error>>,
    ) -> Result<AppExecResult, Box<dyn Error>> {
        let aer = self.wait(main_hash, fb_hash, vub, err)?;
        if aer.container != main_hash {
            return Err(Box::new(ErrFallbackAccepted));
        }
        if aer.vm_state != vmstate::Halt {
            return Err(format!("{}: {}", actor::ERR_EXEC_FAILED, aer.fault_exception).into());
        }
        Ok(aer)
    }
}
