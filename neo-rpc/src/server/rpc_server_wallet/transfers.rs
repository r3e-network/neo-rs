use neo_crypto::{ECCurve, ECPoint};
use neo_execution::contract::Contract;
use neo_execution::contract_parameters_context::ContractParametersContext;
use neo_execution::Nep17MetadataReaderImpl;
use neo_io::Serializable;
use neo_native_contracts::{LedgerContract, PolicyContract};
use neo_payloads::conflicts::Conflicts;
use neo_payloads::signer::Signer;
use neo_payloads::transaction::Transaction;
use neo_payloads::transaction_attribute::TransactionAttribute;
use neo_primitives::{BigDecimal, UInt160, WitnessScope};
use neo_storage::persistence::DataCache;
use neo_vm_rs::OpCode;
use neo_wallets::{AssetDescriptor, TransferOutput, Wallet as CoreWallet};
use num_traits::ToPrimitive;
use serde_json::Value;
use std::str::FromStr;
use std::sync::Arc;

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::{
    expect_string_param, internal_error, invalid_params, parse_uint160, parse_uint256,
};
use crate::server::rpc_relay;
use crate::server::rpc_server::RpcServer;
use crate::server::wallet_compat;

use super::support::{TransferParamLayout, TransferRequest, signature_contract_pubkey};
use super::{INVALID_OPERATION_HRESULT, RpcServerWallet};

impl RpcServerWallet {
    pub(super) fn send_from(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let _ = Self::require_wallet(server)?;
        let request = Self::parse_transfer_request(
            server,
            params,
            TransferParamLayout {
                method: "sendfrom",
                from_index: Some(1),
                to_index: 2,
                amount_index: 3,
                signers_index: 4,
            },
        )?;
        Self::process_transfer(
            server,
            request.asset,
            request.from,
            request.to,
            request.amount,
            request.signers.as_deref(),
        )
        .map_err(Self::send_from_transfer_error)
    }

    pub(super) fn send_to_address(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let _ = Self::require_wallet(server)?;
        let request = Self::parse_transfer_request(
            server,
            params,
            TransferParamLayout {
                method: "sendtoaddress",
                from_index: None,
                to_index: 1,
                amount_index: 2,
                signers_index: 3,
            },
        )?;
        Self::process_transfer(
            server,
            request.asset,
            request.from,
            request.to,
            request.amount,
            request.signers.as_deref(),
        )
        .map_err(Self::invalid_operation_transfer_error)
    }

    pub(super) fn send_many(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let wallet = Self::require_wallet(server)?;
        if params.is_empty() {
            return Err(invalid_params("sendmany requires at least one argument"));
        }
        let mut from: Option<UInt160> = None;
        let mut index = 0;
        if params[0].is_string() {
            from = Some(Self::parse_script_hash(
                server,
                &expect_string_param(params, 0, "sendmany")?,
            )?);
            index = 1;
        }

        let outputs_value = params.get(index).cloned().unwrap_or(Value::Null);
        let outputs_array = outputs_value
            .as_array()
            .ok_or_else(|| invalid_params(format!("Invalid 'to' parameter: {outputs_value}")))?;
        if outputs_array.is_empty() {
            return Err(invalid_params("Argument 'to' can't be empty."));
        }

        let signers = Self::parse_optional_signers(server, params, index + 1)?;

        let store = server.system().store_cache();
        let descriptor_snapshot = Arc::new(store.data_cache().clone());
        let reader = Nep17MetadataReaderImpl::new(
            Arc::clone(&descriptor_snapshot),
            server.system().settings().as_ref().clone(),
        );
        let descriptor_cache = |asset: &UInt160| {
            AssetDescriptor::new(
                Arc::clone(&descriptor_snapshot),
                &reader,
                *asset,
            )
            .map_err(|err| neo_error::CoreError::other(err.to_string()))
        };

        let transfers = outputs_array
            .iter()
            .enumerate()
            .map(|(i, entry)| Self::parse_send_many_output(server, &descriptor_cache, i, entry))
            .collect::<Result<Vec<_>, _>>()?;

        Self::build_and_relay(server, &wallet, &transfers, from, signers.as_deref())
            .map_err(Self::invalid_operation_transfer_error)
    }

    pub(super) fn cancel_transaction(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let txid = parse_uint256(params, 0, "canceltransaction")?;
        let signers_value = params
            .get(1)
            .ok_or_else(|| invalid_params("canceltransaction requires signers"))?;
        let signers_array = signers_value
            .as_array()
            .ok_or_else(|| invalid_params("canceltransaction signers must be an array"))?;
        if signers_array.is_empty() {
            return Err(RpcException::from(
                RpcError::bad_request().with_data("No signer."),
            ));
        }

        let signers = Self::parse_signer_array(
            server,
            signers_array,
            "canceltransaction signers must be strings",
            WitnessScope::NONE,
        )?;

        let wallet = Self::require_wallet(server)?;
        let store = server.system().store_cache();
        let ledger = LedgerContract::new();
        let snapshot = store.data_cache();
        if ledger
            .get_transaction_state(snapshot, &txid)
            .map_err(|err| internal_error(err.to_string()))?
            .is_some()
        {
            return Err(RpcException::from(
                RpcError::already_exists()
                    .with_data("This tx is already confirmed, can't be cancelled."),
            ));
        }

        let conflict_attr = TransactionAttribute::Conflicts(Conflicts::new(txid));
        let script = vec![OpCode::RET.byte()];
        let snapshot_arc = Arc::new(snapshot.clone());
        let settings = server.system().settings();
        let mut tx = wallet_compat::make_transaction(
            wallet.as_ref(),
            snapshot_arc.as_ref(),
            &settings,
            &script,
            Some(signers[0].account),
            &signers,
            std::slice::from_ref(&conflict_attr),
            server.settings().max_gas_invoke,
        )
        .map_err(Self::wallet_compat_failure)?;

        if let Some(conflict_tx) = server.system().mempool().get(&txid) {
            let bumped = tx
                .network_fee()
                .max(conflict_tx.transaction.network_fee())
                .saturating_add(1);
            tx.set_network_fee(bumped);
        } else if let Some(extra_fee) = params.get(2).and_then(Value::as_str) {
            // GAS has a fixed 8-decimal precision (C# NativeContract.GAS.Decimals).
            let decimals = 8u8;
            let fee = Self::parse_positive_amount(
                extra_fee,
                decimals,
                || invalid_params("Incorrect amount format."),
                || invalid_params("Incorrect amount format."),
            )?;
            let fee_amount = fee
                .value()
                .to_i64()
                .ok_or_else(|| invalid_params("Incorrect amount format."))?;
            tx.set_network_fee(tx.network_fee().saturating_add(fee_amount));
        }

        Self::sign_and_relay(server, &wallet, tx, snapshot_arc)
    }

    pub(super) fn parse_signers(
        server: &RpcServer,
        value: &Value,
    ) -> Result<Vec<Signer>, RpcException> {
        let array = value
            .as_array()
            .ok_or_else(|| invalid_params("signers must be an array"))?;
        Self::parse_signer_array(
            server,
            array,
            "signer entries must be strings",
            WitnessScope::CALLED_BY_ENTRY,
        )
    }

    pub(super) fn parse_signer_array(
        server: &RpcServer,
        array: &[Value],
        entry_error: &'static str,
        scope: WitnessScope,
    ) -> Result<Vec<Signer>, RpcException> {
        let mut signers = Vec::with_capacity(array.len());
        for entry in array {
            let addr = entry.as_str().ok_or_else(|| invalid_params(entry_error))?;
            let hash = Self::parse_script_hash(server, addr)?;
            signers.push(Signer::new(hash, scope));
        }
        Ok(signers)
    }

    fn parse_optional_signers(
        server: &RpcServer,
        params: &[Value],
        index: usize,
    ) -> Result<Option<Vec<Signer>>, RpcException> {
        params
            .get(index)
            .map(|value| Self::parse_signers(server, value))
            .transpose()
    }

    fn parse_transfer_request(
        server: &RpcServer,
        params: &[Value],
        layout: TransferParamLayout,
    ) -> Result<TransferRequest, RpcException> {
        let asset = parse_uint160(params, 0, layout.method)?;
        let from = layout
            .from_index
            .map(|index| {
                expect_string_param(params, index, layout.method)
                    .and_then(|text| Self::parse_script_hash(server, &text))
            })
            .transpose()?;
        let to = Self::parse_script_hash(
            server,
            &expect_string_param(params, layout.to_index, layout.method)?,
        )?;
        let amount = expect_string_param(params, layout.amount_index, layout.method)?;
        let signers = Self::parse_optional_signers(server, params, layout.signers_index)?;

        Ok(TransferRequest {
            asset,
            from,
            to,
            amount,
            signers,
        })
    }

    fn parse_send_many_output(
        server: &RpcServer,
        descriptor_cache: &impl Fn(&UInt160) -> neo_error::CoreResult<AssetDescriptor>,
        index: usize,
        entry: &Value,
    ) -> Result<TransferOutput, RpcException> {
        let obj = entry
            .as_object()
            .ok_or_else(|| invalid_params(format!("Invalid 'to' parameter at {index}.")))?;
        let asset_str = obj
            .get("asset")
            .and_then(|value| value.as_str())
            .ok_or_else(|| invalid_params(format!("no 'asset' parameter at 'to[{index}]'.")))?;
        let asset = UInt160::from_str(asset_str)
            .map_err(|err| invalid_params(format!("invalid asset {asset_str}: {err}")))?;
        let descriptor = descriptor_cache(&asset).map_err(|e| invalid_params(e.to_string()))?;
        let value_str = obj
            .get("value")
            .and_then(|value| value.as_str())
            .ok_or_else(|| invalid_params(format!("no 'value' parameter at 'to[{index}]'.")))?;
        let value = Self::parse_positive_amount(
            value_str,
            descriptor.decimals,
            || invalid_params(format!("Invalid 'to' parameter at {index}.")),
            || invalid_params(format!("Amount of '{asset}' can't be negative.")),
        )?;
        let address_str = obj
            .get("address")
            .and_then(|value| value.as_str())
            .ok_or_else(|| invalid_params(format!("no 'address' parameter at 'to[{index}]'.")))?;
        let to_hash = Self::parse_script_hash(server, address_str)?;
        Ok(TransferOutput {
            asset_id: asset,
            value,
            script_hash: to_hash,
            data: None,
        })
    }

    pub(super) fn parse_positive_amount(
        text: &str,
        decimals: u8,
        invalid_amount: impl FnOnce() -> RpcException,
        non_positive_amount: impl FnOnce() -> RpcException,
    ) -> Result<BigDecimal, RpcException> {
        let (ok, value) = BigDecimal::try_parse(text, decimals);
        if !ok {
            return Err(invalid_amount());
        }
        if value.sign() <= 0 {
            return Err(non_positive_amount());
        }
        Ok(value)
    }

    fn send_from_transfer_error(err: RpcException) -> RpcException {
        Self::map_insufficient_funds(err, |_| {
            RpcException::from(
                RpcError::invalid_request().with_data("Can not process this request."),
            )
        })
    }

    fn invalid_operation_transfer_error(err: RpcException) -> RpcException {
        Self::map_insufficient_funds(err, |rpc_error| {
            RpcException::new(INVALID_OPERATION_HRESULT, rpc_error.error_message())
        })
    }

    fn map_insufficient_funds(
        err: RpcException,
        map_insufficient: impl FnOnce(RpcError) -> RpcException,
    ) -> RpcException {
        let rpc_error: RpcError = err.into();
        if rpc_error.code() == RpcError::insufficient_funds_wallet().code() {
            map_insufficient(rpc_error)
        } else {
            RpcException::from(rpc_error)
        }
    }

    fn process_transfer(
        server: &RpcServer,
        asset: UInt160,
        from: Option<UInt160>,
        to: UInt160,
        amount: String,
        signers: Option<&[Signer]>,
    ) -> Result<Value, RpcException> {
        let wallet = Self::require_wallet(server)?;
        let store = server.system().store_cache();
        let snapshot = Arc::new(store.data_cache().clone());
        let reader = Nep17MetadataReaderImpl::new(
            Arc::clone(&snapshot),
            server.system().settings().as_ref().clone(),
        );
        let descriptor = AssetDescriptor::new(
            snapshot,
            &reader,
            asset,
        )
        .map_err(|err| invalid_params(err.to_string()))?;
        let value = Self::parse_positive_amount(
            &amount,
            descriptor.decimals,
            || invalid_params("Amount can't be negative."),
            || invalid_params("Amount can't be negative."),
        )?;

        let transfer = TransferOutput {
            asset_id: asset,
            value,
            script_hash: to,
            data: None,
        };

        let tx_json = Self::build_and_relay(server, &wallet, &[transfer], from, signers)?;
        Ok(tx_json)
    }

    fn build_and_relay(
        server: &RpcServer,
        wallet: &Arc<dyn CoreWallet>,
        outputs: &[TransferOutput],
        from: Option<UInt160>,
        signers: Option<&[Signer]>,
    ) -> Result<Value, RpcException> {
        let store = server.system().store_cache();
        let snapshot_arc = Arc::new(store.data_cache().clone());
        let settings = server.system().settings();
        let tx = wallet_compat::make_transfer_transaction(
            wallet.as_ref(),
            snapshot_arc.as_ref(),
            &settings,
            outputs,
            from,
            signers,
            server.settings().max_gas_invoke,
        )
        .map_err(Self::wallet_compat_failure)?;

        Self::sign_and_relay(server, wallet, tx, snapshot_arc)
    }

    fn sign_and_relay(
        server: &RpcServer,
        wallet: &Arc<dyn CoreWallet>,
        mut tx: Transaction,
        snapshot_arc: Arc<DataCache>,
    ) -> Result<Value, RpcException> {
        let mut sign_data: Option<Vec<u8>> = None;

        // Build contract parameter context and add signatures from available keys.
        let mut context = ContractParametersContext::new_with_type(
            snapshot_arc.clone(),
            tx.clone(),
            server.system().settings().network,
            Some("Neo.Network.P2P.Payloads.Transaction".to_string()),
        );
        let signer_accounts: Vec<UInt160> =
            tx.signers().iter().map(|signer| signer.account).collect();
        for signer_account in signer_accounts {
            if let Some(account) = wallet.get_account(&signer_account) {
                let mut contract_opt: Option<Contract> = account
                    .contract()
                    .cloned()
                    .map(|c| Contract::create(c.parameter_list, c.script));
                let key_opt = account.get_key();
                if contract_opt.is_none() {
                    if let Some(ref key) = key_opt {
                        let pub_point = key
                            .get_public_key_point()
                            .ok()
                            .and_then(|p| ECPoint::from_bytes(&p.to_bytes()).ok());
                        if let Some(point) = pub_point {
                            contract_opt = Some(Contract::create_signature_contract(point));
                        }
                    }
                }

                if let Some(contract) = contract_opt {
                    context.add_contract(contract.clone());
                    if let Some(key) = key_opt {
                        if account.has_key() && !account.is_locked() {
                            let signature = wallet_compat::sign_transaction_with_key(
                                &tx,
                                &key,
                                server.system().settings().network,
                            )
                            .map_err(internal_error)?;
                            // Neo N3 uses secp256r1 (NIST P-256) curve.
                            let pub_key =
                                ECPoint::new(ECCurve::Secp256r1, key.compressed_public_key())
                                    .map_err(|e| internal_error(e.to_string()))?;
                            let _ = context.add_signature(contract.clone(), pub_key, signature);
                        }
                    } else if account.has_key() && !account.is_locked() {
                        let sign_data = if let Some(data) = sign_data.as_ref() {
                            data.clone()
                        } else {
                            let data = neo_payloads::get_sign_data_vec(
                                &tx,
                                server.system().settings().network,
                            )
                            .map_err(|err| internal_error(err.to_string()))?;
                            sign_data = Some(data.clone());
                            data
                        };
                        let wallet_clone = Arc::clone(wallet);
                        let signature = Self::await_wallet_future(Box::pin(async move {
                            wallet_clone.sign(&sign_data, &signer_account).await
                        }))?;
                        if signature.len() != 64 {
                            return Err(internal_error(
                                "Invalid signature length from wallet".to_string(),
                            ));
                        }
                        let pub_key_bytes = signature_contract_pubkey(&contract.script)?;
                        let pub_key = ECPoint::new(ECCurve::Secp256r1, pub_key_bytes)
                            .map_err(|e| internal_error(e.to_string()))?;
                        let _ = context.add_signature(contract.clone(), pub_key, signature);
                    }
                }
            }
        }

        if !context.completed() {
            return Ok(context.to_json());
        }

        if let Some(witnesses) = context.witnesses() {
            tx.set_witnesses(witnesses);
        }

        // Adjust network fee if necessary (parity with C# min fee calculation).
        if tx.size() > 1024 {
            let policy = PolicyContract::new();
            let fee_per_byte = policy
                .get_fee_per_byte_snapshot(snapshot_arc.as_ref())
                .map(i64::from)
                .unwrap_or_else(|_| {
                    i64::from(neo_native_contracts::policy_contract::DEFAULT_FEE_PER_BYTE)
                });
            let cal_fee = tx.size() as i64 * fee_per_byte + 100_000;
            if tx.network_fee() < cal_fee {
                tx.set_network_fee(cal_fee);
            }
        }
        if tx.network_fee() > server.settings().max_fee {
            return Err(RpcException::from(RpcError::wallet_fee_limit()));
        }

        match rpc_relay::relay_transaction(server, tx.clone()) {
            Ok(relay_result) => {
                rpc_relay::map_relay_result(relay_result)?;
                let settings = server.system().settings();
                Ok(tx.to_json(&settings))
            }
            Err(err) => {
                // Preverify failure: surface unsigned context.
                let mut context = ContractParametersContext::new_with_type(
                    snapshot_arc,
                    tx.clone(),
                    server.system().settings().network,
                    Some("Neo.Network.P2P.Payloads.Transaction".to_string()),
                );
                for signer in tx.signers() {
                    if let Some(account) = wallet.get_account(&signer.account) {
                        if let Some(contract) = account.contract() {
                            context.add_contract(Contract::create(
                                contract.parameter_list.clone(),
                                contract.script.clone(),
                            ));
                        }
                    }
                }
                let mut json = context.to_json();
                if let Some(obj) = json.as_object_mut() {
                    obj.insert("preverifyFail".to_string(), Value::String(err.to_string()));
                }
                Ok(json)
            }
        }
    }
}
