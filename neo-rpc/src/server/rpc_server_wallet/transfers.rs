use neo_execution::Nep17MetadataReaderImpl;
use neo_payloads::conflicts::Conflicts;
use neo_payloads::signer::Signer;
use neo_payloads::transaction_attribute::TransactionAttribute;
#[cfg(test)]
use neo_primitives::WitnessScope;
use neo_primitives::{BigDecimal, UInt160};
use neo_vm_rs::OpCode;
use neo_wallets::{AssetDescriptor, TransferOutput, Wallet as CoreWallet};
use num_traits::ToPrimitive;
use serde_json::Value;
use std::sync::Arc;

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::invalid_params;
use crate::server::rpc_server::RpcServer;
use crate::server::wallet_compat;

use super::RpcServerWallet;
use super::errors::{
    invalid_operation_transfer_error, send_from_transfer_error, wallet_compat_failure,
};
use super::ledger_provider::{
    NativeWalletLedgerProviderFactory, WalletLedgerProvider, WalletLedgerProviderFactory,
};
use super::request::{
    CancelTransactionRequest, SendManyOutputRequest, SendManyRequest, TransferRequest,
};
use super::signing;

impl RpcServerWallet {
    pub(super) fn send_from(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let _ = Self::require_wallet(server)?;
        let request =
            TransferRequest::parse_send_from(params, server.system().settings().address_version)?;
        Self::process_transfer(
            server,
            request.asset,
            request.from,
            request.to,
            request.amount,
            request.signers.as_deref(),
        )
        .map_err(send_from_transfer_error)
    }

    pub(super) fn send_to_address(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let _ = Self::require_wallet(server)?;
        let request = TransferRequest::parse_send_to_address(
            params,
            server.system().settings().address_version,
        )?;
        Self::process_transfer(
            server,
            request.asset,
            request.from,
            request.to,
            request.amount,
            request.signers.as_deref(),
        )
        .map_err(invalid_operation_transfer_error)
    }

    pub(super) fn send_many(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let wallet = Self::require_wallet(server)?;
        let request = SendManyRequest::parse(params, server.system().settings().address_version)?;

        let store = server.system().store_cache();
        let descriptor_snapshot = Arc::new(store.data_cache().clone());
        let reader = Nep17MetadataReaderImpl::new_with_native_contract_provider(
            Arc::clone(&descriptor_snapshot),
            server.system().settings().as_ref().clone(),
            server.system().native_contract_provider(),
        );
        let descriptor_cache = |asset: &UInt160| {
            AssetDescriptor::new(Arc::clone(&descriptor_snapshot), &reader, *asset)
                .map_err(|err| neo_error::CoreError::other(err.to_string()))
        };

        let transfers = request
            .outputs
            .iter()
            .enumerate()
            .map(|(i, entry)| Self::parse_send_many_output(server, &descriptor_cache, i, entry))
            .collect::<Result<Vec<_>, _>>()?;

        Self::build_and_relay(
            server,
            &wallet,
            &transfers,
            request.from,
            request.signers.as_deref(),
        )
        .map_err(invalid_operation_transfer_error)
    }

    pub(super) fn cancel_transaction(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let request =
            CancelTransactionRequest::parse(params, server.system().settings().address_version)?;
        let wallet = Self::require_wallet(server)?;
        let store = server.system().store_cache();
        let snapshot = store.data_cache();
        if NativeWalletLedgerProviderFactory
            .provider()
            .transaction_state_by_hash(snapshot, &request.txid)?
            .is_some()
        {
            return Err(RpcException::from(
                RpcError::already_exists()
                    .with_data("This tx is already confirmed, can't be cancelled."),
            ));
        }

        let conflict_attr = TransactionAttribute::Conflicts(Conflicts::new(request.txid));
        let script = vec![OpCode::RET.byte()];
        let snapshot_arc = Arc::new(snapshot.clone());
        let native_contract_provider = server.system().native_contract_provider();
        let settings = server.system().settings();
        let mut tx = wallet_compat::make_transaction(
            wallet.as_ref(),
            snapshot_arc.as_ref(),
            &settings,
            &script,
            Some(request.signers[0].account),
            &request.signers,
            std::slice::from_ref(&conflict_attr),
            &native_contract_provider,
            server.settings().max_gas_invoke,
        )
        .map_err(wallet_compat_failure)?;

        if let Some(conflict_tx) = server.system().mempool().get(&request.txid) {
            let bumped = tx
                .network_fee()
                .max(conflict_tx.transaction.network_fee())
                .saturating_add(1);
            tx.set_network_fee(bumped);
        } else if let Some(extra_fee) = request.extra_fee.as_deref() {
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

        signing::sign_and_relay(server, &wallet, tx, snapshot_arc)
    }

    #[cfg(test)]
    pub(super) fn parse_signers(
        server: &RpcServer,
        value: &Value,
    ) -> Result<Vec<Signer>, RpcException> {
        super::request::parse_signers(value, server.system().settings().address_version)
    }

    #[cfg(test)]
    pub(super) fn parse_signer_array(
        server: &RpcServer,
        array: &[Value],
        entry_error: &'static str,
        scope: WitnessScope,
    ) -> Result<Vec<Signer>, RpcException> {
        super::request::parse_signer_array(
            array,
            server.system().settings().address_version,
            entry_error,
            scope,
        )
    }

    fn parse_send_many_output(
        server: &RpcServer,
        descriptor_cache: &impl Fn(&UInt160) -> neo_error::CoreResult<AssetDescriptor>,
        index: usize,
        entry: &SendManyOutputRequest,
    ) -> Result<TransferOutput, RpcException> {
        let descriptor =
            descriptor_cache(&entry.asset).map_err(|e| invalid_params(e.to_string()))?;
        let value_str = entry
            .value
            .as_deref()
            .ok_or_else(|| invalid_params(format!("no 'value' parameter at 'to[{index}]'.")))?;
        let value = Self::parse_positive_amount(
            value_str,
            descriptor.decimals,
            || invalid_params(format!("Invalid 'to' parameter at {index}.")),
            || invalid_params(format!("Amount of '{}' can't be negative.", entry.asset)),
        )?;
        let address = entry
            .address
            .as_deref()
            .ok_or_else(|| invalid_params(format!("no 'address' parameter at 'to[{index}]'.")))?;
        let to = Self::parse_script_hash(server, address)?;
        Ok(TransferOutput {
            asset_id: entry.asset,
            value,
            script_hash: to,
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
        let reader = Nep17MetadataReaderImpl::new_with_native_contract_provider(
            Arc::clone(&snapshot),
            server.system().settings().as_ref().clone(),
            server.system().native_contract_provider(),
        );
        let descriptor = AssetDescriptor::new(snapshot, &reader, asset)
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
        let native_contract_provider = server.system().native_contract_provider();
        let settings = server.system().settings();
        let tx = wallet_compat::make_transfer_transaction(
            wallet.as_ref(),
            snapshot_arc.as_ref(),
            &settings,
            outputs,
            from,
            signers,
            &native_contract_provider,
            server.settings().max_gas_invoke,
        )
        .map_err(wallet_compat_failure)?;

        signing::sign_and_relay(server, wallet, tx, snapshot_arc)
    }
}
