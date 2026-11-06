//! Port traits for Oracle service integration with the ledger and system layers.
//!
//! These traits define the contract between the Oracle service and the
//! blockchain state. Task 89 will provide concrete implementations that
//! bridge to neo-core's persistence, native contracts, and actor system.

use crate::error::OracleServiceError;
use crate::request::OracleRequest;
use async_trait::async_trait;
use neo_crypto::ECPoint;
use neo_primitives::{OracleResponseCode, UInt160, UInt256};
use neo_protocol::payloads::Transaction;

/// Opaque snapshot handle for reading blockchain state.
///
/// The concrete type is provided by the integration layer (task 89).
/// This trait allows the Oracle service to query state without depending
/// on neo-core's DataCache directly.
///
/// `Any` is a supertrait so integration-side adapters (e.g. the response
/// transaction builder, which needs the raw ledger snapshot for the
/// ApplicationEngine and native contracts) can recover their concrete
/// snapshot type via trait upcasting.
pub trait OracleSnapshot: Send + Sync + std::any::Any {
    /// Returns the current ledger height (block index + 1).
    fn ledger_height(&self) -> u32;

    /// Returns the designated oracle nodes at the given height.
    ///
    /// Mirrors `RoleManagement::get_designated_by_role_at(.., Role::Oracle, ..)`;
    /// the returned points are ordered exactly as the native contract stores
    /// them, because the multi-sig redemption script and the accumulated
    /// signature order both derive from this list.
    fn designated_oracles(&self, height: u32) -> Result<Vec<ECPoint>, OracleServiceError>;

    /// Retrieves an oracle request by ID.
    fn get_request(&self, request_id: u64) -> Result<Option<OracleRequest>, OracleServiceError>;

    /// Retrieves all pending oracle requests.
    fn get_requests(&self) -> Result<Vec<(u64, OracleRequest)>, OracleServiceError>;

    /// Retrieves all pending oracle requests matching a URL.
    fn get_requests_by_url(
        &self,
        url: &str,
    ) -> Result<Vec<(u64, OracleRequest)>, OracleServiceError>;

    /// Returns the block index where a transaction was included.
    ///
    /// Mirrors `LedgerContract::get_transaction_state`; `None` means the
    /// transaction is absent from the ledger, which the service reports as
    /// [`OracleServiceError::RequestTransactionNotFound`].
    fn transaction_block_index(&self, tx_hash: &UInt256)
    -> Result<Option<u32>, OracleServiceError>;

    /// Returns the maximum valid-until-block increment from policy.
    fn max_valid_until_block_increment(&self) -> u32;

    /// Returns the execution fee factor from policy.
    fn exec_fee_factor(&self) -> u32;

    /// Returns the fee per byte from policy.
    fn fee_per_byte(&self) -> i64;
}

/// Factory for creating oracle snapshots.
pub trait SnapshotFactory: Send + Sync {
    /// The snapshot type produced by this factory.
    type Snapshot: OracleSnapshot;

    /// Creates a new snapshot of the current blockchain state.
    fn snapshot(&self) -> Self::Snapshot;
}

/// Wallet abstraction for oracle account management.
///
/// Mirrors neo-core's `select_oracle_accounts` helper: for every designated
/// oracle point the wallet is asked for the single-signature account
/// (`Contract::create_signature_contract(point).script_hash()`), and only
/// unlocked accounts that hold a key are returned.
pub trait OracleWallet: Send + Sync {
    /// Returns the oracle accounts (script_hash, public_key) that this wallet
    /// can sign for, filtered to only those in the designated oracle set.
    fn oracle_accounts(&self, designated_oracles: &[ECPoint]) -> Vec<(UInt160, ECPoint)>;

    /// Returns true if the wallet has at least one unlocked oracle account
    /// in the designated set.
    fn has_oracle_account(&self, designated_oracles: &[ECPoint]) -> bool {
        !self.oracle_accounts(designated_oracles).is_empty()
    }
}

/// Signer abstraction for oracle response signing.
#[async_trait]
pub trait OracleSigner: Send + Sync {
    /// Signs a payload for oracle response submission.
    ///
    /// Returns the signature bytes or an error if signing fails.
    async fn sign_payload(
        &self,
        operation_id: String,
        account: UInt160,
        payload: Vec<u8>,
        protocol: &str,
        fields: std::collections::BTreeMap<String, String>,
    ) -> Result<Vec<u8>, OracleServiceError>;
}

/// Transaction builder for oracle response transactions.
pub trait ResponseTxBuilder: Send + Sync {
    /// Builds an oracle response transaction.
    ///
    /// This encapsulates the complex fee calculation, witness construction,
    /// and verification logic that depends on the ApplicationEngine and
    /// native contracts.
    ///
    /// Returns the transaction together with the *effective* response code.
    /// Mirroring neo-core's `create_response_tx`, the builder downgrades the
    /// code to [`OracleResponseCode::ResponseTooLarge`] when the filtered
    /// result exceeds `MAX_RESULT_SIZE`, or to
    /// [`OracleResponseCode::InsufficientFunds`] when the computed network fee
    /// exceeds the request's `gas_for_response` budget; in both cases the
    /// result embedded in the transaction is cleared.
    #[allow(clippy::too_many_arguments)]
    fn build_response_tx(
        &self,
        snapshot: &dyn OracleSnapshot,
        request: &OracleRequest,
        response_id: u64,
        response_code: OracleResponseCode,
        response_result: Vec<u8>,
        oracle_nodes: &[ECPoint],
        network: u32,
        use_current_height: bool,
    ) -> Result<(Transaction, OracleResponseCode), OracleServiceError>;
}

/// Transaction relay for submitting completed oracle responses.
pub trait TxRelay: Send + Sync {
    /// Attempts to relay a transaction to the network.
    ///
    /// Returns true if the transaction was accepted for relay.
    fn relay_transaction(&self, tx: Transaction) -> Result<(), OracleServiceError>;
}

/// Signature verification for oracle responses.
///
/// Mirrors neo-core's `verify_oracle_signature`, which delegates to
/// `ECPoint::verify_signature`.
pub trait SignatureVerifier: Send + Sync {
    /// Verifies an ECDSA signature against a public key and message.
    fn verify(&self, public_key: &ECPoint, message: &[u8], signature: &[u8]) -> bool;
}

/// Combined oracle infrastructure providing all integration points.
pub trait OracleInfrastructure: Send + Sync {
    /// The snapshot factory type.
    type Snapshots: SnapshotFactory;
    /// The wallet type.
    type Wallet: OracleWallet;
    /// The signer type.
    type Signer: OracleSigner;
    /// The transaction builder type.
    type TxBuilder: ResponseTxBuilder;
    /// The transaction relay type.
    type Relay: TxRelay;
    /// The signature verifier type.
    type Verifier: SignatureVerifier;

    /// Returns the snapshot factory.
    fn snapshots(&self) -> &Self::Snapshots;

    /// Returns the current wallet, if available.
    fn wallet(&self) -> Option<&Self::Wallet>;

    /// Returns the signer, if available.
    fn signer(&self) -> Option<&Self::Signer>;

    /// Returns the transaction builder.
    fn tx_builder(&self) -> &Self::TxBuilder;

    /// Returns the transaction relay.
    fn relay(&self) -> &Self::Relay;

    /// Returns the signature verifier.
    fn verifier(&self) -> &Self::Verifier;
}
