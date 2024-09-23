use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::error::Error;
use serde::{Serialize, Deserialize};

use crate::crypto::keys::{ScryptParams, NEP2ScryptParams};
use crate::encoding::address;
use crate::util::Uint160;
use crate::vm;

const WALLET_VERSION: &str = "1.0";

#[derive(Debug, thiserror::Error)]
#[error("path is empty")]
pub struct EmptyPathError;

#[derive(Serialize, Deserialize)]
pub struct Wallet {
    version: String,
    accounts: Vec<Account>,
    scrypt: ScryptParams,
    extra: Extra,
    #[serde(skip)]
    path: Option<PathBuf>,
}

#[derive(Serialize, Deserialize)]
pub struct Extra {
    tokens: Vec<Token>,
}

impl Wallet {
    pub fn new(location: &str) -> Result<Self, Box<dyn Error>> {
        let file = File::create(location)?;
        Ok(Self::new_wallet(Some(file)))
    }

    pub fn new_in_memory() -> Self {
        Self::new_wallet(None)
    }

    pub fn from_file(path: &str) -> Result<Self, Box<dyn Error>> {
        let mut file = File::open(path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;

        let mut wallet: Wallet = serde_json::from_str(&contents)?;
        wallet.path = Some(PathBuf::from(path));
        Ok(wallet)
    }

    pub fn from_bytes(wallet: &[u8]) -> Result<Self, Box<dyn Error>> {
        Ok(serde_json::from_slice(wallet)?)
    }

    fn new_wallet(file: Option<File>) -> Self {
        let path = file.as_ref().map(|f| PathBuf::from(f.path()));
        Self {
            version: WALLET_VERSION.to_string(),
            accounts: Vec::new(),
            scrypt: NEP2ScryptParams(),
            extra: Extra { tokens: Vec::new() },
            path,
        }
    }

    pub fn create_account(&mut self, name: &str, passphrase: &str) -> Result<(), Box<dyn Error>> {
        let mut acc = Account::new()?;
        acc.set_label(name);
        acc.encrypt(passphrase, &self.scrypt)?;
        self.add_account(acc);
        self.save()
    }

    pub fn add_account(&mut self, acc: Account) {
        self.accounts.push(acc);
    }

    pub fn remove_account(&mut self, addr: &str) -> Result<(), Box<dyn Error>> {
        if let Some(pos) = self.accounts.iter().position(|acc| acc.address() == addr) {
            self.accounts.remove(pos);
            Ok(())
        } else {
            Err("account wasn't found".into())
        }
    }

    pub fn add_token(&mut self, tok: Token) {
        self.extra.tokens.push(tok);
    }

    pub fn remove_token(&mut self, h: &Uint160) -> Result<(), Box<dyn Error>> {
        if let Some(pos) = self.extra.tokens.iter().position(|tok| tok.hash() == h) {
            self.extra.tokens.remove(pos);
            Ok(())
        } else {
            Err("token wasn't found".into())
        }
    }

    pub fn path(&self) -> Option<&PathBuf> {
        self.path.as_ref()
    }

    pub fn set_path(&mut self, path: PathBuf) {
        self.path = Some(path);
    }

    pub fn save(&self) -> Result<(), Box<dyn Error>> {
        let data = serde_json::to_vec(self)?;
        self.write_raw(&data)
    }

    pub fn save_pretty(&self) -> Result<(), Box<dyn Error>> {
        let data = serde_json::to_vec_pretty(self)?;
        self.write_raw(&data)
    }

    fn write_raw(&self, data: &[u8]) -> Result<(), Box<dyn Error>> {
        let path = self.path.as_ref().ok_or(EmptyPathError)?;
        let mut file = OpenOptions::new().write(true).truncate(true).open(path)?;
        file.write_all(data)?;
        Ok(())
    }

    pub fn json(&self) -> Result<String, Box<dyn Error>> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn close(&mut self) {
        for acc in &mut self.accounts {
            acc.close();
        }
    }

    pub fn get_account(&self, h: &Uint160) -> Option<&Account> {
        let addr = address::uint160_to_string(h);
        self.accounts.iter().find(|acc| acc.address() == addr)
    }

    pub fn get_change_address(&self) -> Uint160 {
        self.accounts.iter()
            .find(|acc| acc.is_default() && acc.contract().map_or(false, |c| vm::is_signature_contract(&c.script())))
            .or_else(|| self.accounts.iter().find(|acc| acc.contract().map_or(false, |c| vm::is_signature_contract(&c.script()))))
            .and_then(|acc| acc.contract().map(|c| c.script_hash()))
            .unwrap_or_default()
    }
}
