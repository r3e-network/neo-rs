use std::error::Error;
use std::fmt;
use std::sync::Arc;

use crate::core::state::Contract;
use crate::rpcclient::invoker::{self, RPCInvoke};
use crate::smartcontract::manifest::{self, NEP11StandardName, NEP17StandardName};
use crate::util::Uint160;
use crate::wallet::Token;

pub trait InfoClient: RPCInvoke {
    fn get_contract_state_by_hash(&self, hash: Uint160) -> Result<Contract, Box<dyn Error>>;
}

pub fn info(c: &dyn InfoClient, hash: Uint160) -> Result<Token, Box<dyn Error>> {
    let cs = c.get_contract_state_by_hash(hash)?;
    let mut standard = String::new();
    for st in &cs.manifest.supported_standards {
        if st == NEP17StandardName || st == NEP11StandardName {
            standard = st.clone();
            break;
        }
    }
    if standard.is_empty() {
        return Err(Box::new(fmt::Error::new(fmt::ErrorKind::Other, format!("contract {} is not NEP-11/NEP17", hash.to_string_le()))));
    }
    let b = Base::new(Arc::new(invoker::new(c, None)), hash);
    let symbol = b.symbol()?;
    let decimals = b.decimals()?;
    Ok(Token::new(hash, cs.manifest.name.clone(), symbol, decimals as i64, standard))
}
