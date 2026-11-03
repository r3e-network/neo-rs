use super::{FILTER_MAX_NEST, OracleServiceError};
use crate::cryptography::ECPoint;
use crate::persistence::DataCache;
use crate::smart_contract::Contract;
use crate::smart_contract::native::LedgerContract;
use crate::wallets::Wallet;
use neo_json::JToken;

pub(super) fn verify_oracle_signature(pubkey: &ECPoint, data: &[u8], signature: &[u8]) -> bool {
    if signature.len() != 64 {
        return false;
    }
    pubkey.verify_signature(data, signature).unwrap_or(false)
}

pub(super) fn filter_json(
    input: &str,
    filter: Option<&str>,
) -> Result<Vec<u8>, OracleServiceError> {
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
    !select_oracle_accounts(wallet, oracles).is_empty()
}

pub(super) fn select_oracle_accounts(
    wallet: &dyn Wallet,
    oracles: &[ECPoint],
) -> Vec<(crate::UInt160, ECPoint)> {
    oracles
        .iter()
        .filter_map(|oracle| {
            let script_hash = Contract::create_signature_contract(oracle.clone()).script_hash();
            wallet.get_account(&script_hash).and_then(|account| {
                (!account.is_locked() && account.has_key()).then(|| (script_hash, oracle.clone()))
            })
        })
        .collect()
}
