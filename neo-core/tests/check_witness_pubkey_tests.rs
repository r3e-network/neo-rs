//! `System.Runtime.CheckWitness` public-key validation.
//!
//! C# `CheckWitness` decodes 33-byte arguments as an `ECPoint` and faults on
//! an invalid curve point. Rust previously hashed any 33 bytes into a
//! signature-redeem script and silently returned `false`.

use neo_core::persistence::DataCache;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::application_engine::{ApplicationEngine, TEST_MODE_GAS};
use neo_core::smart_contract::trigger_type::TriggerType;
use std::sync::Arc;

fn engine() -> ApplicationEngine {
    ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        None,
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        None,
    )
    .expect("application engine")
}

#[test]
fn invalid_33_byte_public_key_is_rejected() {
    let engine = engine();
    assert!(engine.pubkey_to_hash(&[0u8; 33]).is_err());
    assert!(engine.pubkey_to_hash(&[0x04; 33]).is_err());
}

#[test]
fn valid_compressed_public_key_is_accepted() {
    let engine = engine();
    let generator =
        hex::decode("036b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296")
            .expect("hex");
    let hash = engine.pubkey_to_hash(&generator).expect("valid point");
    assert!(!hash.is_zero());
}
