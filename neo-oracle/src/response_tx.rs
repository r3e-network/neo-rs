//! Oracle response-transaction construction.
//!
//! Building the transaction that carries an [`OracleResponse`] to the oracle
//! native contract is oracle-domain logic, not host wiring: the quorum size,
//! the valid-until-block window, the signer/attribute/script/witness layout,
//! the verification round-trip that prices the transaction, the size-based fee
//! math and the budget downgrades all come from the reference
//! `create_response_tx`. This module owns that logic.
//!
//! The host supplies only what the oracle domain cannot know on its own:
//! snapshot queries through [`OracleSnapshot`], and the native oracle contract
//! plus verification execution through [`ResponseTxHost`]. Ledger persistence,
//! native-contract storage and the application engine stay in the host node.

use crate::error::OracleServiceError;
use crate::ports::{OracleSnapshot, ResponseTxHost};
use crate::request::OracleRequest;
use neo_crypto::ECPoint;
use neo_io::serializable::helper::{
    get_var_size, get_var_size_bytes, get_var_size_serializable_slice,
};
use neo_primitives::{OracleResponseCode, UInt160};
use neo_protocol::payloads::Transaction;
use neo_protocol::payloads::{
    OracleResponse, Signer, TransactionAttribute, Witness, get_fixed_script,
    oracle_response::MAX_RESULT_SIZE,
};
use neo_smart_contract::Contract;
use neo_smart_contract::helper::Helper;
use std::collections::HashMap;

/// Serialized size of a Neo transaction header (version, nonce,
/// system fee, network fee, valid-until-block, signer count and attribute
/// count fields), matching the reference `HEADER_SIZE`.
pub const HEADER_SIZE: usize = 25;

/// Builds the raw oracle response transaction, downgrading `response` in place
/// when the result is too large or the request budget is insufficient.
///
/// # Errors
///
/// Returns [`OracleServiceError::RequestTransactionNotFound`] when the request's
/// original transaction is not in the snapshot, [`OracleServiceError::BuildFailed`]
/// when no oracles are designated, the oracle contract is missing, its `verify`
/// method is absent, or verification does not halt, and
/// [`OracleServiceError::Processing`] for host/port failures.
pub fn create_response_tx(
    host: &dyn ResponseTxHost,
    snapshot: &dyn OracleSnapshot,
    request: &OracleRequest,
    response: &mut OracleResponse,
    oracle_nodes: &[ECPoint],
    use_current_height: bool,
) -> Result<Transaction, OracleServiceError> {
    let block_index = snapshot
        .transaction_block_index(&request.original_tx_id)?
        .ok_or(OracleServiceError::RequestTransactionNotFound)?;

    let n = oracle_nodes.len();
    if n == 0 {
        return Err(OracleServiceError::Processing(
            "oracle nodes not designated".to_string(),
        ));
    }
    let m = n - (n - 1) / 3;
    let oracle_sign_contract = Contract::create_multi_sig_contract(m, oracle_nodes);
    let oracle_contract_hash = host.oracle_contract_hash();

    let height = snapshot.ledger_height().saturating_sub(1);
    let max_vub = snapshot.max_valid_until_block_increment();
    let mut valid_until_block = block_index.saturating_add(max_vub);
    while use_current_height && valid_until_block <= height {
        valid_until_block = valid_until_block.saturating_add(max_vub);
    }

    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(response.id as u32);
    tx.set_valid_until_block(valid_until_block);
    tx.set_signers(vec![
        Signer::new(oracle_contract_hash, neo_primitives::WitnessScope::NONE),
        Signer::new(
            oracle_sign_contract.script_hash(),
            neo_primitives::WitnessScope::NONE,
        ),
    ]);
    tx.set_attributes(vec![TransactionAttribute::OracleResponse(response.clone())]);
    tx.set_script(get_fixed_script());
    tx.set_witnesses(vec![Witness::empty(), Witness::new()]);

    let witness_map: HashMap<UInt160, Witness> = vec![
        (
            oracle_sign_contract.script_hash(),
            Witness::new_with_scripts(Vec::new(), oracle_sign_contract.script.clone()),
        ),
        (oracle_contract_hash, Witness::empty()),
    ]
    .into_iter()
    .collect();

    // Neo transaction verification requires witnesses to be ordered exactly as
    // returned by GetScriptHashesForVerifying: the transaction sender (first
    // signer), the additional signers in order, then the contract hashes that
    // must verify the transaction. For an oracle response that is the oracle
    // multisig contract witness (holding the signatures) followed by the oracle
    // native contract witness (empty; verification happens inside the contract).
    let hashes = tx.signer_accounts();
    let witnesses = hashes
        .iter()
        .map(|hash| {
            witness_map
                .get(hash)
                .cloned()
                .unwrap_or_else(Witness::empty)
        })
        .collect::<Vec<_>>();
    tx.set_witnesses(witnesses);

    let oracle_contract = host
        .contract_state(snapshot, &oracle_contract_hash)?
        .ok_or_else(|| OracleServiceError::BuildFailed("oracle contract missing".to_string()))?;

    let mut abi = oracle_contract.manifest.abi.clone();
    let method = abi
        .get_method(
            neo_primitives::ContractBasicMethod::VERIFY,
            neo_primitives::ContractBasicMethod::VERIFY_P_COUNT,
        )
        .ok_or_else(|| OracleServiceError::BuildFailed("oracle verify method missing".to_string()))?
        .clone();

    let outcome = host.execute_verification(
        snapshot,
        &tx,
        &oracle_contract,
        &method,
        neo_primitives::CallFlags::NONE,
    )?;
    if outcome.state != neo_vm::VmState::HALT {
        return Err(OracleServiceError::BuildFailed(
            "oracle verify failed".to_string(),
        ));
    }
    tx.set_network_fee(outcome.fee_consumed);

    let exec_fee_factor = i64::from(snapshot.exec_fee_factor());
    let multi_sig_cost = Helper::multi_signature_contract_cost(m as i32, n as i32);
    let comp2 = exec_fee_factor * multi_sig_cost;
    tx.set_network_fee(tx.network_fee().saturating_add(comp2));

    let size_inv = 66 * m;
    let oracle_witness_size = Witness::empty().size();
    let mut size = HEADER_SIZE
        + get_var_size_serializable_slice(tx.signers())
        + get_var_size_bytes(tx.script())
        + get_var_size(hashes.len() as u64)
        + oracle_witness_size
        + get_var_size(size_inv as u64)
        + size_inv
        + get_var_size(oracle_sign_contract.script.len() as u64)
        + oracle_sign_contract.script.len();

    let fee_per_byte = snapshot.fee_per_byte();

    if response.result.len() > MAX_RESULT_SIZE {
        response.code = OracleResponseCode::ResponseTooLarge;
        response.result = Vec::new();
        tx.set_attributes(vec![TransactionAttribute::OracleResponse(response.clone())]);
    } else if tx.network_fee()
        + ((size + get_var_size_serializable_slice(tx.attributes())) as i64 * fee_per_byte)
        > request.gas_for_response
    {
        response.code = OracleResponseCode::InsufficientFunds;
        response.result = Vec::new();
        tx.set_attributes(vec![TransactionAttribute::OracleResponse(response.clone())]);
    }

    size += get_var_size_serializable_slice(tx.attributes());
    let comp3 = size as i64 * fee_per_byte;
    let final_network_fee = tx.network_fee().saturating_add(comp3);
    tx.set_network_fee(final_network_fee);
    tx.set_system_fee(request.gas_for_response - final_network_fee);

    Ok(tx)
}
