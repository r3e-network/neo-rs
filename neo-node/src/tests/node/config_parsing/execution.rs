use neo_execution::specialization::{
    FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_ID, FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_VERSION,
    SpecializationRouteDecision,
};

use super::*;

#[test]
fn specialization_shadow_is_absent_by_default() {
    let config = NodeConfig::default();

    assert!(
        config
            .execution
            .specialization_shadow
            .build_runtime(true)
            .expect("default shadow config")
            .is_none(),
        "the ordinary path must not allocate a specialization control"
    );
}

#[test]
fn optimistic_signature_verification_is_absent_by_default() {
    let config = NodeConfig::default();
    assert!(
        config
            .execution
            .optimistic_signature_verification
            .build_runtime(true)
            .expect("default optimistic signature config")
            .is_none(),
        "the ordinary path must not allocate verification workers"
    );
}

#[test]
fn optimistic_signature_verification_builds_a_bounded_pool_only_when_enabled() {
    let config: NodeConfig = toml::from_str(
        r#"
[execution.optimistic_signature_verification]
enabled = true
workers = 2
queue_capacity = 8
"#,
    )
    .expect("parse optimistic signature config");
    let runtime = config
        .execution
        .optimistic_signature_verification
        .build_runtime(true)
        .expect("validate optimistic signature config")
        .expect("enabled optimistic signature runtime");
    assert_eq!(runtime.pool.window(), 10);
}

#[test]
fn optimistic_signature_verification_rejects_zero_limits() {
    let config: NodeConfig = toml::from_str(
        r#"
[execution.optimistic_signature_verification]
workers = 0
"#,
    )
    .expect("parse zero worker config");
    let error = validate_config(&config, 0x334F_454E).expect_err("zero workers must fail");
    assert!(error.to_string().contains("at least one worker"));
}

#[test]
fn specialization_shadow_builds_only_the_exact_flamingo_v1_shadow_route() {
    let config: NodeConfig = toml::from_str(
        r#"
[execution.specialization_shadow]
enabled = true
strict_replay = true
candidates = ["flamingo_factory_pair_key_v1"]
max_reproducers = 4
max_reproducer_bytes = 65536
max_artifact_bytes = 1048576
"#,
    )
    .expect("parse bounded shadow config");

    let runtime = config
        .execution
        .specialization_shadow
        .build_runtime(true)
        .expect("validate shadow config")
        .expect("enabled shadow runtime");
    let snapshot = runtime.control.snapshot();

    assert!(snapshot.enabled);
    assert!(snapshot.strict_replay);
    assert_eq!(snapshot.candidates.len(), 1);
    assert_eq!(runtime.artifact_limits.max_retained_bytes, 1_048_576);
    assert_eq!(
        runtime.control.route(
            FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_ID,
            FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_VERSION,
        ),
        SpecializationRouteDecision::Shadow
    );
}

#[test]
fn specialization_shadow_config_has_no_authoritative_mode_surface() {
    let error = toml::from_str::<NodeConfig>(
        r#"
[execution.specialization_shadow]
enabled = true
mode = "authoritative"
candidates = ["flamingo_factory_pair_key_v1"]
"#,
    )
    .expect_err("authoritative routing must not be representable in node TOML");

    assert!(
        error.to_string().contains("unknown field `mode`"),
        "{error}"
    );
}

#[test]
fn specialization_shadow_applies_optional_artifact_limit_overrides() {
    let config: NodeConfig = toml::from_str(
        r#"
[execution.specialization_shadow]
enabled = true
strict_replay = true
allow_artifact_overflow = true
candidates = ["flamingo_factory_pair_key_v1"]
max_artifact_bytes = 16777216
max_stack_nodes = 262144
max_storage_reads = 131072
"#,
    )
    .expect("parse shadow config with artifact limit overrides");

    let runtime = config
        .execution
        .specialization_shadow
        .build_runtime(true)
        .expect("validate shadow config")
        .expect("enabled shadow runtime");

    assert_eq!(runtime.artifact_limits.max_retained_bytes, 16_777_216);
    assert_eq!(runtime.artifact_limits.max_stack_nodes, 262_144);
    assert_eq!(runtime.artifact_limits.max_storage_reads, 131_072);
    // Untouched bounds keep their defaults.
    assert_eq!(
        runtime.artifact_limits.max_stack_edges,
        neo_execution::ExecutionArtifactLimits::DEFAULT.max_stack_edges
    );

    let zeroed: NodeConfig = toml::from_str(
        r#"
[execution.specialization_shadow]
enabled = true
candidates = ["flamingo_factory_pair_key_v1"]
max_stack_edges = 0
"#,
    )
    .expect("parse zeroed override");
    let error = validate_config(&zeroed, 0x334F_454E).expect_err("zero override must fail");
    assert!(error.to_string().contains("max_stack_edges"));
}

#[test]
fn specialization_shadow_rejects_missing_duplicate_and_oversized_bounds() {
    let missing: NodeConfig = toml::from_str(
        r#"
[execution.specialization_shadow]
enabled = true
"#,
    )
    .expect("parse missing candidate config");
    let error = validate_config(&missing, 0x334F_454E).expect_err("candidate is required");
    assert!(error.to_string().contains("at least one exact candidate"));

    let duplicate: NodeConfig = toml::from_str(
        r#"
[execution.specialization_shadow]
enabled = true
candidates = ["flamingo_factory_pair_key_v1", "flamingo_factory_pair_key_v1"]
"#,
    )
    .expect("parse duplicate candidate config");
    let error = validate_config(&duplicate, 0x334F_454E).expect_err("duplicate must fail");
    assert!(
        error
            .to_string()
            .contains("duplicate or unsupported candidate")
    );

    let oversized: NodeConfig = toml::from_str(
        r#"
[execution.specialization_shadow]
max_reproducer_bytes = 1048577
"#,
    )
    .expect("parse oversized bound");
    let error = validate_config(&oversized, 0x334F_454E).expect_err("hard cap must fail");
    assert!(error.to_string().contains("must not exceed the hard limit"));
}
