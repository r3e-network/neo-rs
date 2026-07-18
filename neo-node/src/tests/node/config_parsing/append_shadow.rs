use super::*;
use crate::node::config::AppendShadowSection;

#[test]
fn append_shadow_is_disabled_by_default() {
    let config = NodeConfig::default();

    assert!(!config.storage.append_shadow.enabled);
    assert!(config.storage.append_shadow.path.is_none());
    assert!(config.storage.append_shadow.max_index_memory_mb.is_none());
    assert_eq!(
        config.storage.append_shadow.max_index_memory_bytes(),
        AppendShadowSection::DEFAULT_MAX_INDEX_MEMORY_BYTES
    );
    validate_config(&config, 0x3554_334E).expect("default config validates");
}

#[test]
fn append_shadow_parses_enabled_section_with_aliases() {
    let config: NodeConfig = toml::from_str(
        r#"
[storage]
backend = "mdbx"

[storage.append_shadow]
Enabled = true
Path = "ShadowPacks_{0}"
MaxIndexMemoryMb = 128
"#,
    )
    .expect("parse append shadow config");

    let shadow = &config.storage.append_shadow;
    assert!(shadow.enabled);
    assert_eq!(
        shadow.path.as_deref(),
        Some(std::path::Path::new("ShadowPacks_{0}"))
    );
    assert_eq!(shadow.max_index_memory_mb, Some(128));
    assert_eq!(shadow.max_index_memory_bytes(), 128 * 1024 * 1024);
}

#[test]
fn append_shadow_validation_accepts_coordinated_state_service() {
    let temp = tempfile::tempdir().expect("temp shadow root");
    let config: NodeConfig = toml::from_str(&format!(
        r#"
[storage]
backend = "mdbx"

[storage.append_shadow]
enabled = true
path = "{}"

[state_service]
enabled = true
coordinated = true
"#,
        temp.path().join("shadow-packs").display()
    ))
    .expect("parse append shadow config");

    validate_config(&config, 0x3554_334E).expect("coordinated shadow config validates");
}

#[test]
fn append_shadow_validation_rejects_missing_path() {
    let config: NodeConfig = toml::from_str(
        r#"
[storage]
backend = "mdbx"

[storage.append_shadow]
enabled = true

[state_service]
enabled = true
"#,
    )
    .expect("parse append shadow config");

    let err = validate_config(&config, 0x3554_334E).expect_err("missing path must fail");
    assert!(
        err.to_string().contains("[storage.append_shadow].path"),
        "unexpected error: {err}"
    );
}

#[test]
fn append_shadow_validation_rejects_memory_only_backend() {
    let temp = tempfile::tempdir().expect("temp shadow root");
    let config: NodeConfig = toml::from_str(&format!(
        r#"
[storage]
backend = "memory"

[storage.append_shadow]
enabled = true
path = "{}"

[state_service]
enabled = true
"#,
        temp.path().join("shadow-packs").display()
    ))
    .expect("parse append shadow config");

    let err = validate_config(&config, 0x3554_334E).expect_err("memory backend must fail");
    assert!(err.to_string().contains("MDBX"), "unexpected error: {err}");
}

#[test]
fn append_shadow_validation_rejects_read_only_storage() {
    let temp = tempfile::tempdir().expect("temp shadow root");
    let config: NodeConfig = toml::from_str(&format!(
        r#"
[storage]
backend = "mdbx"
read_only = true

[storage.append_shadow]
enabled = true
path = "{}"

[state_service]
enabled = true
"#,
        temp.path().join("shadow-packs").display()
    ))
    .expect("parse append shadow config");

    let err = validate_config(&config, 0x3554_334E).expect_err("read-only must fail");
    assert!(
        err.to_string().contains("read_only"),
        "unexpected error: {err}"
    );
}

#[test]
fn append_shadow_validation_requires_coordinated_state_service() {
    let temp = tempfile::tempdir().expect("temp shadow root");
    for state_service in [
        "[state_service]\nenabled = false".to_owned(),
        "[state_service]\nenabled = true\ncoordinated = false\npath = \"./state-aux\"".to_owned(),
    ] {
        let config: NodeConfig = toml::from_str(&format!(
            r#"
[storage]
backend = "mdbx"

[storage.append_shadow]
enabled = true
path = "{}"

{state_service}
"#,
            temp.path().join("shadow-packs").display()
        ))
        .expect("parse append shadow config");

        let err = validate_config(&config, 0x3554_334E)
            .expect_err("shadow requires a coordinated state service");
        assert!(
            err.to_string().contains("[state_service]"),
            "unexpected error: {err}"
        );
    }
}

#[test]
fn append_shadow_validation_rejects_zero_index_memory_bound() {
    let temp = tempfile::tempdir().expect("temp shadow root");
    let config: NodeConfig = toml::from_str(&format!(
        r#"
[storage]
backend = "mdbx"

[storage.append_shadow]
enabled = true
path = "{}"
max_index_memory_mb = 0

[state_service]
enabled = true
"#,
        temp.path().join("shadow-packs").display()
    ))
    .expect("parse append shadow config");

    let err = validate_config(&config, 0x3554_334E).expect_err("zero bound must fail");
    assert!(
        err.to_string().contains("max_index_memory_mb"),
        "unexpected error: {err}"
    );
}
