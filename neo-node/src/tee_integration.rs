//! TEE privacy-submission glue (assembly layer).
//!
//! Phase 3 S6c: the TEE runtime, privacy-encryption resolution and wallet
//! adapter sank into `neo-tee` (behind its `integration` feature). Only the
//! RPC-facing `PrivacySubmitter` adapter stays here so `neo-tee` remains
//! rpc-free. The `pub use` re-exports freeze every historical
//! `crate::tee_integration::*` path used by startup/consensus.

use neo_tee::attestation::AttestationService;
use neo_tee::mempool::{ContentCipher, KeyEncapsulation, MempoolCiphertext};
use std::sync::Arc;

pub use neo_tee::runtime::{
    PrivacyEncryption, TeeRuntime, active_runtime, clear_active_runtime, register_active_runtime,
};

/// Host-side decrypting submitter backing the opt-in
/// `sendrawtransactionencrypted` RPC endpoint when privacy mode is enabled.
///
/// Holds the configured client-facing intake KEM plus the enclave attestation
/// service so submissions are answered with an attestation report containing
/// the enclave measurements.
pub struct PrivacySubmitter {
    kem: Arc<KeyEncapsulation>,
    attestation: AttestationService,
}

/// Builds the host-side submitter for the encrypted RPC submission endpoint.
/// Returns `None` when privacy mode is off (no intake KEM is installed).
pub fn privacy_submitter(runtime: &TeeRuntime) -> Option<Arc<PrivacySubmitter>> {
    let kem = runtime.intake_kem().cloned()?;
    let attestation = AttestationService::new(runtime.enclave.clone()).ok()?;
    Some(Arc::new(PrivacySubmitter { kem, attestation }))
}

impl neo_rpc::server::EncryptedTransactionSubmitter for PrivacySubmitter {
    fn open_encrypted(
        &self,
        tx_hash: &neo_primitives::UInt256,
        ciphertext: &neo_rpc::server::MempoolCiphertext,
    ) -> Result<Arc<neo_protocol::payloads::transaction::Transaction>, String> {
        // The RPC layer hands over the raw wire bytes (base64-decoded
        // envelope). Decode them into the TEE ciphertext envelope before
        // decryption; malformed envelopes fail closed.
        let envelope =
            MempoolCiphertext::decode(ciphertext.as_bytes()).map_err(|err| err.to_string())?;
        let plaintext = ContentCipher::open(&envelope, &tx_hash.as_bytes(), &self.kem)
            .map_err(|err| err.to_string())?;
        let transaction = neo_protocol::payloads::transaction::Transaction::from_bytes(&plaintext)
            .map_err(|err| err.to_string())?;
        if transaction.hash() != *tx_hash {
            return Err("decrypted transaction hash does not match the claimed hash".to_string());
        }
        Ok(Arc::new(transaction))
    }

    fn attestation_report(&self) -> Result<Vec<u8>, String> {
        let report = self
            .attestation
            .generate_report(b"neo-node-privacy-submission")
            .map_err(|err| err.to_string())?;
        Ok(report.to_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    use neo_primitives::UInt256;
    use neo_protocol::ProtocolSettings;
    use neo_protocol::payloads::transaction::Transaction;
    use neo_rpc::server::EncryptedTransactionSubmitter;
    use neo_tee::attestation::AttestationReport;
    use neo_wallets::KeyPair;

    fn build_privacy_test_transaction(keypair: &KeyPair) -> Transaction {
        use neo_primitives::WitnessScope;
        use neo_protocol::get_sign_data_vec;
        use neo_protocol::payloads::Signer;
        use neo_protocol::witness::Witness;
        use neo_vm::OpCode;

        let settings = ProtocolSettings::default();
        let mut tx = Transaction::new();
        tx.set_nonce(1);
        tx.set_network_fee(1_0000_0000);
        tx.set_system_fee(0);
        tx.set_valid_until_block(1);
        tx.set_script(vec![OpCode::PUSH1.byte()]);
        tx.set_signers(vec![Signer::new(
            keypair.get_script_hash(),
            WitnessScope::GLOBAL,
        )]);

        let sign_data = get_sign_data_vec(&tx, settings.network).expect("sign data");
        let signature = keypair.sign(&sign_data).expect("sign");
        let mut invocation = Vec::with_capacity(signature.len() + 2);
        invocation.push(OpCode::PUSHDATA1.byte());
        invocation.push(signature.len() as u8);
        invocation.extend_from_slice(&signature);
        tx.set_witnesses(vec![Witness::new_with_scripts(
            invocation,
            keypair.get_verification_script(),
        )]);
        tx
    }

    #[cfg(not(feature = "tee-sgx"))]
    #[test]
    fn privacy_disabled_by_default_keeps_plaintext_runtime() {
        let temp = tempdir().unwrap();
        let runtime = TeeRuntime::new(temp.path().to_path_buf(), "batched", 1000)
            .expect("TEE runtime (privacy off)");

        // No encryption engine is installed: no privacy flag, no intake KEM,
        // no encrypted-submission submitter.
        assert!(!runtime.privacy_enabled());
        assert!(runtime.intake_kem().is_none());
        assert!(privacy_submitter(&runtime).is_none());

        // Pre-privacy path: a mirrored transaction is immediately ordered
        // without any resolve step.
        let tx_hash = [4u8; 32];
        runtime
            .mempool
            .add_transaction(tx_hash, b"plain".to_vec(), 1_000, 0, [7u8; 20])
            .unwrap();
        assert_eq!(runtime.mempool.get_ordered_hashes(1), vec![tx_hash]);
    }

    #[cfg(not(feature = "tee-sgx"))]
    #[test]
    fn privacy_mode_committee_intake_kem_uses_configured_secret() {
        let temp = tempdir().unwrap();
        let runtime = TeeRuntime::with_privacy(
            temp.path().to_path_buf(),
            "batched",
            1000,
            PrivacyEncryption::CommitteeKey {
                epoch_id: 9,
                secret: [0x21u8; 32],
            },
        )
        .expect("TEE runtime with committee privacy");

        assert!(runtime.privacy_enabled());
        let submitter = privacy_submitter(&runtime).expect("privacy submitter");
        let kem = runtime.intake_kem().expect("intake kem");

        let keypair = KeyPair::from_private_key(&[0x44u8; 32]).expect("keypair");
        let tx = build_privacy_test_transaction(&keypair);
        let ciphertext =
            ContentCipher::seal(&tx.to_bytes(), &tx.hash().as_bytes(), kem).expect("seal");
        // Wrap the encoded envelope into the RPC wire-format handle exactly as
        // the RPC endpoint would after base64 decoding.
        let wire_ciphertext = neo_rpc::server::MempoolCiphertext::from_wire_bytes(
            ciphertext.encode().expect("encode envelope"),
        );

        // Roundtrip: the configured committee secret decrypts the envelope.
        let opened = submitter
            .open_encrypted(&tx.hash(), &wire_ciphertext)
            .expect("open encrypted");
        assert_eq!(opened.hash(), tx.hash());

        // A wrong claimed hash fails closed (AEAD binds the hash as AAD).
        let wrong_hash = UInt256::from_bytes(&[0xABu8; 32]).unwrap();
        assert!(
            submitter
                .open_encrypted(&wrong_hash, &wire_ciphertext)
                .is_err()
        );

        // The attestation report carries enclave measurements.
        let report = submitter.attestation_report().expect("report");
        let parsed = AttestationReport::from_bytes(&report).expect("parse report");
        assert_ne!(parsed.mrenclave, [0u8; 32]);
        assert_ne!(parsed.mrsigner, [0u8; 32]);
    }
}
