use super::*;
impl TransactionAuthorizer {
    /// Creates an authorizer for `tx`.
    #[must_use]
    pub fn new(tx: Transaction, network: u32, limits: AuthorizationLimits) -> Self {
        Self {
            tx,
            network,
            limits,
            entries: Vec::new(),
        }
    }

    /// Returns the managed transaction.
    #[must_use]
    pub const fn tx(&self) -> &Transaction {
        &self.tx
    }

    /// Returns the applied ceilings.
    #[must_use]
    pub const fn limits(&self) -> AuthorizationLimits {
        self.limits
    }

    /// Replaces the ceilings.
    pub const fn set_limits(&mut self, limits: AuthorizationLimits) {
        self.limits = limits;
    }

    /// Sets the network fee carried by the managed transaction.
    pub fn set_network_fee(&mut self, network_fee: i64) {
        self.tx.set_network_fee(network_fee);
    }

    /// Sets the system fee carried by the managed transaction.
    pub fn set_system_fee(&mut self, system_fee: i64) {
        self.tx.set_system_fee(system_fee);
    }

    /// Replaces the attributes carried by the managed transaction.
    pub fn set_attributes(&mut self, attributes: Vec<TransactionAttribute>) {
        self.tx.set_attributes(attributes);
    }

    /// Registers the contract of a transaction signer.
    ///
    /// The requirement for an unrecognised script defaults to one signature so
    /// an account the wallet cannot interpret is never treated as satisfied.
    ///
    /// # Errors
    ///
    /// Returns [`TransactionAuthorizationError::UnknownSigner`] when the
    /// contract's script hash is not one of the transaction signers.
    pub fn register_contract(
        &mut self,
        contract: &Contract,
    ) -> Result<(), TransactionAuthorizationError> {
        let account = contract.script_hash();
        // A contract with no verification script cannot consume signatures:
        // its witness arrives out of band, so the slot is complete as soon as
        // it is registered.
        if contract.script.is_empty() {
            return self.register_witness_only(&account);
        }
        self.ensure_signer(account)?;
        if self.entry(&account).is_some() {
            return Ok(());
        }
        let parameter_count = contract.parameter_list.len().max(1);
        let context = ContractSignatureContext::new(&contract.script, parameter_count);
        self.entries.push(SignerEntry {
            account,
            contract: contract.clone(),
            context,
        });
        Ok(())
    }

    /// Registers a signer whose witness is supplied elsewhere.
    ///
    /// Used for hash-only stubs (a deployed contract with no local script) and
    /// for contract accounts whose invocation script arrives out of band: the
    /// slot is complete immediately and emits an empty invocation script with
    /// the registered (possibly empty) verification script.
    ///
    /// # Errors
    ///
    /// Returns [`TransactionAuthorizationError::UnknownSigner`] when `account`
    /// is not one of the transaction signers.
    pub fn register_witness_only(
        &mut self,
        account: &UInt160,
    ) -> Result<(), TransactionAuthorizationError> {
        self.ensure_signer(*account)?;
        if self.entry(account).is_some() {
            return Ok(());
        }
        self.entries.push(SignerEntry {
            account: *account,
            contract: Contract::create_with_hash(*account, Vec::new()),
            context: ContractSignatureContext::new(&[], 0),
        });
        Ok(())
    }

    /// Returns the registered signers with their signature contexts.
    pub fn entries(&self) -> impl Iterator<Item = (&UInt160, &ContractSignatureContext)> {
        self.entries
            .iter()
            .map(|entry| (&entry.account, &entry.context))
    }

    /// Returns the signature context of one signer, if resolved.
    #[must_use]
    pub fn entry(&self, account: &UInt160) -> Option<&ContractSignatureContext> {
        self.entries
            .iter()
            .find(|entry| entry.account == *account)
            .map(|entry| &entry.context)
    }

    /// Returns the resolved contract of one signer, if any.
    #[must_use]
    pub fn contract(&self, account: &UInt160) -> Option<&Contract> {
        self.entries
            .iter()
            .find(|entry| entry.account == *account)
            .map(|entry| &entry.contract)
    }

    /// Returns the transaction signers that still have no contract.
    #[must_use]
    pub fn unresolved_signers(&self) -> Vec<UInt160> {
        self.tx
            .signer_accounts()
            .into_iter()
            .filter(|account| self.entry(account).is_none())
            .collect()
    }

    /// Returns the transaction signers that still need signatures.
    #[must_use]
    pub fn pending_signers(&self) -> Vec<UInt160> {
        self.tx
            .signer_accounts()
            .into_iter()
            .filter(|account| match self.entry(account) {
                Some(context) => !context.is_complete(),
                None => true,
            })
            .collect()
    }

    /// Builds the signature request of one signer.
    ///
    /// The request is built even for a signer that already holds its
    /// signatures: re-signing an occupied slot fails closed in
    /// [`Self::add_signature`].
    ///
    /// # Errors
    ///
    /// Returns [`TransactionAuthorizationError::UnresolvedSigner`] when the
    /// account has no registered contract, or
    /// [`TransactionAuthorizationError::InvalidTransaction`] when the sign
    /// preimage cannot be derived.
    pub fn signer_request(
        &self,
        account: &UInt160,
    ) -> Result<SignerRequest<'_>, TransactionAuthorizationError> {
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.account == *account)
            .ok_or(TransactionAuthorizationError::UnresolvedSigner(*account))?;
        let sign_data = get_sign_data_vec(&self.tx, self.network)
            .map_err(|err| TransactionAuthorizationError::InvalidTransaction(err.to_string()))?;
        Ok(SignerRequest {
            transaction: &self.tx,
            account: &entry.account,
            contract: &entry.contract,
            sign_data,
            network: self.network,
        })
    }

    /// Attaches one signature to its signer slot.
    ///
    /// # Errors
    ///
    /// Returns [`TransactionAuthorizationError::UnresolvedSigner`] when the
    /// account has no registered contract,
    /// [`TransactionAuthorizationError::SigningFailed`] for malformed or
    /// duplicate signatures, and [`TransactionAuthorizationError::UnknownSigningKey`]
    /// when the key does not take part in the contract.
    pub fn add_signature(
        &mut self,
        account: &UInt160,
        signature: AccountSignature,
    ) -> Result<SignatureAddOutcome, TransactionAuthorizationError> {
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.account == *account)
            .ok_or(TransactionAuthorizationError::UnresolvedSigner(*account))?;
        let outcome = entry
            .context
            .add_signature(&signature.public_key, signature.signature)
            .map_err(|err| TransactionAuthorizationError::SigningFailed(err.to_string()))?;
        match outcome {
            SignatureAddOutcome::Added => Ok(outcome),
            SignatureAddOutcome::Duplicate => Err(TransactionAuthorizationError::SigningFailed(
                format!("AddSignature failed: duplicate signature for {account}"),
            )),
            SignatureAddOutcome::UnknownKey => {
                Err(TransactionAuthorizationError::UnknownSigningKey(*account))
            }
        }
    }

    /// Attaches every signature produced for one signer account.
    ///
    /// An empty list leaves the context unchanged.
    ///
    /// # Errors
    ///
    /// Propagates [`Self::add_signature`] failures.
    pub fn add_account_signatures(
        &mut self,
        account: &UInt160,
        signatures: Vec<AccountSignature>,
    ) -> Result<(), TransactionAuthorizationError> {
        for signature in signatures {
            self.add_signature(account, signature)?;
        }
        Ok(())
    }

    /// Returns `true` once every signer holds its required signatures.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        self.tx.signer_accounts().iter().all(|account| {
            self.entry(account)
                .is_some_and(ContractSignatureContext::is_complete)
        })
    }

    /// Returns verification-script-only witnesses for fee estimation.
    ///
    /// The node prices verification from the verification script; the empty
    /// invocation script keeps the estimate independent of the signatures.
    #[must_use]
    pub fn placeholder_witnesses(&self) -> Vec<Witness> {
        self.tx
            .signer_accounts()
            .into_iter()
            .map(|account| match self.contract(&account) {
                Some(contract) => Witness::new_with_scripts(Vec::new(), contract.script.clone()),
                None => Witness::new(),
            })
            .collect()
    }

    /// Installs placeholder witnesses on the managed transaction.
    pub fn install_placeholder_witnesses(&mut self) {
        let witnesses = self.placeholder_witnesses();
        self.tx.set_witnesses(witnesses);
    }

    /// Removes witnesses from the managed transaction.
    pub fn clear_witnesses(&mut self) {
        self.tx.set_witnesses(Vec::new());
    }

    /// Installs witnesses supplied out of band on the managed transaction.
    pub fn install_witnesses(&mut self, witnesses: Vec<Witness>) {
        self.tx.set_witnesses(witnesses);
    }

    /// Collects the final witnesses in transaction signer order.
    ///
    /// # Errors
    ///
    /// Returns [`TransactionAuthorizationError::Incomplete`] while any signer
    /// still needs signatures, and
    /// [`TransactionAuthorizationError::SigningFailed`] when a complete slot
    /// cannot be encoded.
    pub fn witnesses(&self) -> Result<Vec<Witness>, TransactionAuthorizationError> {
        let mut witnesses = Vec::with_capacity(self.entries.len());
        for account in self.tx.signer_accounts() {
            let Some(context) = self.entry(&account) else {
                return Err(TransactionAuthorizationError::UnresolvedSigner(account));
            };
            if !context.is_complete() {
                return Err(TransactionAuthorizationError::Incomplete {
                    missing: self.pending_signers(),
                });
            }
            witnesses.push(
                context
                    .build_witness()
                    .map_err(|err| TransactionAuthorizationError::SigningFailed(err.to_string()))?,
            );
        }
        if witnesses.is_empty() {
            return Err(TransactionAuthorizationError::InvalidTransaction(
                "No witnesses available; context incomplete".to_string(),
            ));
        }
        Ok(witnesses)
    }

    /// Consumes the authorizer, returning the fully signed transaction.
    ///
    /// # Errors
    ///
    /// Returns [`TransactionAuthorizationError::Incomplete`] while any signer
    /// still needs signatures.
    pub fn into_transaction(mut self) -> Result<Transaction, TransactionAuthorizationError> {
        if !self.is_complete() {
            return Err(TransactionAuthorizationError::Incomplete {
                missing: self.pending_signers(),
            });
        }
        let witnesses = self.witnesses()?;
        self.tx.set_witnesses(witnesses);
        Ok(self.tx)
    }

    /// Validates structure, fee shape and the configured network-fee ceiling.
    ///
    /// # Errors
    ///
    /// Returns [`TransactionAuthorizationError::InvalidTransaction`] for a
    /// malformed transaction and
    /// [`TransactionAuthorizationError::FeeLimitExceeded`] above the ceiling.
    pub fn validate_structure(&self) -> Result<(), TransactionAuthorizationError> {
        self.validate_fees()?;
        if self.tx.script().is_empty() {
            return Err(TransactionAuthorizationError::InvalidTransaction(
                "Transaction script cannot be empty".to_string(),
            ));
        }
        neo_vm::validate_strict_script(self.tx.script()).map_err(|err| {
            TransactionAuthorizationError::InvalidTransaction(format!(
                "Invalid transaction script: {err}"
            ))
        })?;
        Ok(())
    }

    /// Validates the fee shape, the sender's presence and the fee ceiling.
    ///
    /// Version and signer checks belong here because they decide who pays and
    /// how much. Script validity is node policy and is only checked by
    /// [`Self::validate_structure`], so a transport that trusts its caller's
    /// script can still share the fee rules.
    ///
    /// # Errors
    ///
    /// Returns [`TransactionAuthorizationError::InvalidTransaction`] for an
    /// unusable transaction and
    /// [`TransactionAuthorizationError::FeeLimitExceeded`] above the ceiling.
    pub fn validate_fees(&self) -> Result<(), TransactionAuthorizationError> {
        if self.tx.version() != 0 {
            return Err(TransactionAuthorizationError::InvalidTransaction(
                "Invalid transaction version".to_string(),
            ));
        }
        if self.tx.signers().is_empty() {
            return Err(TransactionAuthorizationError::InvalidTransaction(
                "Transaction must have at least one signer".to_string(),
            ));
        }
        if self.tx.system_fee() < 0 {
            return Err(TransactionAuthorizationError::InvalidTransaction(
                "Invalid system fee".to_string(),
            ));
        }
        if self.tx.network_fee() < 0 {
            return Err(TransactionAuthorizationError::InvalidTransaction(
                "Invalid network fee".to_string(),
            ));
        }
        self.tx
            .system_fee()
            .checked_add(self.tx.network_fee())
            .ok_or_else(|| {
                TransactionAuthorizationError::InvalidTransaction(
                    "Combined transaction fee overflow".to_string(),
                )
            })?;
        if self.tx.network_fee() > self.limits.max_network_fee {
            return Err(TransactionAuthorizationError::FeeLimitExceeded {
                network_fee: self.tx.network_fee(),
                limit: self.limits.max_network_fee,
            });
        }
        Ok(())
    }

    /// Validates `valid_until_block` against a known chain height.
    ///
    /// # Errors
    ///
    /// Returns [`TransactionAuthorizationError::InvalidTransaction`] when the
    /// validity window is closed or wider than the configured increment.
    pub fn validate_valid_until(&self, height: u32) -> Result<(), TransactionAuthorizationError> {
        let limit = height.saturating_add(self.limits.max_valid_until_increment);
        if self.tx.valid_until_block() <= height || self.tx.valid_until_block() > limit {
            return Err(TransactionAuthorizationError::InvalidTransaction(
                "Invalid valid-until-block".to_string(),
            ));
        }
        Ok(())
    }

    /// Validates the sender balance against the transaction fees.
    ///
    /// # Errors
    ///
    /// Returns [`TransactionAuthorizationError::InsufficientFunds`] when
    /// `available` cannot cover the system plus network fee.
    pub fn validate_funds(&self, available: &BigInt) -> Result<(), TransactionAuthorizationError> {
        let required = self.required_fee_total()?;
        if available < &required {
            return Err(TransactionAuthorizationError::InsufficientFunds {
                required,
                available: available.clone(),
            });
        }
        Ok(())
    }

    /// Returns the combined system and network fee owed by the sender.
    ///
    /// # Errors
    ///
    /// Returns [`TransactionAuthorizationError::InvalidTransaction`] when the
    /// fee pair overflows.
    pub fn required_fee_total(&self) -> Result<BigInt, TransactionAuthorizationError> {
        let total = self
            .tx
            .system_fee()
            .checked_add(self.tx.network_fee())
            .ok_or_else(|| {
                TransactionAuthorizationError::InvalidTransaction(
                    "Combined transaction fee overflow".to_string(),
                )
            })?;
        Ok(BigInt::from(total))
    }

    /// Validates `valid_until_block` through the chain port.
    ///
    /// # Errors
    ///
    /// Propagates port failures as
    /// [`TransactionAuthorizationError::ChainUnavailable`] and the window rules
    /// of [`Self::validate_valid_until`].
    pub async fn ensure_valid_until_window<Q>(
        &self,
        chain: &Q,
    ) -> Result<(), TransactionAuthorizationError>
    where
        Q: TransactionChainQuery,
    {
        let height = chain
            .block_height()
            .await
            .map_err(|err| TransactionAuthorizationError::ChainUnavailable(err.to_string()))?;
        self.validate_valid_until(height)
    }

    /// Validates the sender balance through the chain port.
    ///
    /// # Errors
    ///
    /// Propagates port failures as
    /// [`TransactionAuthorizationError::ChainUnavailable`] and the balance rule
    /// of [`Self::validate_funds`].
    pub async fn ensure_sufficient_funds<Q>(
        &self,
        chain: &Q,
    ) -> Result<(), TransactionAuthorizationError>
    where
        Q: TransactionChainQuery,
    {
        let sender = self.tx.sender().ok_or_else(|| {
            TransactionAuthorizationError::InvalidTransaction(
                "Transaction must have a sender".to_string(),
            )
        })?;
        let available = chain
            .gas_balance(&sender)
            .await
            .map_err(|err| TransactionAuthorizationError::ChainUnavailable(err.to_string()))?;
        self.validate_funds(&available)
    }

    /// Asks the chain port for the network fee and applies it.
    ///
    /// Placeholder witnesses are installed for the estimate and removed
    /// afterwards so the unsigned hash matches the finalized fee fields.
    ///
    /// # Errors
    ///
    /// Propagates port failures as
    /// [`TransactionAuthorizationError::ChainUnavailable`] and rejects negative
    /// estimates.
    pub async fn estimate_network_fee<Q>(
        &mut self,
        chain: &Q,
    ) -> Result<i64, TransactionAuthorizationError>
    where
        Q: TransactionChainQuery,
    {
        self.install_placeholder_witnesses();
        let fee = chain
            .network_fee(&self.tx)
            .await
            .map_err(|err| TransactionAuthorizationError::ChainUnavailable(err.to_string()));
        self.clear_witnesses();
        let fee = fee?;
        if fee < 0 {
            return Err(TransactionAuthorizationError::InvalidTransaction(
                "Invalid network fee".to_string(),
            ));
        }
        self.tx.set_network_fee(fee);
        Ok(fee)
    }

    /// Collects one round of signatures from every registered signer.
    ///
    /// Signers that already hold signatures are asked again, so duplicate key
    /// material fails closed in [`Self::add_signature`]; a signer that declines
    /// to sign (locked or watch-only) leaves its slot untouched.
    ///
    /// # Errors
    ///
    /// Propagates port and slot failures.
    pub async fn collect_signatures<S>(
        &mut self,
        signer: &S,
    ) -> Result<(), TransactionAuthorizationError>
    where
        S: TransactionSigner,
    {
        let accounts: Vec<UInt160> = self.entries.iter().map(|entry| entry.account).collect();
        for account in accounts {
            let request = self.signer_request(&account)?;
            let signatures = signer.sign(&request).await?;
            drop(request);
            self.add_account_signatures(&account, signatures)?;
        }
        Ok(())
    }

    /// Returns the verification-script public key of a single-signature
    /// contract.
    #[must_use]
    pub fn single_signature_public_key(&self, account: &UInt160) -> Option<Vec<u8>> {
        self.entry(account)
            .and_then(ContractSignatureContext::signature_public_key)
            .map(<[u8]>::to_vec)
    }

    /// Installs the complete witnesses and hands back the signed transaction.
    ///
    /// # Errors
    ///
    /// Same failures as [`Self::witnesses`].
    pub fn finalize(&mut self) -> Result<Transaction, TransactionAuthorizationError> {
        let witnesses = self.witnesses()?;
        self.install_witnesses(witnesses);
        Ok(self.tx.clone())
    }

    /// Ensures `account` is one of the transaction signers.
    fn ensure_signer(&self, account: UInt160) -> Result<(), TransactionAuthorizationError> {
        if self.tx.signer_accounts().contains(&account) {
            Ok(())
        } else {
            Err(TransactionAuthorizationError::UnknownSigner(account))
        }
    }
}
