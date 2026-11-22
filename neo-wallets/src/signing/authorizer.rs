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

mod eval;

#[cfg(test)]
mod tests;
