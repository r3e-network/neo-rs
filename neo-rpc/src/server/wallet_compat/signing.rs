//! Wallet signing compatibility helpers.

use neo_error::{CoreError, CoreResult};
use neo_payloads::transaction::Transaction;
use neo_wallets::KeyPair;

/// C# `Neo.Wallets.Helper.Sign(IVerifiable, KeyPair, network)`: signs
/// the verifiable's network-prefixed sign data with the key.
pub(crate) fn sign_transaction_with_key(
    tx: &Transaction,
    key: &KeyPair,
    network: u32,
) -> CoreResult<Vec<u8>> {
    let data = neo_payloads::get_sign_data(tx, network)
        .map_err(|err| CoreError::other(err.to_string()))?;
    key.sign(&data)
        .map_err(|err| CoreError::other(err.to_string()))
}
