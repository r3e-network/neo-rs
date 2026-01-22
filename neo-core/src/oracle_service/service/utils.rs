use super::{OracleServiceError, FILTER_MAX_NEST};
use crate::cryptography::ECPoint;
use crate::network::p2p::helper::get_sign_data_vec;
use crate::network::p2p::payloads::Transaction;
use crate::persistence::DataCache;
use crate::smart_contract::native::LedgerContract;
use crate::smart_contract::Contract;
use crate::wallets::{KeyPair, Wallet};
use neo_json::JToken;

pub(super) fn sign_transaction(tx: &Transaction, key: &KeyPair, network: u32) -> Vec<u8> {
    let Ok(data) = get_sign_data_vec(tx, network) else {
        return Vec::new();
    };
    key.sign(&data).unwrap_or_default()
}

pub(super) fn verify_oracle_signature(pubkey: &ECPoint, data: &[u8], signature: &[u8]) -> bool {
    if signature.len() != 64 {
        return false;
    }
    pubkey.verify_signature(data, signature).unwrap_or(false)
}

pub(super) fn filter_json(input: &str, filter: Option<&str>) -> Result<Vec<u8>, OracleServiceError> {
    if filter.map(|value| value.is_empty()).unwrap_or(true) {
        return Ok(input.as_bytes().to_vec());
    }

    let token = JToken::parse(input, FILTER_MAX_NEST)
        .map_err(|err| OracleServiceError::Processing(err.to_string()))?;
    let array = token
        .json_path(filter.unwrap_or(""))
        .map_err(|err| OracleServiceError::Processing(err.to_string()))?;
    let token = JToken::from(array);
    token
        .to_byte_array(false)
        .map_err(|err| OracleServiceError::Processing(err.to_string()))
}

pub(super) fn ledger_height(snapshot: &DataCache) -> u32 {
    LedgerContract::new()
        .current_index(snapshot)
        .unwrap_or(0)
        .saturating_add(1)
}

pub(super) fn wallet_has_oracle_account(wallet: &dyn Wallet, oracles: &[ECPoint]) -> bool {
    wallet.get_accounts().iter().any(|account| {
        if !account.has_key() || account.is_locked() {
            return false;
        }
        let Some(key) = account.get_key() else {
            return false;
        };
        let Ok(pubkey) = key.get_public_key_point() else {
            return false;
        };
        oracles.iter().any(|oracle| oracle == &pubkey)
    })
}

pub(super) fn select_oracle_key(wallet: &dyn Wallet, oracles: &[ECPoint]) -> Option<KeyPair> {
    for oracle in oracles {
        let script_hash = Contract::create_signature_contract(oracle.clone()).script_hash();
        let Some(account) = wallet.get_account(&script_hash) else {
            continue;
        };
        if !account.has_key() || account.is_locked() {
            continue;
        }
        let Some(key) = account.get_key() else {
            continue;
        };
        return Some(key);
    }
    None
}
