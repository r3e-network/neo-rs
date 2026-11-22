use super::*;

use super::prepared::{
    LedgerChainQuery, PreparedTransaction, TRANSACTION_TYPE_NAME, WalletAuthorization,
    authorization_json,
};

impl RpcServerWallet {
    pub(super) fn sign_and_relay(
        server: &RpcServer,
        wallet: &Arc<dyn CoreWallet>,
        tx: Transaction,
        snapshot_arc: Arc<PersistenceDataCache>,
    ) -> Result<Value, RpcException> {
        Self::sign_and_relay_with_observers(server, wallet, tx, snapshot_arc, || {}, || {})
    }

    pub(super) fn sign_and_relay_with_observers(
        server: &RpcServer,
        wallet: &Arc<dyn CoreWallet>,
        tx: Transaction,
        snapshot_arc: Arc<PersistenceDataCache>,
        after_authorize: impl FnOnce(),
        before_relay: impl FnOnce(),
    ) -> Result<Value, RpcException> {
        let prepared = Self::prepare_transaction(server, wallet, tx, snapshot_arc.as_ref())?;
        let tx = match Self::authorize_transaction(server, wallet, prepared)? {
            WalletAuthorization::Signed(tx) => tx,
            WalletAuthorization::Pending(context) => return Ok(context),
        };
        after_authorize();
        let current_snapshot = server.system().store_cache();
        Self::validate_transaction_before_signing(server, &tx, current_snapshot.data_cache())?;
        before_relay();

        match rpc_relay::with_relay_responder(server, |sender| {
            server
                .system()
                .tx_router_actor()
                .try_enqueue_preverify_from(tx.clone(), true, Some(sender))
                .map_err(|err| internal_error(err.to_string()))
        }) {
            Ok(relay_result) => {
                rpc_relay::map_relay_result(relay_result)?;
                Ok(tx.to_json(server.system().settings()))
            }
            Err(err) => {
                // Preverify failure: surface unsigned context
                let mut authorizer = Self::authorizer_for(
                    server,
                    tx.clone(),
                    server.system().store_cache().data_cache(),
                );
                for signer in tx.signers() {
                    let Some(account) = wallet.get_account(&signer.account) else {
                        continue;
                    };
                    let Some(contract) = account.contract() else {
                        continue;
                    };
                    // The lookup above already matched a transaction signer, so
                    // this can only reject a wallet whose contract hash drifted
                    // from its account hash; the context then omits the item.
                    if let Err(error) = authorizer.register_contract(contract) {
                        tracing::debug!(
                            %error,
                            "wallet context skipped a contract that is not a signer"
                        );
                    }
                }
                let mut json = authorization_json(
                    &authorizer,
                    server.system().settings().network,
                    Some(TRANSACTION_TYPE_NAME),
                );
                // Attach reason
                if let Some(obj) = json.as_object_mut() {
                    obj.insert("preverifyFail".to_string(), Value::String(err.to_string()));
                }
                Ok(json)
            }
        }
    }

    pub(super) fn prepare_transaction(
        server: &RpcServer,
        wallet: &Arc<dyn CoreWallet>,
        mut tx: Transaction,
        snapshot: &PersistenceDataCache,
    ) -> Result<PreparedTransaction, RpcException> {
        let projected_size = Self::projected_signed_size(&tx, wallet);
        if projected_size > 1024 {
            let fee_per_byte = PolicyContract::new()
                .get_fee_per_byte_snapshot(snapshot)
                .unwrap_or(i64::from(PolicyContract::DEFAULT_FEE_PER_BYTE));
            let projected_size = i64::try_from(projected_size)
                .map_err(|_| internal_error("Transaction size overflow".to_string()))?;
            let minimum_fee = projected_size
                .checked_mul(fee_per_byte)
                .and_then(|fee| fee.checked_add(100_000))
                .ok_or_else(|| internal_error("Network fee overflow".to_string()))?;
            if tx.network_fee() < minimum_fee {
                tx.set_network_fee(minimum_fee);
            }
        }

        let authorizer = Self::authorizer_for(server, tx, snapshot);
        Self::validate_with_chain(&authorizer, snapshot)?;
        let unsigned_data = authorizer
            .tx()
            .try_get_hash_data()
            .map_err(|err| internal_error(err.to_string()))?;
        Ok(PreparedTransaction {
            authorizer,
            unsigned_data,
        })
    }

    /// Builds an authorizer with the server's policy ceilings.
    ///
    /// `MaxFee` and the policy `MaxValidUntilBlockIncrement` are resolved from
    /// the local ledger so the shared authorizer applies the same limits the
    /// node enforces.
    fn authorizer_for(
        server: &RpcServer,
        tx: Transaction,
        snapshot: &PersistenceDataCache,
    ) -> TransactionAuthorizer {
        let system = server.system();
        let settings = system.settings();
        let limits = AuthorizationLimits::new(Self::max_valid_until_increment(snapshot, settings))
            .with_max_network_fee(server.settings().max_fee);
        TransactionAuthorizer::new(tx, settings.network, limits)
    }

    /// Reads the policy valid-until window from the local ledger.
    fn max_valid_until_increment(
        snapshot: &PersistenceDataCache,
        settings: &neo_protocol::ProtocolSettings,
    ) -> u32 {
        PolicyContract::new()
            .get_max_valid_until_block_increment_snapshot(snapshot, settings)
            .unwrap_or(settings.max_valid_until_block_increment)
    }

    /// Applies the wallets authorization rules against the local ledger.
    ///
    /// Structure, fee shape and the `MaxFee` ceiling come from
    /// [`TransactionAuthorizer::validate_structure`]; the valid-until window
    /// and the sender's GAS balance are resolved through the ledger port.
    fn validate_with_chain(
        authorizer: &TransactionAuthorizer,
        snapshot: &PersistenceDataCache,
    ) -> Result<(), RpcException> {
        authorizer
            .validate_structure()
            .map_err(Self::authorization_failure)?;
        let chain = LedgerChainQuery::new(snapshot.clone());
        Self::drive(authorizer.ensure_valid_until_window(&chain))
            .map_err(Self::authorization_failure)?;
        Self::drive(authorizer.ensure_sufficient_funds(&chain))
            .map_err(Self::authorization_failure)?;
        Self::validate_wire_format(authorizer.tx())
    }

    /// Rejects a transaction whose signers or witness scopes do not survive a
    /// wire round-trip.
    fn validate_wire_format(tx: &Transaction) -> Result<(), RpcException> {
        let mut validation_tx = tx.clone();
        validation_tx.set_witnesses(vec![Witness::new(); tx.signers().len()]);
        let bytes = validation_tx
            .try_to_bytes()
            .map_err(|err| invalid_params(format!("Invalid transaction fields: {err}")))?;
        Transaction::from_bytes(&bytes)
            .map_err(|err| invalid_params(format!("Invalid signer or witness scope: {err}")))?;
        Ok(())
    }
    /// Maps an authorization failure onto the RPC error vocabulary.
    fn authorization_failure(error: TransactionAuthorizationError) -> RpcException {
        match error {
            TransactionAuthorizationError::FeeLimitExceeded { .. } => {
                RpcException::from(RpcError::wallet_fee_limit())
            }
            TransactionAuthorizationError::InsufficientFunds { .. } => {
                RpcException::from(RpcError::insufficient_funds_wallet())
            }
            TransactionAuthorizationError::ChainUnavailable(message) => internal_error(message),
            other => invalid_params(other.to_string()),
        }
    }

    fn validate_transaction_before_signing(
        server: &RpcServer,
        tx: &Transaction,
        snapshot: &PersistenceDataCache,
    ) -> Result<(), RpcException> {
        let authorizer = Self::authorizer_for(server, tx.clone(), snapshot);
        Self::validate_with_chain(&authorizer, snapshot)
    }

    fn projected_signed_size(tx: &Transaction, wallet: &Arc<dyn CoreWallet>) -> usize {
        let witnesses = tx
            .signers()
            .iter()
            .map(|signer| {
                let Some(account) = wallet.get_account(&signer.account) else {
                    return Witness::new();
                };
                let Some(contract) = account.contract().cloned() else {
                    return Witness::new();
                };
                let signature_count = ContractSignatureContext::from_contract(&contract).required();
                Witness::new_with_scripts(
                    vec![0; signature_count * SIGNATURE_INVOCATION_SCRIPT_LENGTH],
                    contract.script,
                )
            })
            .collect();
        let mut projected = tx.clone();
        projected.set_witnesses(witnesses);
        projected.size()
    }

    /// Collects signatures for every wallet-held signer of `prepared`.
    ///
    /// Signer resolution, slot placement and completion come from the shared
    /// [`TransactionAuthorizer`]; this handler only supplies keys (through the
    /// unified signer service) and renders the RPC response. Chain state was
    /// already validated during preparation, so no further snapshot is needed.
    pub(super) fn authorize_transaction(
        server: &RpcServer,
        wallet: &Arc<dyn CoreWallet>,
        prepared: PreparedTransaction,
    ) -> Result<WalletAuthorization, RpcException> {
        let network = server.system().settings().network;
        let mut prepared = prepared;
        let signer_accounts: Vec<UInt160> = prepared
            .transaction()
            .signers()
            .iter()
            .map(|signer| signer.account)
            .collect();
        for signer_account in signer_accounts {
            let Some(account) = wallet.get_account(&signer_account) else {
                continue;
            };
            let Some(contract) = account.contract().cloned() else {
                continue;
            };
            prepared
                .authorizer
                .register_contract(&contract)
                .map_err(Self::authorization_failure)?;
            if account.is_locked() || !account.has_key() {
                continue;
            }

            // The shared authorizer decides what is signed and where the
            // signature belongs; this handler only supplies the signer.
            let canonical_data = prepared
                .authorizer
                .signer_request(&signer_account)
                .map_err(Self::authorization_failure)?
                .sign_data;
            let signature = if let Some(runtime) = server.signer_runtime() {
                Self::sign_transaction_with_service(
                    server,
                    runtime,
                    prepared.transaction(),
                    signer_account,
                    canonical_data,
                )?
            } else {
                #[cfg(not(test))]
                {
                    return Err(internal_error(
                        "RPC wallet signing requires the unified signer service".to_string(),
                    ));
                }
                #[cfg(test)]
                {
                    let wallet_clone = Arc::clone(wallet);
                    Self::await_wallet_future(async move {
                        wallet_clone.sign(&canonical_data, &signer_account)
                    })?
                }
            };
            if signature.len() != 64 {
                return Err(internal_error(
                    "Invalid signature length from signer service".to_string(),
                ));
            }
            // Standard accounts sign with the key embedded in their verification
            // script, so a remote signer needs no local key material. Multi-sig
            // accounts must use their own key, which selects the witness slot.
            let public_key = if ContractHelper::is_signature_contract(&contract.script) {
                signature_contract_pubkey(&contract.script)?
            } else {
                let Some(key_pair) = account.get_key() else {
                    return Err(internal_error(
                        "Signing account holds no key for its contract".to_string(),
                    ));
                };
                key_pair.compressed_public_key()
            };
            prepared
                .authorizer
                .add_signature(
                    &signer_account,
                    AccountSignature::new(public_key, signature),
                )
                .map_err(Self::authorization_failure)?;
        }

        if !prepared.authorizer.is_complete() {
            return Ok(WalletAuthorization::Pending(authorization_json(
                &prepared.authorizer,
                network,
                Some(TRANSACTION_TYPE_NAME),
            )));
        }
        let witnesses = prepared
            .authorizer
            .witnesses()
            .map_err(Self::authorization_failure)?;
        if witnesses.is_empty() {
            return Err(internal_error(
                "Completed signature context has no witnesses".to_string(),
            ));
        }
        Ok(WalletAuthorization::Signed(prepared.complete(witnesses)?))
    }

    fn sign_transaction_with_service(
        server: &RpcServer,
        runtime: &RpcSignerRuntime,
        tx: &Transaction,
        signer_account: UInt160,
        canonical_data: Vec<u8>,
    ) -> Result<Vec<u8>, RpcException> {
        let network = server.system().settings().network;
        let tx_hash = tx.hash().to_string();
        let account = signer_account.to_string();
        let request_id = format!("rpc:{network}:{tx_hash}:{account}");
        let fee_units = tx.system_fee().saturating_add(tx.network_fee()).max(0) as u64;
        let key_ref = runtime
            .resolve_key(&account, &SignPurpose::Transaction)
            .map_err(|error| internal_error(error.to_string()))?;
        let intent = FrozenSignIntent {
            schema_version: SIGNING_SCHEMA_VERSION,
            network,
            purpose: SignPurpose::Transaction,
            key_ref,
            payload: SignPayload::Canonical {
                encoding: "neo-n3-sign-data-v1".to_string(),
                bytes: canonical_data,
            },
            protocol_context: ProtocolContext {
                schema_version: SIGNING_SCHEMA_VERSION,
                protocol: "neo-n3-transaction".to_string(),
                protocol_version: "1".to_string(),
                fields: BTreeMap::from([
                    ("transaction_hash".to_string(), tx_hash),
                    ("nonce".to_string(), tx.nonce().to_string()),
                    (
                        "valid_until_block".to_string(),
                        tx.valid_until_block().to_string(),
                    ),
                    ("signer_account".to_string(), account.clone()),
                ]),
            },
            policy_version: runtime.policy_version.clone(),
            fee_units,
            quota_units: 1,
            account: Some(account),
            targets: BTreeSet::new(),
        };
        let runtime = runtime.clone();
        Self::await_signer_future(async move { runtime.resume_or_start(&request_id, intent).await })
            .map(|response| response.signature)
    }

    fn await_signer_future(
        future: impl Future<Output = SignResult<SignResponse>> + Send + 'static,
    ) -> Result<SignResponse, RpcException> {
        let result = if let Ok(handle) = Handle::try_current() {
            match handle.runtime_flavor() {
                RuntimeFlavor::CurrentThread => std::thread::spawn(move || {
                    RuntimeBuilder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|error| {
                            neo_wallets::signing::SignError::new(
                                neo_wallets::signing::SignErrorCode::ProviderFailure,
                                error.to_string(),
                            )
                        })?
                        .block_on(future)
                })
                .join()
                .map_err(|_| internal_error("signer runtime thread panicked".to_string()))?,
                RuntimeFlavor::MultiThread => {
                    tokio::task::block_in_place(move || handle.block_on(future))
                }
                _ => tokio::task::block_in_place(move || handle.block_on(future)),
            }
        } else {
            RuntimeBuilder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| internal_error(error.to_string()))?
                .block_on(future)
        };
        result.map_err(|error| {
            internal_error(format!(
                "signer service rejected transaction authorization ({:?}): {}",
                error.code, error.message
            ))
        })
    }
}
pub(super) fn signature_contract_pubkey(script: &[u8]) -> Result<Vec<u8>, RpcException> {
    if !ContractHelper::is_signature_contract(script) {
        return Err(RpcException::from(
            RpcError::invalid_params().with_data("Unsupported contract script for signing"),
        ));
    }

    if script.len() < 35 {
        return Err(RpcException::from(
            RpcError::invalid_params().with_data("Invalid signature contract script"),
        ));
    }

    Ok(script[2..35].to_vec())
}
