use super::super::{OracleService, OracleServiceError};
use crate::cryptography::ECPoint;
use crate::neo_io::serializable::helper::{
    get_var_size, get_var_size_bytes, get_var_size_serializable_slice,
};
use crate::neo_io::Serializable;
use crate::network::p2p::payloads::{
    oracle_response::MAX_RESULT_SIZE, OracleResponse, OracleResponseCode, Signer, Transaction,
    TransactionAttribute, Witness, HEADER_SIZE,
};
use crate::persistence::DataCache;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::contract_basic_method::ContractBasicMethod;
use crate::smart_contract::native::native_contract::NativeContract;
use crate::smart_contract::native::{
    ContractManagement, LedgerContract, OracleContract, PolicyContract,
};
use crate::smart_contract::{ApplicationEngine, Contract, TriggerType};
use crate::IVerifiable;
use crate::{UInt160, WitnessScope};
use std::collections::HashMap;
use std::sync::Arc;

impl OracleService {
    pub(in super::super) fn create_response_tx(
        &self,
        snapshot: &DataCache,
        request: &crate::smart_contract::native::OracleRequest,
        response: &mut OracleResponse,
        oracle_nodes: &[ECPoint],
        settings: &ProtocolSettings,
        use_current_height: bool,
    ) -> Result<Transaction, OracleServiceError> {
        let ledger = LedgerContract::new();
        let state = ledger
            .get_transaction_state(snapshot, &request.original_tx_id)
            .map_err(|err| OracleServiceError::Processing(err.to_string()))?
            .ok_or(OracleServiceError::RequestTransactionNotFound)?;

        let n = oracle_nodes.len();
        if n == 0 {
            return Err(OracleServiceError::Processing(
                "oracle nodes not designated".to_string(),
            ));
        }
        let m = n - (n - 1) / 3;
        let oracle_sign_contract = Contract::create_multi_sig_contract(m, oracle_nodes);

        let height = ledger.current_index(snapshot).unwrap_or(0);
        let max_vub = PolicyContract::new()
            .get_max_valid_until_block_increment_snapshot(snapshot, settings)
            .unwrap_or(settings.max_valid_until_block_increment);
        let mut valid_until_block = state.block_index().saturating_add(max_vub);
        while use_current_height && valid_until_block <= height {
            valid_until_block = valid_until_block.saturating_add(max_vub);
        }

        let mut tx = Transaction::new();
        tx.set_version(0);
        tx.set_nonce(response.id as u32);
        tx.set_valid_until_block(valid_until_block);
        tx.set_signers(vec![
            Signer::new(OracleContract::new().hash(), WitnessScope::NONE),
            Signer::new(oracle_sign_contract.script_hash(), WitnessScope::NONE),
        ]);
        tx.set_attributes(vec![TransactionAttribute::OracleResponse(response.clone())]);
        tx.set_script(OracleResponse::get_fixed_script());
        tx.set_witnesses(vec![Witness::empty(), Witness::new()]);

        let witness_map: HashMap<UInt160, Witness> = vec![
            (
                oracle_sign_contract.script_hash(),
                Witness::new_with_scripts(Vec::new(), oracle_sign_contract.script.clone()),
            ),
            (OracleContract::new().hash(), Witness::empty()),
        ]
        .into_iter()
        .collect();

        let hashes = tx.get_script_hashes_for_verifying(snapshot);
        let mut witnesses = Vec::with_capacity(hashes.len());
        for hash in hashes.iter() {
            let witness = witness_map
                .get(hash)
                .cloned()
                .unwrap_or_else(Witness::empty);
            witnesses.push(witness);
        }
        tx.set_witnesses(witnesses);

        let oracle_contract =
            ContractManagement::get_contract_from_snapshot(snapshot, &OracleContract::new().hash())
                .map_err(|err| OracleServiceError::Processing(err.to_string()))?
                .ok_or_else(|| {
                    OracleServiceError::BuildFailed("oracle contract missing".to_string())
                })?;

        let mut engine = ApplicationEngine::new(
            TriggerType::Verification,
            Some(Arc::new(tx.clone())),
            Arc::new(snapshot.clone()),
            None,
            settings.clone(),
            crate::smart_contract::helper::Helper::MAX_VERIFICATION_GAS,
            None,
        )
        .map_err(|err| OracleServiceError::Processing(err.to_string()))?;

        let mut abi = oracle_contract.manifest.abi.clone();
        let method = abi
            .get_method(
                ContractBasicMethod::VERIFY,
                ContractBasicMethod::VERIFY_P_COUNT,
            )
            .ok_or_else(|| {
                OracleServiceError::BuildFailed("oracle verify method missing".to_string())
            })?
            .clone();

        engine
            .load_contract_method(oracle_contract.clone(), method, CallFlags::NONE)
            .map_err(|err| OracleServiceError::Processing(err.to_string()))?;
        engine
            .execute()
            .map_err(|err| OracleServiceError::Processing(err.to_string()))?;
        if engine.state() != neo_vm::VMState::HALT {
            return Err(OracleServiceError::BuildFailed(
                "oracle verify failed".to_string(),
            ));
        }

        tx.set_network_fee(engine.fee_consumed());

        let exec_fee_factor = PolicyContract::new()
            .get_exec_fee_factor_snapshot(snapshot, settings, height)
            .unwrap_or(PolicyContract::DEFAULT_EXEC_FEE_FACTOR)
            as i64;
        let network_fee = exec_fee_factor
            * crate::smart_contract::helper::Helper::multi_signature_contract_cost(
                m as i32, n as i32,
            );
        tx.set_network_fee(tx.network_fee().saturating_add(network_fee));

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

        let fee_per_byte = PolicyContract::new()
            .get_fee_per_byte_snapshot(snapshot)
            .unwrap_or(PolicyContract::DEFAULT_FEE_PER_BYTE as i64);

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
        let final_network_fee = tx.network_fee().saturating_add(size as i64 * fee_per_byte);
        tx.set_network_fee(final_network_fee);
        tx.set_system_fee(request.gas_for_response - final_network_fee);

        Ok(tx)
    }
}
