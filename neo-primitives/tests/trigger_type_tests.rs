use neo_primitives::TriggerType;
use std::str::FromStr;

#[test]
fn trigger_type_individual_bits() {
    assert_eq!(TriggerType::ON_PERSIST.bits(), 0x01);
    assert_eq!(TriggerType::POST_PERSIST.bits(), 0x02);
    assert_eq!(TriggerType::VERIFICATION.bits(), 0x20);
    assert_eq!(TriggerType::APPLICATION.bits(), 0x40);
}

#[test]
fn trigger_type_system_combination() {
    let system = TriggerType::SYSTEM;
    assert!(system.contains(TriggerType::ON_PERSIST));
    assert!(system.contains(TriggerType::POST_PERSIST));
    assert!(!system.contains(TriggerType::VERIFICATION));
    assert!(!system.contains(TriggerType::APPLICATION));
}

#[test]
fn trigger_type_all_combination() {
    let all = TriggerType::ALL;
    assert!(all.contains(TriggerType::ON_PERSIST));
    assert!(all.contains(TriggerType::POST_PERSIST));
    assert!(all.contains(TriggerType::VERIFICATION));
    assert!(all.contains(TriggerType::APPLICATION));
}

#[test]
fn trigger_type_c_sharp_style_aliases() {
    assert_eq!(TriggerType::OnPersist, TriggerType::ON_PERSIST);
    assert_eq!(TriggerType::PostPersist, TriggerType::POST_PERSIST);
    assert_eq!(TriggerType::Verification, TriggerType::VERIFICATION);
    assert_eq!(TriggerType::Application, TriggerType::APPLICATION);
    assert_eq!(TriggerType::System, TriggerType::SYSTEM);
    assert_eq!(TriggerType::All, TriggerType::ALL);
}

#[test]
fn trigger_type_from_str_case_insensitive() {
    assert_eq!(
        TriggerType::from_str("OnPersist").unwrap(),
        TriggerType::ON_PERSIST
    );
    assert_eq!(
        TriggerType::from_str("onpersist").unwrap(),
        TriggerType::ON_PERSIST
    );
    assert_eq!(
        TriggerType::from_str("ONPERSIST").unwrap(),
        TriggerType::ON_PERSIST
    );
    assert_eq!(
        TriggerType::from_str("oNpErSiSt").unwrap(),
        TriggerType::ON_PERSIST
    );
}

#[test]
fn trigger_type_from_str_all_variants() {
    assert_eq!(
        TriggerType::from_str("OnPersist").unwrap(),
        TriggerType::ON_PERSIST
    );
    assert_eq!(
        TriggerType::from_str("PostPersist").unwrap(),
        TriggerType::POST_PERSIST
    );
    assert_eq!(
        TriggerType::from_str("Verification").unwrap(),
        TriggerType::VERIFICATION
    );
    assert_eq!(
        TriggerType::from_str("Application").unwrap(),
        TriggerType::APPLICATION
    );
    assert_eq!(
        TriggerType::from_str("System").unwrap(),
        TriggerType::SYSTEM
    );
    assert_eq!(TriggerType::from_str("All").unwrap(), TriggerType::ALL);
}

#[test]
fn trigger_type_from_str_invalid() {
    assert!(TriggerType::from_str("Invalid").is_err());
    assert!(TriggerType::from_str("").is_err());
    assert!(TriggerType::from_str("OnPersist2").is_err());
}

#[test]
fn trigger_type_from_str_error_message() {
    let err = TriggerType::from_str("BadTrigger").unwrap_err();
    assert!(err.contains("unknown trigger type"));
    assert!(err.contains("BadTrigger"));
}

#[test]
fn trigger_type_union_operations() {
    let triggers = TriggerType::ON_PERSIST | TriggerType::VERIFICATION;
    assert!(triggers.contains(TriggerType::ON_PERSIST));
    assert!(triggers.contains(TriggerType::VERIFICATION));
    assert!(!triggers.contains(TriggerType::POST_PERSIST));
}

#[test]
fn trigger_type_intersection_operations() {
    let triggers1 = TriggerType::SYSTEM;
    let triggers2 = TriggerType::ON_PERSIST | TriggerType::APPLICATION;
    let intersection = triggers1 & triggers2;
    assert_eq!(intersection, TriggerType::ON_PERSIST);
}

#[test]
fn trigger_type_difference_operations() {
    let triggers = TriggerType::ALL;
    let without_verification = triggers - TriggerType::VERIFICATION;
    assert!(!without_verification.contains(TriggerType::VERIFICATION));
    assert!(without_verification.contains(TriggerType::APPLICATION));
}

#[test]
fn trigger_type_is_empty() {
    let empty = TriggerType::empty();
    assert!(empty.is_empty());
    assert!(!TriggerType::ON_PERSIST.is_empty());
}

#[test]
fn trigger_type_is_all() {
    assert!(TriggerType::ALL.is_all());
    assert!(!TriggerType::SYSTEM.is_all());
}

#[test]
fn trigger_type_clone_and_copy() {
    let trigger = TriggerType::VERIFICATION;
    let cloned = trigger;
    let copied = trigger;
    assert_eq!(trigger, cloned);
    assert_eq!(trigger, copied);
}

#[test]
fn trigger_type_debug_format() {
    let trigger = TriggerType::ON_PERSIST | TriggerType::POST_PERSIST;
    let debug_str = format!("{trigger:?}");
    assert!(debug_str.contains("ON_PERSIST"));
    assert!(debug_str.contains("POST_PERSIST"));
}

#[test]
fn trigger_type_equality() {
    assert_eq!(TriggerType::ON_PERSIST, TriggerType::ON_PERSIST);
    assert_ne!(TriggerType::ON_PERSIST, TriggerType::POST_PERSIST);
    assert_eq!(
        TriggerType::ON_PERSIST | TriggerType::POST_PERSIST,
        TriggerType::SYSTEM
    );
}

#[test]
fn trigger_type_complementary_flags() {
    let system = TriggerType::SYSTEM;
    let non_system = TriggerType::VERIFICATION | TriggerType::APPLICATION;
    let combined = system | non_system;
    assert_eq!(combined, TriggerType::ALL);
}
