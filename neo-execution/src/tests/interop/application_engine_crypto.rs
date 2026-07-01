use super::*;
use neo_config::{Hardfork, ProtocolSettings};
use neo_primitives::TriggerType;
use neo_storage::DataCache;
use std::sync::Arc;

fn engine_with_gorgon_active() -> ApplicationEngine {
    let mut settings = ProtocolSettings::default();
    settings.hardforks.insert(Hardfork::HfGorgon, 0);
    ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        None,
        settings,
        crate::application_engine::TEST_MODE_GAS,
        None,
    )
    .expect("application engine")
}

#[test]
fn wrong_length_signature_returns_false_even_with_gorgon_configured_like_csharp_v3100() {
    let engine = engine_with_gorgon_active();
    let public_key =
        hex::decode("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c")
            .expect("public key hex");

    assert_eq!(
        engine.verify_signature(b"message", &public_key, &[0u8; 63]),
        Ok(false)
    );
}
