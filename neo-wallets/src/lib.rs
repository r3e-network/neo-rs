//! Wallet primitives and account management.
//!
//! This crate hosts the wallet layer extracted from `neo-core`:
//!
//! * BIP-32 key paths ([`bip32`]) and BIP-39 mnemonic helpers ([`bip39`]).
//! * BIP-32 extended-key derivation ([`bip32::extended_key`]).
//! * NEP-2 encrypted-key encrypt/decrypt ([`nep2`]).
//! * Key pair management ([`key_pair`]) and address/script helpers ([`helper`]).
//! * Wallet accounts ([`account`]), the base wallet abstraction ([`wallet`]) and
//!   the concrete NEP-6 file-backed wallet ([`nep6`]).
//! * Transfer outputs ([`transfer_output`]) and the active-wallet provider
//!   ([`wallet_provider`]).
//! * Unified, policy-bound signing service contracts ([`signing`]).
//!
//! # Features
//!
//! - `remote` (default): the outbound mTLS remote-signer transport in
//!   `signing::remote_client` (`MtlsRemoteSignerTransport`,
//!   `RemoteSignerProvider`) and its `reqwest`/TLS dependency. The
//!   remote-signing wire vocabulary in [`signing`] stays available without it.
//!
//! Disabling default features (`--no-default-features`) yields the wallet core
//! only — key management, accounts, NEP-2/NEP-6, and signing-service policy,
//! replay and provider contracts — with no TLS stack linked.

pub mod bip32;
pub mod bip39;

pub mod account;
pub mod helper;
pub mod key_pair;
pub mod multisig;
pub mod nep2;
pub mod nep6;
pub mod signing;
pub mod transfer_output;
pub mod wallet;
pub mod wallet_provider;

// BIP-39 mnemonic helpers re-exported at the crate root (used by integration tests).
pub use bip39::{get_mnemonic_code, get_mnemonic_code_with_language, mnemonic_to_entropy};

// Core wallet types re-exported at the crate root for ergonomic access.
pub use account::{Contract, StandardWalletAccount, UnlockState, WalletAccount, Witness};
pub use bip32::{ExtendedKey, KeyPath};
pub use helper::{AccountScriptResolver, Base58CheckDecode, Helper};
pub use key_pair::KeyPair;
pub use multisig::{ContractSignatureContext, MultiSigWitnessBuilder, SignatureAddOutcome};
pub use nep2::ScryptParameters;
pub use nep6::{Nep6Account, Nep6Wallet, Nep6WalletFactory};
pub use transfer_output::TransferOutput;
pub use wallet::{Version, Wallet, WalletError, WalletFactory, WalletManager, WalletResult};
pub use wallet_provider::WalletProvider;

// Signing service types re-exported at the crate root.
pub use signing::{
    ALGORITHM_ECDSA_P256_SHA256, AccountSignature, AuthenticatedContext, AuthorizationLimits,
    AuthorizedSignRequest, BeginSigningOutcome, BeginSigningRequest, DefaultSignerService,
    DigestAlgorithm, DurableSigningStore, FrozenSignIntent, IdempotencyInfo, KeyInfo, KeyRef,
    PersistedSigningOperation, PolicyDecision, PolicyLimits, PolicySelector, ProtocolContext,
    ProviderCapabilities, ProviderOperationStatus, ProviderOutcomeProvenance, ProviderSignature,
    QuotaReservation, SIGNING_SCHEMA_VERSION, SecurityLevel, SignError, SignErrorCode, SignPayload,
    SignPurpose, SignRequest, SignResponse, SignResult, SignerProvider, SignerProviderRegistry,
    SignerRequest, SignerService, SigningAuditEvent, SigningDeadlineRegistry, SigningKeyBinding,
    SigningKeyResolver, SigningPolicy, SigningRequestState, SigningRequestStatus, SigningStore,
    TransactionAuthorizationError, TransactionAuthorizer, TransactionChainQuery, TransactionSigner,
    TrustedSignerClient,
};
