use std::error::Error;
use std::sync::Arc;

use neo_crypto::{keys::{PrivateKey, PublicKey, ScryptParams}, hash::{Hash160, Hashable}};
use neo_types::{Address, Uint160};
use neo_vm::{opcode, script::Script};
use neo_transaction::{Transaction, Witness, Signer};
use neo_smartcontract::{ParamType, ContractParam};

#[derive(Clone, Debug)]
pub struct Account {
    private_key: Option<Arc<PrivateKey>>,
    script_hash: Uint160,
    address: Address,
    encrypted_wif: Option<String>,
    label: Option<String>,
    contract: Option<Contract>,
    locked: bool,
    is_default: bool,
}

#[derive(Clone, Debug)]
pub struct Contract {
    script: Script,
    parameters: Vec<ContractParam>,
    deployed: bool,
    invocation_builder: Option<Box<dyn Fn(&Transaction) -> Result<Vec<u8>, Box<dyn Error>> + Send + Sync>>,
}

impl Contract {
    pub fn script_hash(&self) -> Uint160 {
        self.script.hash160()
    }
}

impl Account {
    pub fn new() -> Result<Self, Box<dyn Error>> {
        let private_key = PrivateKey::new()?;
        Ok(Self::from_private_key(private_key))
    }

    pub fn new_contract_account(hash: Uint160, args: Vec<Box<dyn Any>>) -> Self {
        Self {
            address: Address::from(hash),
            contract: Some(Contract {
                parameters: vec![ContractParam::default(); args.len()],
                deployed: true,
                invocation_builder: Some(Box::new(move |_tx: &Transaction| {
                    let mut writer = io::Cursor::new(Vec::new());
                    for arg in &args {
                        // Implement emit::Any equivalent
                        // This is a placeholder and needs to be implemented
                        writer.write_all(&[])?;
                    }
                    Ok(writer.into_inner())
                })),
                script: Script::default(),
            }),
            ..Default::default()
        }
    }

    pub fn sign_tx(&self, net: u32, t: &mut Transaction) -> Result<(), Box<dyn Error>> {
        if self.locked {
            return Err("account is locked".into());
        }
        let contract = self.contract.as_ref().ok_or("account has no contract")?;
        let pos = t.signers.iter().position(|s| s.account == self.script_hash())
            .ok_or("transaction is not signed by this account")?;
        
        if t.scripts.len() < pos {
            return Err("transaction is not yet signed by the previous signer".into());
        }
        if t.scripts.len() == pos {
            t.scripts.push(Witness {
                verification_script: contract.script.clone(),
                ..Default::default()
            });
        }
        
        if contract.deployed && contract.invocation_builder.is_some() {
            let invoc = (contract.invocation_builder.as_ref().unwrap())(t)?;
            t.scripts[pos].invocation_script = invoc;
            return Ok(());
        }
        
        if contract.parameters.is_empty() {
            return Ok(());
        }
        
        let private_key = self.private_key.as_ref().ok_or("account key is not available (need to decrypt?)")?;
        
        if contract.parameters.len() == 1 && !t.scripts[pos].invocation_script.is_empty() {
            t.scripts[pos].invocation_script.clear();
        }
        
        let signature = private_key.sign_hashable(net, t);
        t.scripts[pos].invocation_script.push(opcode::PUSHDATA1);
        t.scripts[pos].invocation_script.extend_from_slice(&(signature.len() as u8).to_le_bytes());
        t.scripts[pos].invocation_script.extend_from_slice(&signature);
        
        Ok(())
    }

    pub fn sign_hashable(&self, net: u32, item: &dyn Hashable) -> Option<Vec<u8>> {
        if !self.can_sign() {
            return None;
        }
        Some(self.private_key.as_ref().unwrap().sign_hashable(net, item))
    }

    pub fn can_sign(&self) -> bool {
        !self.locked && self.private_key.is_some()
    }

    pub fn get_verification_script(&self) -> Script {
        self.contract.as_ref().map_or_else(
            || self.private_key.as_ref().unwrap().public_key().get_verification_script(),
            |c| c.script.clone()
        )
    }

    pub fn decrypt(&mut self, passphrase: &str, scrypt: ScryptParams) -> Result<(), Box<dyn Error>> {
        let encrypted_wif = self.encrypted_wif.as_ref().ok_or("no encrypted wif in the account")?;
        let private_key = PrivateKey::nep2_decrypt(encrypted_wif, passphrase, &scrypt)?;
        self.private_key = Some(Arc::new(private_key));
        Ok(())
    }

    pub fn encrypt(&mut self, passphrase: &str, scrypt: ScryptParams) -> Result<(), Box<dyn Error>> {
        let wif = PrivateKey::nep2_encrypt(self.private_key.as_ref().unwrap(), passphrase, &scrypt)?;
        self.encrypted_wif = Some(wif);
        Ok(())
    }

    pub fn private_key(&self) -> Option<Arc<PrivateKey>> {
        self.private_key.clone()
    }

    pub fn public_key(&self) -> Option<PublicKey> {
        self.private_key.as_ref().map(|pk| pk.public_key())
    }

    pub fn script_hash(&self) -> Uint160 {
        self.script_hash
    }

    pub fn close(&mut self) {
        self.private_key = None;
    }

    pub fn from_wif(wif: &str) -> Result<Self, Box<dyn Error>> {
        let private_key = PrivateKey::from_wif(wif)?;
        Ok(Self::from_private_key(private_key))
    }

    pub fn from_encrypted_wif(wif: &str, pass: &str, scrypt: ScryptParams) -> Result<Self, Box<dyn Error>> {
        let private_key = PrivateKey::nep2_decrypt(wif, pass, &scrypt)?;
        let mut account = Self::from_private_key(private_key);
        account.encrypted_wif = Some(wif.to_string());
        Ok(account)
    }

    pub fn convert_multisig(&mut self, m: usize, pubs: &[PublicKey]) -> Result<(), Box<dyn Error>> {
        if self.locked {
            return Err("account is locked".into());
        }
        let acc_key = self.private_key.as_ref().ok_or("account key is not available (need to decrypt?)")?.public_key();
        self.convert_multisig_encrypted(&acc_key, m, pubs)
    }

    pub fn convert_multisig_encrypted(&mut self, acc_key: &PublicKey, m: usize, pubs: &[PublicKey]) -> Result<(), Box<dyn Error>> {
        if !pubs.iter().any(|p| p == acc_key) {
            return Err("own public key was not found among multisig keys".into());
        }

        let script = Script::create_multisig_redeem_script(m, pubs)?;
        self.script_hash = script.hash160();
        self.address = Address::from(self.script_hash);
        self.contract = Some(Contract {
            script,
            parameters: get_contract_params(m),
            deployed: false,
            invocation_builder: None,
        });

        Ok(())
    }

    pub fn from_private_key(p: PrivateKey) -> Self {
        let public_key = p.public_key();
        let script_hash = public_key.get_script_hash();
        
        Self {
            private_key: Some(Arc::new(p)),
            script_hash,
            address: Address::from(script_hash),
            encrypted_wif: None,
            label: None,
            contract: Some(Contract {
                script: public_key.get_verification_script(),
                parameters: get_contract_params(1),
                deployed: false,
                invocation_builder: None,
            }),
            locked: false,
            is_default: false,
        }
    }
}

fn get_contract_params(n: usize) -> Vec<ContractParam> {
    (0..n).map(|i| ContractParam {
        name: format!("parameter{}", i),
        param_type: ParamType::Signature,
    }).collect()
}
