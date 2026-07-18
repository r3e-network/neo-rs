use super::*;

#[test]
fn state_packs_are_disabled_by_default() {
    let config = NodeConfig::default();
    assert!(!config.storage.state_packs.enabled);
    assert!(config.storage.state_packs.path.is_none());
    assert!(!config.storage.state_packs.random_point_mmap);
}

#[test]
fn state_packs_parse_and_validate_only_the_safe_authoritative_profile() {
    let config: NodeConfig = toml::from_str(
        r#"
[storage]
backend = "mdbx"
data_dir = "Data_{0}"

[storage.state_packs]
enabled = true
path = "StatePacks_{0}"
max_index_memory_mb = 768
random_point_mmap = true

[state_service]
enabled = true
coordinated = true
full_state = true
track_during_catchup = true
    defer_full_state_finalization = true
"#,
    )
    .expect("parse authoritative state packs");
    assert!(config.storage.state_packs.enabled);
    assert!(config.storage.state_packs.random_point_mmap);
    assert_eq!(
        config.storage.state_packs.max_index_memory_bytes(),
        768 * 1024 * 1024
    );
    assert!(config.state_service.defer_full_state_finalization);
    validate_config(&config, 0x334F_454E).expect("validate authoritative profile");
}

#[test]
fn state_packs_reject_incomplete_or_conflicting_profiles() {
    let input = r#"
[storage]
backend = "mdbx"
data_dir = "Data_{0}"

[storage.state_packs]
enabled = true
path = "StatePacks_{0}"

[state_service]
enabled = true
coordinated = true
"#;
    let config: NodeConfig = toml::from_str(input).expect("parse invalid profile");
    let error = validate_config(&config, 0x334F_454E)
        .expect_err("incomplete authoritative profile must fail");
    assert!(
        error.to_string().contains("coordinated full-state"),
        "{error}"
    );
}
