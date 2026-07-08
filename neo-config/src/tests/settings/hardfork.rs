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
    assert!(manager.is_enabled(Hardfork::HfAspidochelone, 1730000));
    assert!(!manager.is_enabled(Hardfork::HfAspidochelone, 1729999));
    assert!(manager.is_enabled(Hardfork::HfBasilisk, 4120000));
    assert!(!manager.is_enabled(Hardfork::HfBasilisk, 4119999));
    assert!(manager.is_enabled(Hardfork::HfEchidna, 7300000));
    assert!(!manager.is_enabled(Hardfork::HfEchidna, 7299999));
    assert!(manager.is_enabled(Hardfork::HfFaun, 8800000));
    assert!(!manager.is_enabled(Hardfork::HfFaun, 8799999));
    assert_eq!(
        manager.get_hardforks().get(&Hardfork::HfFaun),
        Some(&8_800_000)
    );
    // HfGorgon and HfHuyao are defined in Neo v3.10.1 but not scheduled in
    // MainNet config.
    assert!(!manager.is_enabled(Hardfork::HfGorgon, u32::MAX));
    assert!(!manager.is_enabled(Hardfork::HfHuyao, u32::MAX));
}
#[test]
fn test_testnet_hardforks() {
    let manager = HardforkManager::testnet();
    assert!(manager.is_enabled(Hardfork::HfAspidochelone, 210000));
    assert!(!manager.is_enabled(Hardfork::HfAspidochelone, 209999));
    assert!(manager.is_enabled(Hardfork::HfBasilisk, 2680000));
    assert!(!manager.is_enabled(Hardfork::HfBasilisk, 2679999));
    assert!(manager.is_enabled(Hardfork::HfEchidna, 5870000));
    assert!(!manager.is_enabled(Hardfork::HfEchidna, 5869999));
    assert!(manager.is_enabled(Hardfork::HfFaun, 12960000));
    assert!(!manager.is_enabled(Hardfork::HfFaun, 12959999));
    assert_eq!(
        manager.get_hardforks().get(&Hardfork::HfFaun),
        Some(&12_960_000)
    );
    // HfGorgon and HfHuyao are defined in Neo v3.10.1 but not scheduled in
    // TestNet config.
    assert!(!manager.is_enabled(Hardfork::HfGorgon, u32::MAX));
    assert!(!manager.is_enabled(Hardfork::HfHuyao, u32::MAX));
}
