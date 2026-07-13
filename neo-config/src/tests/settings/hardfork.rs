use super::*;

#[test]
fn test_hardfork_manager() {
    let mut manager = HardforkManager::new();
    assert!(!manager.is_enabled(Hardfork::HfAspidochelone, 0));
    assert!(!manager.is_enabled(Hardfork::HfBasilisk, 0));
    // Register a hardfork
    manager.register(Hardfork::HfAspidochelone, 100);
    // Test hardfork activation
    assert!(!manager.is_enabled(Hardfork::HfAspidochelone, 99));
    assert!(manager.is_enabled(Hardfork::HfAspidochelone, 100));
    assert!(manager.is_enabled(Hardfork::HfAspidochelone, 101));
    // Test unregistered hardfork
    assert!(!manager.is_enabled(Hardfork::HfBasilisk, 1000));
    assert!(!manager.is_enabled(Hardfork::HfHuyao, 1000));
}
#[test]
fn test_mainnet_hardforks() {
    let manager = HardforkManager::mainnet();
    let expected = [
        (Hardfork::HfAspidochelone, 1_730_000),
        (Hardfork::HfBasilisk, 4_120_000),
        (Hardfork::HfCockatrice, 5_450_000),
        (Hardfork::HfDomovoi, 5_570_000),
        (Hardfork::HfEchidna, 7_300_000),
        (Hardfork::HfFaun, 8_800_000),
        (Hardfork::HfGorgon, 12_020_000),
    ];
    assert_eq!(manager.get_hardforks().len(), expected.len());
    for (hardfork, height) in expected {
        assert_eq!(manager.get_hardforks().get(&hardfork), Some(&height));
        assert!(!manager.is_enabled(hardfork, height - 1));
        assert!(manager.is_enabled(hardfork, height));
    }
    // Neo Node v3.10.1 schedules MainNet through Gorgon. Huyao is not present
    // in the operational config and must remain disabled at every height.
    assert!(!manager.is_enabled(Hardfork::HfHuyao, u32::MAX));
}
#[test]
fn test_testnet_hardforks() {
    let manager = HardforkManager::testnet();
    let expected = [
        (Hardfork::HfAspidochelone, 210_000),
        (Hardfork::HfBasilisk, 2_680_000),
        (Hardfork::HfCockatrice, 3_967_000),
        (Hardfork::HfDomovoi, 4_144_000),
        (Hardfork::HfEchidna, 5_870_000),
        (Hardfork::HfFaun, 12_960_000),
        (Hardfork::HfGorgon, 17_960_000),
    ];
    assert_eq!(manager.get_hardforks().len(), expected.len());
    for (hardfork, height) in expected {
        assert_eq!(manager.get_hardforks().get(&hardfork), Some(&height));
        assert!(!manager.is_enabled(hardfork, height - 1));
        assert!(manager.is_enabled(hardfork, height));
    }
    // Neo Node v3.10.1 schedules TestNet through Gorgon. Huyao is not present
    // in the operational config and must remain disabled at every height.
    assert!(!manager.is_enabled(Hardfork::HfHuyao, u32::MAX));
}
