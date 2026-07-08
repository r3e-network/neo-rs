use super::super::{OracleService, OracleServiceError, OracleServiceSettings, OracleTask};
use neo_config::ProtocolSettings;
use neo_crypto::{ECCurve, ECPoint, Secp256r1Crypto};
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_native_contracts::StandardNativeProvider;
use neo_payloads::Transaction;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::SystemTime;

fn sample_point(byte: u8) -> ECPoint {
    let mut private_key = [0u8; 32];
    private_key[31] = byte.max(1);
    let public_key = Secp256r1Crypto::derive_public_key(&private_key).expect("derive test key");
    ECPoint::decode_compressed_with_curve(ECCurve::secp256r1(), &public_key)
        .expect("static test key")
}

fn oracle_service() -> OracleService {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let _guard = runtime.enter();
    let settings = Arc::new(ProtocolSettings::testnet());
    let system =
        Arc::new(neo_system::Node::new(Arc::clone(&settings), None, None).expect("neo system"));
    let oracle_settings = OracleServiceSettings {
        network: settings.network,
        ..Default::default()
    };

    OracleService::new(
        oracle_settings,
        system.clone(),
        system.clone(),
        system.clone(),
        Arc::new(StandardNativeProvider::new()) as Arc<dyn NativeContractProvider>,
    )
    .expect("oracle service")
}

#[test]
fn add_response_tx_sign_reports_missing_backup_tx() {
    let service = oracle_service();
    service.pending_queue.lock().insert(
        7,
        OracleTask {
            tx: Some(Transaction::new()),
            backup_tx: None,
            signs: BTreeMap::new(),
            backup_signs: BTreeMap::new(),
            timestamp: SystemTime::now(),
        },
    );

    let snapshot = service.snapshot_cache();
    let err = service
        .add_response_tx_sign(&snapshot, 7, sample_point(1), vec![0x01], None, None, None)
        .expect_err("missing backup transaction must be reported");

    assert!(matches!(err, OracleServiceError::Processing(message) if message.contains("backup")));
}
