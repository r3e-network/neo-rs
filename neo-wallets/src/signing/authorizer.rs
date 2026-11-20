//! Transport-independent transaction authorization.
//!
//! [`TransactionAuthorizer`] owns the parts of transaction authorization that do
//! not depend on a transport:
//!
//! * signer resolution — every transaction signer maps to exactly one wallet
//!   contract ([`Contract`]) and one [`ContractSignatureContext`];
//! * witness ordering — witnesses are emitted in transaction signer order,
//!   which is the order the ledger consumes them;
//! * completion checks — a transaction is only authorized once every signer
//!   contract holds its required signatures;
//! * fee and funding validation — structural checks, the configured network-fee
//!   ceiling, the valid-until-block window and the sender's GAS balance.
//!
//! Chain state is reached through the [`TransactionChainQuery`] port and
//! signatures are produced through the [`TransactionSigner`] port, so this
//! module depends neither on an RPC client, an RPC server, nor on the ledger.
//! `neo-rpc-client` implements the ports over its RPC connection and
//! `neo-rpc-server` implements them over its local ledger snapshot; both call
//! the same authorizer, so witness ordering and completion rules cannot drift
//! between the two wallet surfaces.
//!
//! Signature collection itself stays in [`ContractSignatureContext`], which
//! already encodes the canonical `CheckSig`/`CheckMultisig` invocation-script
//! layouts.

use crate::account::Witness;
use crate::multisig::{ContractSignatureContext, SignatureAddOutcome};
use neo_primitives::UInt160;
use neo_protocol::get_sign_data_vec;
use neo_protocol::payloads::{Transaction, TransactionAttribute};
use neo_smart_contract::contract::Contract;
use num_bigint::BigInt;
use std::fmt;

/// Length of a Neo N3 secp256r1 `r||s` signature.
pub const SIGNATURE_LENGTH: usize = 64;

/// Failures raised while authorizing a transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransactionAuthorizationError {
    /// The transaction fails a structural or fee-shape check.
    InvalidTransaction(String),
    /// A contract was registered for an account that is not a transaction signer.
    UnknownSigner(UInt160),
    /// A transaction signer has no resolved contract yet.
    UnresolvedSigner(UInt160),
    /// A signature could not be attached to its slot.
    SigningFailed(String),
    /// The signer produced a signature the contract cannot consume.
    UnknownSigningKey(UInt160),
    /// The network fee exceeds the configured ceiling.
    FeeLimitExceeded {
        /// Fee carried by the transaction.
        network_fee: i64,
        /// Ceiling configured for this authorization.
        limit: i64,
    },
    /// The sender cannot pay the system plus network fee.
    InsufficientFunds {
        /// Fee total required from the sender.
        required: BigInt,
        /// Balance available to the sender.
        available: BigInt,
    },
    /// At least one signer still holds fewer signatures than it requires.
    Incomplete {
        /// Signers that still need signatures.
        missing: Vec<UInt160>,
    },
    /// A chain query used by the authorizer failed.
    ChainUnavailable(String),
}

impl fmt::Display for TransactionAuthorizationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTransaction(message) => write!(formatter, "{message}"),
            Self::UnknownSigner(account) => {
                write!(formatter, "Account {account} is not a transaction signer")
            }
            Self::UnresolvedSigner(account) => {
                write!(formatter, "No contract resolved for signer {account}")
            }
            Self::SigningFailed(message) => write!(formatter, "{message}"),
            Self::UnknownSigningKey(account) => {
                write!(
                    formatter,
                    "Signing key is not part of the {account} contract"
                )
            }
            Self::FeeLimitExceeded { network_fee, limit } => write!(
                formatter,
                "Network fee {network_fee} exceeds the configured limit {limit}"
            ),
            Self::InsufficientFunds {
                required,
                available,
            } => write!(
                formatter,
                "Insufficient GAS: required {required}, available {available}"
            ),
            Self::Incomplete { missing } => {
                let accounts: Vec<String> = missing.iter().map(ToString::to_string).collect();
                write!(
                    formatter,
                    "Signatures are still missing for {}",
                    accounts.join(", ")
                )
            }
            Self::ChainUnavailable(message) => {
                write!(formatter, "Chain query failed: {message}")
            }
        }
    }
}

impl std::error::Error for TransactionAuthorizationError {}

/// Per-transaction ceilings applied by the authorizer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthorizationLimits {
    /// Highest network fee the caller accepts for one transaction.
    pub max_network_fee: i64,
    /// Widest accepted `valid_until_block` offset from the current height.
    pub max_valid_until_increment: u32,
}

impl AuthorizationLimits {
    /// Creates limits with no network-fee ceiling.
    ///
    /// `max_valid_until_increment` is the protocol policy window the caller
    /// resolved from chain state (or `ProtocolSettings`).
    #[must_use]
    pub const fn new(max_valid_until_increment: u32) -> Self {
        Self {
            max_network_fee: i64::MAX,
            max_valid_until_increment,
        }
    }

    /// Sets the network-fee ceiling.
    #[must_use]
    pub const fn with_max_network_fee(mut self, max_network_fee: i64) -> Self {
        self.max_network_fee = max_network_fee;
        self
    }
}

/// Signature material produced for one signer account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountSignature {
    /// Compressed public key that owns the witness slot.
    pub public_key: Vec<u8>,
    /// 64-byte secp256r1 signature over the canonical sign preimage.
    pub signature: Vec<u8>,
}

impl AccountSignature {
    /// Creates signature material for one key.
    #[must_use]
    pub fn new(public_key: impl Into<Vec<u8>>, signature: impl Into<Vec<u8>>) -> Self {
        Self {
            public_key: public_key.into(),
            signature: signature.into(),
        }
    }
}

/// A signature request for exactly one signer account.
#[derive(Debug)]
pub struct SignerRequest<'a> {
    /// Transaction being authorized (fees already finalized).
    pub transaction: &'a Transaction,
    /// Signer account that must produce a signature.
    pub account: &'a UInt160,
    /// Resolved verification contract of that account.
    pub contract: &'a Contract,
    /// Canonical N3 sign preimage for this transaction.
    pub sign_data: Vec<u8>,
    /// Network magic used for the preimage.
    pub network: u32,
}

/// Port: produces signatures for one signer account.
///
/// Implementations hold key material, an HSM session, or an authorized remote
/// signer service; the authorizer only decides which account needs which
/// signature and where it belongs.
#[async_trait::async_trait]
pub trait TransactionSigner: Send + Sync {
    /// Signs the canonical preimage for `request.account`.
    ///
    /// Returning an empty vector means the signer declined to sign (for example
    /// a watch-only or locked account) and leaves the context incomplete.
    async fn sign(
        &self,
        request: &SignerRequest<'_>,
    ) -> Result<Vec<AccountSignature>, TransactionAuthorizationError>;
}

/// Port: chain state consulted while authorizing a transaction.
///
/// `neo-rpc-client` implements it over its RPC connection and
/// `neo-rpc-server` over its local ledger snapshot, so neither side leaks its
/// transport into the authorizer.
#[async_trait::async_trait]
pub trait TransactionChainQuery: Send + Sync {
    /// Error surfaced by the underlying transport or ledger.
    type Error: fmt::Display + Send + Sync + 'static;

    /// Current persisted block height (not the next index).
    async fn block_height(&self) -> Result<u32, Self::Error>;

    /// Balance of `account` denominated in the smallest asset unit.
    async fn gas_balance(&self, account: &UInt160) -> Result<BigInt, Self::Error>;

    /// Network fee the node would charge for `transaction`.
    async fn network_fee(&self, transaction: &Transaction) -> Result<i64, Self::Error>;
}

/// One resolved signer: account, contract and its signature slots.
#[derive(Debug, Clone)]
struct SignerEntry {
    /// Signer account (verification script hash).
    account: UInt160,
    /// Resolved verification contract.
    contract: Contract,
    /// Signature slots for this contract.
    context: ContractSignatureContext,
}

/// Transport-independent transaction authorization state.
#[derive(Debug, Clone)]
pub struct TransactionAuthorizer {
    /// Transaction under authorization.
    tx: Transaction,
    /// Network magic used for the sign preimage.
    network: u32,
    /// Caller-supplied ceilings.
    limits: AuthorizationLimits,
    /// Resolved signers in registration order.
    entries: Vec<SignerEntry>,
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key_pair::KeyPair;
    use neo_primitives::WitnessScope;
    use neo_protocol::payloads::Signer;
    use neo_smart_contract::helper::Helper as ContractHelper;

    const NETWORK: u32 = 0x334F_454E;

    /// Deterministic chain stub: fixed height, balance and fee estimate.
    struct StubChain {
        height: u32,
        balance: BigInt,
        network_fee: i64,
    }

    #[async_trait::async_trait]
    impl TransactionChainQuery for StubChain {
        type Error = String;

        async fn block_height(&self) -> Result<u32, Self::Error> {
            Ok(self.height)
        }

        async fn gas_balance(&self, _account: &UInt160) -> Result<BigInt, Self::Error> {
            Ok(self.balance.clone())
        }

        async fn network_fee(&self, _transaction: &Transaction) -> Result<i64, Self::Error> {
            Ok(self.network_fee)
        }
    }

    /// Signer stub that signs with a fixed key set.
    struct StubSigner {
        keys: Vec<KeyPair>,
    }

    #[async_trait::async_trait]
    impl TransactionSigner for StubSigner {
        async fn sign(
            &self,
            request: &SignerRequest<'_>,
        ) -> Result<Vec<AccountSignature>, TransactionAuthorizationError> {
            // A real signer only signs for keys that take part in the contract.
            let context = ContractSignatureContext::from_contract(request.contract);
            let participants = context.public_keys();
            let mut signatures = Vec::new();
            for key in &self.keys {
                let public_key = key.compressed_public_key();
                if !participants.contains(&public_key.as_slice()) {
                    continue;
                }
                let signature = key
                    .sign(&request.sign_data)
                    .map_err(|err| TransactionAuthorizationError::SigningFailed(err.to_string()))?;
                signatures.push(AccountSignature::new(public_key, signature));
            }
            Ok(signatures)
        }
    }

    fn signature_contract(key: &KeyPair) -> Contract {
        Contract::create_signature_contract(key.get_public_key_point().expect("point"))
    }

    fn transaction_for(accounts: &[UInt160]) -> Transaction {
        let mut tx = Transaction::new();
        tx.set_nonce(7);
        tx.set_script(vec![0x40]); // RET
        tx.set_valid_until_block(100);
        tx.set_signers(
            accounts
                .iter()
                .map(|account| Signer::new(*account, WitnessScope::GLOBAL))
                .collect(),
        );
        tx
    }

    #[test]
    fn registration_rejects_unknown_signers_and_reports_progress() {
        let key = KeyPair::from_private_key(&[3u8; 32]).expect("key");
        let contract = signature_contract(&key);
        let mut authorizer = TransactionAuthorizer::new(
            transaction_for(&[contract.script_hash()]),
            NETWORK,
            AuthorizationLimits::new(10),
        );

        assert_eq!(
            authorizer.register_witness_only(&UInt160::zero()),
            Err(TransactionAuthorizationError::UnknownSigner(UInt160::zero()))
        );
        assert_eq!(authorizer.unresolved_signers().len(), 1);
        authorizer.register_contract(&contract).expect("signer");
        assert!(authorizer.unresolved_signers().is_empty());
        assert!(!authorizer.is_complete());
        assert_eq!(authorizer.pending_signers(), vec![contract.script_hash()]);
    }

    #[test]
    fn witness_only_stub_is_immediately_complete() {
        let key = KeyPair::from_private_key(&[4u8; 32]).expect("key");
        let signer = signature_contract(&key).script_hash();
        let stub = UInt160::zero();
        let mut authorizer = TransactionAuthorizer::new(
            transaction_for(&[signer, stub]),
            NETWORK,
            AuthorizationLimits::new(10),
        );
        authorizer.register_witness_only(&stub).expect("stub");
        assert_eq!(authorizer.unresolved_signers(), vec![signer]);
        assert!(!authorizer.is_complete());
    }

    #[tokio::test]
    async fn collect_signatures_fills_every_pending_slot_in_signer_order() {
        let keys: Vec<KeyPair> = (1u8..=3)
            .map(|seed| KeyPair::from_private_key(&[seed; 32]).expect("key"))
            .collect();
        let points: Vec<neo_crypto::ECPoint> = keys
            .iter()
            .map(|key| key.get_public_key_point().expect("point"))
            .collect();
        let multi = Contract::create_multi_sig_contract(2, &points);
        let single = signature_contract(&keys[0]);
        let mut authorizer = TransactionAuthorizer::new(
            transaction_for(&[single.script_hash(), multi.script_hash()]),
            NETWORK,
            AuthorizationLimits::new(10),
        );
        // Register the second signer first: witness order must still follow the
        // transaction signer list.
        authorizer.register_contract(&multi).expect("multisig");
        authorizer.register_contract(&single).expect("single");

        let signer = StubSigner { keys: keys.clone() };
        authorizer
            .collect_signatures(&signer)
            .await
            .expect("signatures");
        assert!(authorizer.is_complete());

        let witnesses = authorizer.witnesses().expect("witnesses");
        assert_eq!(witnesses.len(), 2);
        assert_eq!(witnesses[0].verification_script(), single.script.as_slice());
        assert_eq!(witnesses[0].invocation_script().len(), 2 + SIGNATURE_LENGTH);
        assert_eq!(witnesses[1].verification_script(), multi.script.as_slice());
        assert_eq!(
            witnesses[1].invocation_script().len(),
            2 * (2 + SIGNATURE_LENGTH)
        );
    }

    #[test]
    fn duplicate_and_unknown_signatures_fail_closed() {
        let key = KeyPair::from_private_key(&[5u8; 32]).expect("key");
        let contract = signature_contract(&key);
        let account = contract.script_hash();
        let mut authorizer = TransactionAuthorizer::new(
            transaction_for(&[account]),
            NETWORK,
            AuthorizationLimits::new(10),
        );
        authorizer.register_contract(&contract).expect("signer");
        let sign_data = authorizer
            .signer_request(&account)
            .expect("request")
            .sign_data;
        let signature = AccountSignature::new(
            key.compressed_public_key(),
            key.sign(&sign_data).expect("sign"),
        );
        authorizer
            .add_signature(&account, signature.clone())
            .expect("first signature");
        assert!(authorizer.is_complete());

        let duplicate = authorizer
            .add_signature(&account, signature)
            .expect_err("second round must not overwrite");
        assert!(duplicate.to_string().contains("AddSignature failed"));

        let stranger = KeyPair::from_private_key(&[9u8; 32]).expect("key");
        let unknown = AccountSignature::new(
            stranger.compressed_public_key(),
            stranger.sign(&sign_data).expect("sign"),
        );
        assert_eq!(
            authorizer.add_signature(&account, unknown),
            Err(TransactionAuthorizationError::UnknownSigningKey(account))
        );
    }

    #[tokio::test]
    async fn fee_estimate_and_funding_rules_use_the_chain_port() {
        let key = KeyPair::from_private_key(&[6u8; 32]).expect("key");
        let contract = signature_contract(&key);
        let mut authorizer = TransactionAuthorizer::new(
            transaction_for(&[contract.script_hash()]),
            NETWORK,
            AuthorizationLimits::new(10).with_max_network_fee(1_000),
        );
        authorizer.register_contract(&contract).expect("signer");

        let chain = StubChain {
            height: 90,
            balance: BigInt::from(50),
            network_fee: 100,
        };
        assert_eq!(authorizer.estimate_network_fee(&chain).await, Ok(100));
        assert!(authorizer.tx().witnesses().is_empty());
        authorizer.validate_structure().expect("structure");
        authorizer
            .ensure_valid_until_window(&chain)
            .await
            .expect("window");

        let error = authorizer
            .ensure_sufficient_funds(&chain)
            .await
            .expect_err("balance below fee");
        assert!(matches!(
            error,
            TransactionAuthorizationError::InsufficientFunds { .. }
        ));

        authorizer.set_network_fee(1_001);
        assert!(matches!(
            authorizer.validate_structure(),
            Err(TransactionAuthorizationError::FeeLimitExceeded { .. })
        ));
    }

    #[test]
    fn valid_until_window_is_enforced() {
        let key = KeyPair::from_private_key(&[7u8; 32]).expect("key");
        let contract = signature_contract(&key);
        let authorizer = TransactionAuthorizer::new(
            transaction_for(&[contract.script_hash()]),
            NETWORK,
            AuthorizationLimits::new(5),
        );
        authorizer.validate_valid_until(95).expect("inside window");
        assert!(authorizer.validate_valid_until(100).is_err());
        assert!(authorizer.validate_valid_until(89).is_err());
    }

    #[test]
    fn single_signature_public_key_comes_from_the_script() {
        let key = KeyPair::from_private_key(&[8u8; 32]).expect("key");
        let contract = signature_contract(&key);
        let account = contract.script_hash();
        let mut authorizer = TransactionAuthorizer::new(
            transaction_for(&[account]),
            NETWORK,
            AuthorizationLimits::new(10),
        );
        authorizer.register_contract(&contract).expect("signer");
        let expected = ContractHelper::is_signature_contract(&contract.script)
            .then(|| contract.script[2..35].to_vec());
        assert_eq!(authorizer.single_signature_public_key(&account), expected);
        assert_eq!(
            authorizer.single_signature_public_key(&account),
            Some(key.compressed_public_key())
        );
    }
}
