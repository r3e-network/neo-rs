use super::*;

#[test]
fn fixed_schedule_tracks_activation_without_hashing() {
    let schedule = HardforkSchedule::new();
    assert!(!schedule.is_active(Hardfork::HfAspidochelone, 0));

    let schedule = schedule.with_activation(Hardfork::HfAspidochelone, 100);

    assert!(!schedule.is_active(Hardfork::HfAspidochelone, 99));
    assert!(schedule.is_active(Hardfork::HfAspidochelone, 100));
    assert!(schedule.is_active(Hardfork::HfAspidochelone, 101));
    assert!(!schedule.is_active(Hardfork::HfBasilisk, u32::MAX));
}

#[test]
fn mainnet_schedule_matches_neo_v3101() {
    let schedule = HardforkSchedule::mainnet();
    let expected = [
        (Hardfork::HfAspidochelone, 1_730_000),
        (Hardfork::HfBasilisk, 4_120_000),
        (Hardfork::HfCockatrice, 5_450_000),
        (Hardfork::HfDomovoi, 5_570_000),
        (Hardfork::HfEchidna, 7_300_000),
        (Hardfork::HfFaun, 8_800_000),
        (Hardfork::HfGorgon, 12_020_000),
    ];

    assert_eq!(schedule.len(), expected.len());
    assert!(schedule.validate().is_ok());
    for (hardfork, height) in expected {
        assert_eq!(schedule.activation_height(hardfork), Some(height));
        assert!(!schedule.is_active(hardfork, height - 1));
        assert!(schedule.is_active(hardfork, height));
    }
    assert!(!schedule.is_defined(Hardfork::HfHuyao));
}

#[test]
fn testnet_schedule_matches_neo_v3101() {
    let schedule = HardforkSchedule::testnet();
    let expected = [
        (Hardfork::HfAspidochelone, 210_000),
        (Hardfork::HfBasilisk, 2_680_000),
        (Hardfork::HfCockatrice, 3_967_000),
        (Hardfork::HfDomovoi, 4_144_000),
        (Hardfork::HfEchidna, 5_870_000),
        (Hardfork::HfFaun, 12_960_000),
        (Hardfork::HfGorgon, 17_960_000),
    ];

    assert_eq!(schedule.len(), expected.len());
    assert!(schedule.validate().is_ok());
    for (hardfork, height) in expected {
        assert_eq!(schedule.activation_height(hardfork), Some(height));
    }
    assert!(!schedule.is_defined(Hardfork::HfHuyao));
}

#[test]
fn omitted_leading_forks_follow_csharp_semantics() {
    let schedule = HardforkSchedule::new()
        .with_activation(Hardfork::HfEchidna, 100)
        .with_omitted_leading_at_genesis();

    for hardfork in [
        Hardfork::HfAspidochelone,
        Hardfork::HfBasilisk,
        Hardfork::HfCockatrice,
        Hardfork::HfDomovoi,
    ] {
        assert_eq!(schedule.activation_height(hardfork), Some(0));
    }
    assert_eq!(schedule.activation_height(Hardfork::HfEchidna), Some(100));
    assert!(!schedule.is_defined(Hardfork::HfFaun));
}

#[test]
fn active_set_is_a_stable_declaration_order_bitmask() {
    let active = HardforkSchedule::mainnet().active_at(5_500_000);

    assert!(active.contains(Hardfork::HfAspidochelone));
    assert!(active.contains(Hardfork::HfBasilisk));
    assert!(active.contains(Hardfork::HfCockatrice));
    assert!(!active.contains(Hardfork::HfDomovoi));
    assert_eq!(active.iter().collect::<Vec<_>>(), Hardfork::ALL[..3]);
}

#[test]
fn validation_rejects_gaps_and_decreasing_heights() {
    let gap = HardforkSchedule::new()
        .with_activation(Hardfork::HfAspidochelone, 1)
        .with_activation(Hardfork::HfCockatrice, 3);
    assert!(matches!(
        gap.validate(),
        Err(HardforkScheduleError::Gap {
            configured: Hardfork::HfCockatrice,
            missing: Hardfork::HfBasilisk,
        })
    ));

    let decreasing = HardforkSchedule::new()
        .with_activation(Hardfork::HfAspidochelone, 10)
        .with_activation(Hardfork::HfBasilisk, 9);
    assert!(matches!(
        decreasing.validate(),
        Err(HardforkScheduleError::NonMonotonic {
            hardfork: Hardfork::HfBasilisk,
            height: 9,
            previous_height: 10,
        })
    ));
}
