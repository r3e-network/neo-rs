//! Small helpers shared by the Oracle service submodules.
//!
//! Mirrors the reference implementation's `service/utils.rs`. The ledger- and
//! wallet-facing helpers of that file (`ledger_height`, `select_oracle_accounts`,
//! `wallet_has_oracle_account`) are expressed by the snapshot and wallet ports
//! instead, and `filter_json` already lives in [`crate::filter`], so only the
//! signature check remains here.

use super::{
    OracleService, OracleSigner, OracleSnapshot, OracleWallet, ResponseTxBuilder,
    SignatureVerifier, SnapshotFactory, TxRelay,
};
use neo_crypto::ECPoint;

impl<S, W, Sig, TB, R, V> OracleService<S, W, Sig, TB, R, V>
where
    S: SnapshotFactory + Send + Sync + 'static,
    S::Snapshot: OracleSnapshot,
    W: OracleWallet + Send + Sync + 'static,
    Sig: OracleSigner + Send + Sync + 'static,
    TB: ResponseTxBuilder + Send + Sync + 'static,
    R: TxRelay + Send + Sync + 'static,
    V: SignatureVerifier + Send + Sync + 'static,
{
    /// Verifies an oracle signature through the injected verifier port.
    ///
    /// Rejects anything that is not a 64-byte raw `r || s` ECDSA signature
    /// before delegating, exactly as the reference `verify_oracle_signature`
    /// does.
    pub fn verify_oracle_signature(&self, pubkey: &ECPoint, data: &[u8], signature: &[u8]) -> bool {
        if signature.len() != 64 {
            return false;
        }
        self.verifier.verify(pubkey, data, signature)
    }
}
