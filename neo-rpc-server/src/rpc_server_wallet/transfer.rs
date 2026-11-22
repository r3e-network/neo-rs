use super::*;

use super::prepared::AssetDescriptor;

impl RpcServerWallet {
    pub(super) fn calculate_network_fee(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let raw = expect_base64_param_with_decode_message(
            params,
            0,
            "calculatenetworkfee",
            "Invalid transaction payload",
        )?;
        let transaction = Transaction::from_bytes(&raw).map_err(|err| {
            RpcException::from(
                RpcError::invalid_params().with_data(format!("Invalid transaction: {err}")),
            )
        })?;
        let system = server.system();
        let store = system.store_cache();
        let wallet = server.wallet();
        let wallet_ref = wallet.as_deref();
        let fee = WalletHelper::calculate_network_fee_with_wallet(
            &transaction,
            store.data_cache(),
            system.settings(),
            wallet_ref,
            server.settings().max_gas_invoke,
        )
        .map_err(invalid_params)?;
        Ok(json!({ "networkfee": fee.to_string() }))
    }

    pub(super) fn send_from(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let _ = Self::require_wallet(server)?;
        let asset = parse_uint160(params, 0, "sendfrom")?;
        let from_hash =
            Self::parse_script_hash(server, &expect_string_param(params, 1, "sendfrom")?)?;
        let to_hash =
            Self::parse_script_hash(server, &expect_string_param(params, 2, "sendfrom")?)?;
        let amount_text = expect_string_param(params, 3, "sendfrom")?;
        let signers = Self::parse_optional_signers(server, params, 4)?;
        Self::process_transfer(
            server,
            asset,
            Some(from_hash),
            to_hash,
            amount_text,
            signers.as_deref(),
        )
        .map_err(Self::send_from_transfer_error)
    }

    pub(super) fn send_to_address(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let _ = Self::require_wallet(server)?;
        let asset = parse_uint160(params, 0, "sendtoaddress")?;
        let to_hash =
            Self::parse_script_hash(server, &expect_string_param(params, 1, "sendtoaddress")?)?;
        let amount_text = expect_string_param(params, 2, "sendtoaddress")?;
        let signers = Self::parse_optional_signers(server, params, 3)?;
        Self::process_transfer(
            server,
            asset,
            None,
            to_hash,
            amount_text,
            signers.as_deref(),
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
        let settings = Arc::new(server.system().settings().clone());
        let descriptor_cache =
            |asset: &UInt160| AssetDescriptor::new(store.data_cache(), &settings, *asset);

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

        let mut signers = Vec::with_capacity(signers_array.len());
        for entry in signers_array {
            let address = entry
                .as_str()
                .ok_or_else(|| invalid_params("canceltransaction signers must be strings"))?;
            let hash = Self::parse_script_hash(server, address)?;
            signers.push(Signer::new(hash, WitnessScope::NONE));
        }

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
        let mut tx = Helper::make_transaction(
            wallet.as_ref(),
            snapshot_arc.as_ref(),
            &script,
            Some(signers[0].account),
            Some(&signers),
            Some(std::slice::from_ref(&conflict_attr)),
            server.system().settings(),
            None,
            server.settings().max_gas_invoke,
        )
        .map_err(Self::wallet_failure)?;

        if let Some(conflict_tx) = server.system().mempool().lock().try_get(&txid) {
            let bumped = tx
                .network_fee()
                .max(conflict_tx.network_fee())
                .saturating_add(1);
            tx.set_network_fee(bumped);
        } else if let Some(extra_fee) = params.get(2).and_then(Value::as_str) {
            let decimals = GasToken::new().decimals();
            let (ok, fee) = BigDecimal::try_parse(extra_fee, decimals);
            let fee_amount = fee
                .value()
                .to_i64()
                .ok_or_else(|| invalid_params("Incorrect amount format."))?;
            if !ok || fee.sign() <= 0 {
                return Err(invalid_params("Incorrect amount format."));
            }
            tx.set_network_fee(tx.network_fee().saturating_add(fee_amount));
        }

        Self::sign_and_relay(server, &wallet, tx, snapshot_arc)
    }

    fn parse_send_many_output(
        server: &RpcServer,
        descriptor_cache: &impl Fn(&UInt160) -> Result<AssetDescriptor, String>,
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
        let descriptor = descriptor_cache(&asset).map_err(invalid_params)?;
        let value_str = obj
            .get("value")
            .and_then(|value| value.as_str())
            .ok_or_else(|| invalid_params(format!("no 'value' parameter at 'to[{index}]'.")))?;
        let (ok, value) = BigDecimal::try_parse(value_str, descriptor.decimals);
        if !ok {
            return Err(invalid_params(format!(
                "Invalid 'to' parameter at {index}."
            )));
        }
        if value.sign() <= 0 {
            return Err(invalid_params(format!(
                "Amount of '{asset}' can't be negative."
            )));
        }
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
        let settings = Arc::new(server.system().settings().clone());
        let descriptor =
            AssetDescriptor::new(store.data_cache(), &settings, asset).map_err(invalid_params)?;
        let (ok, value) = BigDecimal::try_parse(&amount, descriptor.decimals);
        if !ok || value.sign() <= 0 {
            return Err(invalid_params("Amount can't be negative."));
        }

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
        let tx = Helper::make_transfer_transaction(
            wallet.as_ref(),
            snapshot_arc.as_ref(),
            outputs,
            from,
            signers,
            server.system().settings(),
            None,
            server.settings().max_gas_invoke,
        )
        .map_err(Self::wallet_failure)?;

        Self::sign_and_relay(server, wallet, tx, snapshot_arc)
    }
}
