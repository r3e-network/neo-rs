//! Encrypted transaction submission endpoint (privacy mode, strictly opt-in).
//!
//! The `sendrawtransactionencrypted` method exists only when the node
//! explicitly enables privacy mode and installs an
//! [`EncryptedTransactionSubmitter`] into the RPC server. Without an installed
//! submitter the method is never registered, so the RPC method surface is
//! byte-identical to the pre-privacy behavior.
//!
//! Wire format (mirrors the repo convention of base64 payloads plus hex
//! hashes):
//! - parameter 1: transaction hash (hex, same parsing as other hash params)
//! - parameter 2: `MempoolCiphertext` envelope (base64)
//!
//! The transaction hash is required before decryption because the AEAD layers
//! bind the ciphertext to the hash as associated data, so the caller must
//! commit to the hash up front.
//!
//! On success the response is the relay result plus the serialized enclave
//! attestation report (base64), including the enclave measurements, so the
//! client can verify which enclave code decrypted its transaction.

use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::{
    expect_base64_param_with_decode_message, expect_hash_param, internal_error, invalid_params,
};
use crate::server::rpc_relay;
use crate::server::rpc_server::{RpcHandler, RpcServer};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_primitives::UInt256;
use neo_protocol::payloads::transaction::Transaction;
use serde_json::{Value, json};
use std::sync::Arc;

/// Encrypted mempool transaction envelope (privacy mode).
///
/// This is a stub implementation that accepts the wire format but does not
/// provide actual TEE (Trusted Execution Environment) decryption.
///
/// Production deployment requires:
/// - TEE integration (Intel SGX, AMD SEV, or ARM TrustZone)
/// - AEAD decryption with transaction hash as associated data
/// - Attestation report generation for client verification
/// - Key management and secure enclave initialization
///
/// The stub allows the RPC endpoint to be tested without TEE hardware,
/// enabling development of the surrounding infrastructure (RPC handlers,
/// configuration, client tooling) in parallel with TEE integration.
#[derive(Clone)]
pub struct MempoolCiphertext {
    data: Vec<u8>,
}

impl MempoolCiphertext {
    fn decode(bytes: &[u8]) -> Result<Self, String> {
        Ok(Self {
            data: bytes.to_vec(),
        })
    }
}

/// Host-side decrypting submitter installed by neo-node when privacy mode is
/// explicitly enabled in configuration.
///
/// The implementation is provided by the node (see `neo-node::tee_integration`)
/// and owns the configured key encapsulation plus the enclave attestation
/// service. neo-rpc itself stays free of node-specific wiring.
pub trait EncryptedTransactionSubmitter: Send + Sync {
    /// Decrypts `ciphertext` under the configured key encapsulation, binding it
    /// to the claimed transaction hash, and returns the decoded transaction.
    ///
    /// Fails closed (Err) on any envelope, KEM, AEAD, or transaction decode
    /// problem; never returns partially decrypted data.
    fn open_encrypted(
        &self,
        tx_hash: &UInt256,
        ciphertext: &MempoolCiphertext,
    ) -> Result<Arc<Transaction>, String>;

    /// Serialized enclave attestation report (including measurements) returned
    /// to submitting clients.
    fn attestation_report(&self) -> Result<Vec<u8>, String>;
}

/// Privacy-mode RPC method registration.
pub struct RpcServerPrivacy;

impl RpcServerPrivacy {
    /// Handlers for the opt-in encrypted submission endpoint.
    pub fn register_handlers() -> Vec<RpcHandler> {
        super::rpc_handlers![
            "sendrawtransactionencrypted" => Self::send_raw_transaction_encrypted,
        ]
    }

    fn send_raw_transaction_encrypted(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        const METHOD: &str = "sendrawtransactionencrypted";

        let submitter = server.privacy_submitter().ok_or_else(|| {
            invalid_params("sendrawtransactionencrypted requires privacy mode to be enabled")
        })?;

        let tx_hash = expect_hash_param(params, 0, METHOD)?;
        let raw = expect_base64_param_with_decode_message(
            params,
            1,
            METHOD,
            "Invalid encrypted transaction payload",
        )?;
        let ciphertext = MempoolCiphertext::decode(&raw).map_err(|err| {
            invalid_params(format!("Invalid encrypted transaction payload: {err}"))
        })?;

        let transaction = submitter
            .open_encrypted(&tx_hash, &ciphertext)
            .map_err(|err| invalid_params(format!("Encrypted transaction rejected: {err}")))?;

        let relay_result = rpc_relay::with_relay_responder(server, move |sender| {
            server
                .system()
                .tx_router_actor()
                .try_enqueue_preverify_from((*transaction).clone(), true, Some(sender))
                .map_err(|err| internal_error(err.to_string()))
        })?;
        let mut result = rpc_relay::map_relay_result(relay_result)?;

        let report = submitter
            .attestation_report()
            .map_err(|err| internal_error(format!("Attestation report unavailable: {err}")))?;
        if let Some(object) = result.as_object_mut() {
            object.insert(
                "attestationreport".to_string(),
                json!(BASE64_STANDARD.encode(report)),
            );
        }

        Ok(result)
    }
}

// TODO: Re-enable tests when neo_tee crate is available
#[cfg(all(test, feature = "neo_tee"))]
mod tests {
    use super::*;
    use crate::server::rpc_error::RpcError;
    use crate::server::rpc_server_node::RpcServerNode;
    use crate::server::rpc_server_settings::RpcServerConfig;
    use neo_wallets::KeyPair;
    use neo_primitives::{Verifiable, WitnessScope};
    use neo_protocol::ProtocolSettings;
    use neo_protocol::get_sign_data_vec;
    use neo_protocol::payloads::Signer;
    use neo_protocol::witness::Witness;
    use neo_system::NeoSystem;
    // TODO: Uncomment when neo_tee crate is available
    // use neo_tee::attestation::{AttestationReport, AttestationService};
    // use neo_tee::enclave::{EnclaveConfig, TeeEnclave};
    // use neo_tee::mempool::{CommitteeEpochKey, ContentCipher, KeyEncapsulation};
    use neo_vm::OpCode;
    use num_bigint::BigInt;

    /// Committee-key test submitter backed by real neo-tee crypto and a
    /// simulated attestation service, mirroring the node-side implementation.
    struct TestSubmitter {
        kem: KeyEncapsulation,
        attestation: AttestationService,
        _enclave: Arc<TeeEnclave>,
    }

    impl TestSubmitter {
        fn new(temp: &std::path::Path) -> Self {
            let enclave = Arc::new(TeeEnclave::new(EnclaveConfig {
                sealed_data_path: temp.to_path_buf(),
                debug_mode: true,
                simulation: true,
                ..Default::default()
            }));
            enclave.initialize().expect("enclave init");
            let attestation = AttestationService::new(enclave.clone()).expect("attestation");
            Self {
                kem: KeyEncapsulation::CommitteeKey(CommitteeEpochKey::from_secret(
                    7,
                    [0x11u8; 32],
                )),
                attestation,
                _enclave: enclave,
            }
        }
    }

    impl EncryptedTransactionSubmitter for TestSubmitter {
        fn open_encrypted(
            &self,
            tx_hash: &UInt256,
            ciphertext: &MempoolCiphertext,
        ) -> Result<Arc<Transaction>, String> {
            let plaintext = ContentCipher::open(ciphertext, &tx_hash.as_bytes(), &self.kem)
                .map_err(|e| e.to_string())?;
            let transaction = Transaction::from_bytes(&plaintext).map_err(|e| e.to_string())?;
            if transaction.hash() != *tx_hash {
                return Err(
                    "decrypted transaction hash does not match the claimed hash".to_string()
                );
            }
            Ok(Arc::new(transaction))
        }

        fn attestation_report(&self) -> Result<Vec<u8>, String> {
            let report = self
                .attestation
                .generate_report(b"neo-rpc-privacy-test")
                .map_err(|e| e.to_string())?;
            Ok(report.to_bytes())
        }
    }

    fn find_handler<'a>(handlers: &'a [RpcHandler], name: &str) -> &'a RpcHandler {
        handlers
            .iter()
            .find(|handler| handler.descriptor().name.eq_ignore_ascii_case(name))
            .expect("handler present")
    }

    fn build_signed_transaction(
        settings: &ProtocolSettings,
        keypair: &KeyPair,
        nonce: u32,
        system_fee: i64,
    ) -> Transaction {
        let mut tx = Transaction::new();
        tx.set_nonce(nonce);
        tx.set_network_fee(1_0000_0000);
        tx.set_system_fee(system_fee);
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

    fn mint_gas(
        store: &mut neo_storage::persistence::StoreCache,
        settings: &ProtocolSettings,
        account: neo_primitives::UInt160,
        amount: BigInt,
    ) {
        use neo_native_contracts::native::GasToken;
        use neo_primitives::TriggerType;
        use neo_smart_contract::application_engine::ApplicationEngine;
        use neo_storage::persistence::transaction::apply_tracked_items;

        let snapshot = Arc::new(store.data_cache().clone());
        let mut container = Transaction::new();
        container.set_signers(vec![Signer::new(account, WitnessScope::GLOBAL)]);
        container.add_witness(Witness::new());
        let script_container: Arc<dyn Verifiable> = Arc::new(container);
        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            Some(script_container),
            snapshot,
            None,
            settings.clone(),
            400_000_000,
            None,
            neo_native_contracts::native::composition::default_native_services(),
        )
        .expect("engine");

        let gas = GasToken::new();
        gas.mint(&mut engine, &account, &amount, false)
            .expect("mint");
        let tracked = engine.snapshot_cache().tracked_items();
        apply_tracked_items(store, tracked);
    }

    fn sealed_params(tx: &Transaction, kem: &KeyEncapsulation) -> [Value; 2] {
        let ciphertext =
            ContentCipher::seal(&tx.to_bytes(), &tx.hash().as_bytes(), kem).expect("seal");
        let encoded = BASE64_STANDARD.encode(ciphertext.encode().expect("encode"));
        [Value::String(tx.hash().to_string()), Value::String(encoded)]
    }

    #[test]
    fn encrypted_endpoint_absent_from_standard_node_handlers() {
        let handlers = RpcServerNode::register_handlers();
        assert!(
            handlers.iter().all(|handler| !handler
                .descriptor()
                .name
                .eq_ignore_ascii_case("sendrawtransactionencrypted")),
            "privacy endpoint must not be registered with the standard node handler set"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn encrypted_submission_fails_closed_without_submitter() {
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system, RpcServerConfig::default());
        let handlers = RpcServerPrivacy::register_handlers();
        let handler = find_handler(&handlers, "sendrawtransactionencrypted");

        let params = [
            Value::String("0".repeat(64)),
            Value::String("AAAA".to_string()),
        ];
        let err = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
            .expect_err("no submitter installed");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
        assert!(
            rpc_error
                .data()
                .unwrap_or_default()
                .contains("privacy mode")
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn send_raw_transaction_encrypted_accepts_valid_ciphertext() {
        let settings = ProtocolSettings::default();
        let system = NeoSystem::new(settings.clone(), None, None).expect("system to start");
        let mut server = RpcServer::new(system.clone(), RpcServerConfig::default());

        let temp = tempfile::tempdir().expect("tempdir");
        let submitter = Arc::new(TestSubmitter::new(temp.path()));
        let kem = if let KeyEncapsulation::CommitteeKey(key) = &submitter.kem {
            KeyEncapsulation::CommitteeKey(CommitteeEpochKey::from_secret(
                key.epoch_id(),
                key.public_key(),
            ))
        } else {
            unreachable!("test submitter uses the committee KEM");
        };
        server.set_privacy_submitter(Some(submitter));

        let keypair = KeyPair::from_private_key(&[0x44u8; 32]).expect("keypair");
        let account = keypair.get_script_hash();
        let mut store = system.context().store_snapshot_cache();
        mint_gas(
            &mut store,
            &settings,
            account,
            BigInt::from(50_0000_0000i64),
        );
        store.commit();

        let tx = build_signed_transaction(&settings, &keypair, 1, 0);
        let params = sealed_params(&tx, &kem);
        let handlers = RpcServerPrivacy::register_handlers();
        let handler = find_handler(&handlers, "sendrawtransactionencrypted");

        let result = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
            .expect("encrypted submission accepted");
        let hash = result.get("hash").and_then(Value::as_str).expect("hash");
        assert_eq!(hash, tx.hash().to_string());

        // The attestation report must be present and carry enclave measurements.
        let report_b64 = result
            .get("attestationreport")
            .and_then(Value::as_str)
            .expect("attestation report");
        let report_bytes = BASE64_STANDARD.decode(report_b64).expect("report base64");
        let report = AttestationReport::from_bytes(&report_bytes).expect("report decode");
        assert_ne!(report.mrenclave, [0u8; 32]);
        assert_ne!(report.mrsigner, [0u8; 32]);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn send_raw_transaction_encrypted_rejects_tampered_ciphertext() {
        let settings = ProtocolSettings::default();
        let system = NeoSystem::new(settings.clone(), None, None).expect("system to start");
        let mut server = RpcServer::new(system, RpcServerConfig::default());

        let temp = tempfile::tempdir().expect("tempdir");
        let submitter = Arc::new(TestSubmitter::new(temp.path()));
        let kem = if let KeyEncapsulation::CommitteeKey(key) = &submitter.kem {
            KeyEncapsulation::CommitteeKey(CommitteeEpochKey::from_secret(
                key.epoch_id(),
                key.public_key(),
            ))
        } else {
            unreachable!("test submitter uses the committee KEM")
        };
        server.set_privacy_submitter(Some(submitter));

        let keypair = KeyPair::from_private_key(&[0x44u8; 32]).expect("keypair");
        let tx = build_signed_transaction(&settings, &keypair, 2, 0);
        let mut encoded = ContentCipher::seal(&tx.to_bytes(), &tx.hash().as_bytes(), &kem)
            .expect("seal")
            .encode()
            .expect("encode");
        // Corrupt the payload ciphertext (last byte) so the AEAD tag fails.
        let last = encoded.len() - 1;
        encoded[last] ^= 0xFF;
        let params = [
            Value::String(tx.hash().to_string()),
            Value::String(BASE64_STANDARD.encode(encoded)),
        ];

        let handlers = RpcServerPrivacy::register_handlers();
        let handler = find_handler(&handlers, "sendrawtransactionencrypted");
        let err = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
            .expect_err("tampered ciphertext must fail closed");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn send_raw_transaction_encrypted_rejects_wrong_claimed_hash() {
        let settings = ProtocolSettings::default();
        let system = NeoSystem::new(settings.clone(), None, None).expect("system to start");
        let mut server = RpcServer::new(system, RpcServerConfig::default());

        let temp = tempfile::tempdir().expect("tempdir");
        let submitter = Arc::new(TestSubmitter::new(temp.path()));
        let kem = if let KeyEncapsulation::CommitteeKey(key) = &submitter.kem {
            KeyEncapsulation::CommitteeKey(CommitteeEpochKey::from_secret(
                key.epoch_id(),
                key.public_key(),
            ))
        } else {
            unreachable!("test submitter uses the committee KEM")
        };
        server.set_privacy_submitter(Some(submitter));

        let keypair = KeyPair::from_private_key(&[0x44u8; 32]).expect("keypair");
        let tx = build_signed_transaction(&settings, &keypair, 3, 0);
        let mut params = sealed_params(&tx, &kem);
        // Claim a different hash: the AEAD AAD binding must reject it.
        params[0] = Value::String(UInt256::from_bytes(&[0xABu8; 32]).unwrap().to_string());

        let handlers = RpcServerPrivacy::register_handlers();
        let handler = find_handler(&handlers, "sendrawtransactionencrypted");
        let err = tokio::task::block_in_place(|| (handler.callback())(&server, &params))
            .expect_err("wrong claimed hash must fail closed");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
    }
}
