//! Workspace membership, metadata, and dependency-layer invariants.

use super::*;

#[tokio::test]
async fn test_layer_0_has_no_neo_dependencies() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();

    // The only crate with NO neo-* dependencies is neo-primitives. The other
    // foundation crates (neo-io -> primitives; neo-error -> primitives + io)
    // form a small acyclic base; neo-serialization is a serialization *layer*
    // (it depends on the VM stack-item types), not a zero-dependency foundation.
    let strict_layer_0 = ["neo-primitives"];

    for crate_name in &strict_layer_0 {
        let cargo_toml = workspace_root.join(crate_name).join("Cargo.toml");
        if !cargo_toml.exists() {
            continue;
        }

        let deps = parse_neo_dependencies(&cargo_toml);

        assert!(
            deps.is_empty(),
            "Strict Layer 0 crate '{}' should have NO neo-* dependencies, but found: {:?}",
            crate_name,
            deps
        );
    }
}

#[tokio::test]
async fn test_storage_only_depends_on_primitives() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let cargo_toml = workspace_root.join("neo-storage").join("Cargo.toml");

    if !cargo_toml.exists() {
        return;
    }

    let deps = parse_neo_dependencies(&cargo_toml);

    // neo-storage may depend on neo-primitives (UInt160/UInt256 key types) and
    // neo-error (it lifts StorageError / KeyBuilderError into neo_error::CoreError
    // in neo-storage/src/error.rs). Both are acyclic foundation crates — neo-error
    // depends only on neo-primitives + neo-io — so this keeps neo-storage at the
    // foundation layer without introducing a cycle.
    for dep in &deps {
        assert!(
            dep == "neo-primitives" || dep == "neo-error" || dep == "neo-io",
            "neo-storage should only depend on the foundation crates (neo-primitives, neo-error, neo-io), but found: {}",
            dep
        );
    }
}

#[tokio::test]
async fn test_neo_io_only_depends_on_primitives() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let cargo_toml = workspace_root.join("neo-io").join("Cargo.toml");

    if !cargo_toml.exists() {
        return;
    }

    let deps = parse_neo_dependencies(&cargo_toml);

    // neo-io can depend on neo-primitives (for Serializable impls)
    for dep in &deps {
        assert_eq!(
            dep, "neo-primitives",
            "neo-io should only depend on neo-primitives, but found: {}",
            dep
        );
    }
}

#[tokio::test]
async fn test_crypto_only_depends_on_layer_0() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let cargo_toml = workspace_root.join("neo-crypto").join("Cargo.toml");

    if !cargo_toml.exists() {
        return;
    }

    let deps = parse_neo_dependencies(&cargo_toml);
    // neo-error is part of the acyclic foundation; neo-crypto maps its typed
    // errors into CoreError but owns no codec or trie mechanics.
    let allowed = ["neo-primitives", "neo-error"];

    for dep in &deps {
        assert!(
            allowed.contains(&dep.as_str()),
            "neo-crypto (Layer 1) should only depend on Layer 0 crates, but found: {}",
            dep
        );
    }
}

#[test]
fn test_trie_protocol_ownership_is_exclusive() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let crypto_root = workspace_root.join("neo-crypto");
    let trie_root = workspace_root.join("neo-trie");

    assert!(
        !crypto_root.join("src/mpt_trie").exists(),
        "Neo MPT mechanics belong exclusively to neo-trie"
    );
    let crypto_library =
        fs::read_to_string(crypto_root.join("src/lib.rs")).expect("read neo-crypto crate root");
    for forbidden in [
        "pub mod mpt_trie",
        "MptCache",
        "MptStoreSnapshot",
        "pub use mpt_trie",
    ] {
        assert!(
            !crypto_library.contains(forbidden),
            "neo-crypto must not restore MPT facade `{forbidden}`"
        );
    }

    for required in [
        "src/mpt/cache/mod.rs",
        "src/mpt/error.rs",
        "src/mpt/node.rs",
        "src/mpt/node_type.rs",
        "src/mpt/trie/mod.rs",
        "src/mpt/trie/proof.rs",
    ] {
        assert!(
            trie_root.join(required).is_file(),
            "neo-trie is missing canonical implementation file `{required}`"
        );
    }

    let trie_dependencies = parse_neo_dependencies(&trie_root.join("Cargo.toml"));
    assert!(
        trie_dependencies
            .iter()
            .any(|dependency| dependency == "neo-crypto"),
        "neo-trie must delegate Hash256 to neo-crypto"
    );
    for forbidden in ["neo-storage", "neo-state-service", "neo-rpc", "neo-node"] {
        assert!(
            !trie_dependencies
                .iter()
                .any(|dependency| dependency == forbidden),
            "backend-independent neo-trie must not depend on `{forbidden}`"
        );
    }

    let canonical_consumers = [
        "neo-state-service/src/storage/mpt_store.rs",
        "neo-state-service/src/providers/proof.rs",
        "neo-node/src/node/state_packs/authority.rs",
        "neo-rpc/src/tests/server/rpc_server_state/proof.rs",
    ];
    for relative_path in canonical_consumers {
        let source = fs::read_to_string(workspace_root.join(relative_path))
            .unwrap_or_else(|error| panic!("failed to read {relative_path}: {error}"));
        assert!(
            source.contains("neo_trie"),
            "MPT consumer `{relative_path}` must import the exclusive neo-trie owner"
        );
        assert!(
            !source.contains("neo_crypto::mpt_trie"),
            "MPT consumer `{relative_path}` must not restore the removed crypto facade"
        );
    }
}

#[test]
fn test_rpc_exception_is_owned_exclusively_by_rpc() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let primitives_root = workspace_root.join("neo-primitives");
    let rpc_exception = workspace_root.join("neo-rpc/src/server/rpc_exception/mod.rs");

    assert!(
        !primitives_root.join("src/errors/rpc_exception.rs").exists(),
        "neo-primitives must not contain the RPC handler exception implementation"
    );

    let mut pending = vec![primitives_root.join("src")];
    let mut violations = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", directory.display()))
        {
            let path = entry
                .expect("primitive source entry should be readable")
                .path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
                continue;
            }
            let source = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            for forbidden in ["RpcException", "rpc_exception"] {
                if source.contains(forbidden) {
                    violations.push(format!("{} contains `{forbidden}`", path.display()));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "foundation primitives must not expose RPC transport types:\n{}",
        violations.join("\n")
    );

    let source = fs::read_to_string(&rpc_exception)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", rpc_exception.display()));
    for required in [
        "pub struct RpcException",
        "impl Display for RpcException",
        "impl std::error::Error for RpcException",
        "impl From<RpcError> for RpcException",
        "impl From<RpcException> for RpcError",
    ] {
        assert!(
            source.contains(required),
            "canonical RPC exception owner is missing `{required}`"
        );
    }
    assert!(
        !source.contains("pub use neo_primitives::RpcException"),
        "neo-rpc must own RpcException instead of restoring the primitives facade"
    );
}

#[tokio::test]
async fn test_all_neo_crates_are_classified() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let crates = get_workspace_crates(workspace_root);
    let unclassified = crates
        .into_iter()
        .filter(|crate_name| Layer::from_crate_name(crate_name).is_none())
        .collect::<Vec<_>>();

    assert!(
        unclassified.is_empty(),
        "Every neo-* crate with a Cargo.toml should have an explicit architecture layer: {:?}",
        unclassified
    );
}

#[test]
fn test_workspace_architecture_metadata_matches_layer_model() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let architecture = architecture_metadata(workspace_root);
    let layers = architecture
        .get("layers")
        .and_then(toml::Value::as_table)
        .expect("architecture metadata should declare a layers table");
    let crates = get_workspace_crates(workspace_root);

    let mut metadata_layers = HashMap::new();
    for layer in [
        Layer::Foundation,
        Layer::Infrastructure,
        Layer::Protocol,
        Layer::DomainServices,
        Layer::NodeServices,
        Layer::Composition,
        Layer::PluginBoundary,
        Layer::Application,
    ] {
        let members = layers
            .get(layer.metadata_name())
            .and_then(toml::Value::as_array)
            .unwrap_or_else(|| {
                panic!(
                    "architecture metadata should list `{}` layer members",
                    layer.metadata_name()
                )
            });
        for member in members {
            let member = member
                .as_str()
                .expect("architecture layer members should be strings");
            assert!(
                metadata_layers.insert(member.to_string(), layer).is_none(),
                "architecture metadata lists `{member}` in more than one layer"
            );
        }
    }

    let expected = crates
        .into_iter()
        .map(|crate_name| {
            let layer = Layer::from_crate_name(&crate_name)
                .unwrap_or_else(|| panic!("unclassified crate `{crate_name}`"));
            (crate_name, layer)
        })
        .collect::<HashMap<_, _>>();
    assert_eq!(
        metadata_layers, expected,
        "Cargo architecture metadata and the enforced layer model must stay identical"
    );
}

#[tokio::test]
async fn test_development_workspace_crates_are_not_publishable() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let dev_crates = [
        ("tests/Cargo.toml", "neo-tests"),
        ("benches-package/Cargo.toml", "neo-benches"),
    ];

    for (relative_path, package_name) in dev_crates {
        let path = workspace_root.join(relative_path);
        let manifest = read_toml_manifest(&path);
        let package = manifest
            .get("package")
            .and_then(toml::Value::as_table)
            .unwrap_or_else(|| panic!("{relative_path} should declare [package]"));

        assert_eq!(
            package.get("name").and_then(toml::Value::as_str),
            Some(package_name),
            "{relative_path} should declare the expected package name"
        );
        assert_eq!(
            package.get("publish").and_then(toml::Value::as_bool),
            Some(false),
            "{relative_path} is a development-only workspace member and must stay unpublished"
        );
    }
}

#[tokio::test]
async fn test_workspace_members_use_central_lint_policy() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let workspace_members = parse_workspace_string_array(workspace_root, "members");
    let mut violations = Vec::new();

    for member in workspace_members {
        let manifest_path = workspace_root.join(&member).join("Cargo.toml");
        let manifest = read_toml_manifest(&manifest_path);
        let uses_workspace_lints = manifest
            .get("lints")
            .and_then(|lints| lints.get("workspace"))
            .and_then(toml::Value::as_bool)
            .unwrap_or(false);

        if !uses_workspace_lints {
            violations.push(member);
        }
    }

    assert!(
        violations.is_empty(),
        "Every workspace member should opt into [workspace.lints] with `[lints] workspace = true`: {:?}",
        violations
    );
}

#[tokio::test]
async fn test_workspace_dependencies_match_runtime_workspace_members() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let root_manifest = read_toml_manifest(&workspace_root.join("Cargo.toml"));
    let workspace_members = parse_workspace_string_array(workspace_root, "members");
    let member_paths = workspace_members
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let workspace_version = root_manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("package"))
        .and_then(|package| package.get("version"))
        .and_then(toml::Value::as_str)
        .expect("root Cargo.toml should declare [workspace.package].version");
    let workspace_dependencies = root_manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("dependencies"))
        .and_then(toml::Value::as_table)
        .expect("root Cargo.toml should declare [workspace.dependencies]");
    let development_member_paths = ["tests", "benches-package"];

    for member in &workspace_members {
        if development_member_paths.contains(&member.as_str()) {
            continue;
        }

        let manifest_path = workspace_root.join(member).join("Cargo.toml");
        let member_manifest = read_toml_manifest(&manifest_path);
        let package_name = package_name_from_manifest(&member_manifest, &manifest_path);

        if !package_name.starts_with("neo-") {
            continue;
        }

        let dependency = workspace_dependencies
            .get(&package_name)
            .and_then(toml::Value::as_table)
            .unwrap_or_else(|| {
                panic!("{package_name} should be declared in [workspace.dependencies]")
            });

        assert_eq!(
            dependency.get("path").and_then(toml::Value::as_str),
            Some(member.as_str()),
            "{package_name} should use its workspace member path in [workspace.dependencies]"
        );
        assert_eq!(
            dependency.get("version").and_then(toml::Value::as_str),
            Some(workspace_version),
            "{package_name} should use the workspace package version in [workspace.dependencies]"
        );
    }

    let mut stale_internal_dependencies = Vec::new();

    for (dependency_name, dependency_spec) in workspace_dependencies {
        if !dependency_name.starts_with("neo-") {
            continue;
        }

        let Some(path) = dependency_spec
            .as_table()
            .and_then(|dependency| dependency.get("path"))
            .and_then(toml::Value::as_str)
        else {
            continue;
        };

        if path.starts_with("../") {
            continue;
        }

        if !member_paths.contains(path) {
            stale_internal_dependencies.push(format!("{dependency_name} -> {path}"));
            continue;
        }

        let manifest_path = workspace_root.join(path).join("Cargo.toml");
        let member_manifest = read_toml_manifest(&manifest_path);
        let package_name = package_name_from_manifest(&member_manifest, &manifest_path);

        assert_eq!(
            dependency_name, &package_name,
            "[workspace.dependencies].{dependency_name} should match package name at {path}"
        );
    }

    assert!(
        stale_internal_dependencies.is_empty(),
        "Internal neo-* workspace dependencies should point at active workspace members: {:?}",
        stale_internal_dependencies
    );
}

#[tokio::test]
async fn test_development_workspace_crates_are_not_default_members() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let workspace_members = parse_workspace_string_array(workspace_root, "members");
    let default_members = parse_workspace_string_array(workspace_root, "default-members");
    let development_members = ["tests", "benches-package"];

    for default_member in &default_members {
        assert!(
            workspace_members.contains(default_member),
            "default workspace member `{default_member}` must also be listed in [workspace].members"
        );
    }

    for development_member in development_members {
        assert!(
            workspace_members
                .iter()
                .any(|member| member == development_member),
            "development-only crate `{development_member}` should remain an explicit workspace member"
        );
        assert!(
            !default_members
                .iter()
                .any(|member| member == development_member),
            "development-only crate `{development_member}` must stay out of default-members"
        );
    }

    // Only neo-node needs to be in default-members; neo-rpc and neo-indexer
    // were intentionally removed to reduce default build tax (Task #137).
    // They remain workspace members and can be built explicitly with
    // `cargo build -p neo-rpc` or `cargo build -p neo-indexer`.
    let runtime_entrypoint = "neo-node";
    assert!(
        default_members
            .iter()
            .any(|member| member == runtime_entrypoint),
        "runtime-facing crate `{runtime_entrypoint}` should stay in default-members"
    );
}

#[tokio::test]
async fn test_standalone_directories_remain_excluded_from_workspace() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let workspace_members = parse_workspace_string_array(workspace_root, "members");
    let excluded_entries = parse_workspace_string_array(workspace_root, "exclude");
    let required_excludes = ["neo-gui", "fuzz", "examples", "docs", "neo_csharp"];

    for excluded_entry in &excluded_entries {
        assert!(
            !workspace_members.contains(excluded_entry),
            "`{excluded_entry}` cannot be both a workspace member and an excluded standalone path"
        );
    }

    for required_exclude in required_excludes {
        assert!(
            excluded_entries
                .iter()
                .any(|entry| entry == required_exclude),
            "`{required_exclude}` should stay excluded from the core Rust workspace"
        );
    }
}

#[tokio::test]
async fn test_no_upward_dependencies() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let crates = get_workspace_crates(workspace_root);

    let mut violations = Vec::new();

    for crate_name in &crates {
        let Some(crate_layer) = Layer::from_crate_name(crate_name) else {
            continue;
        };

        let cargo_toml = workspace_root.join(crate_name).join("Cargo.toml");
        if !cargo_toml.exists() {
            continue;
        }

        let deps = parse_neo_dependencies(&cargo_toml);

        for dep in deps {
            if let Some(dep_layer) = Layer::from_crate_name(&dep) {
                if dep_layer > crate_layer {
                    violations.push(format!(
                        "{} (Layer {:?}) depends on {} (Layer {:?})",
                        crate_name, crate_layer, dep, dep_layer
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Found upward dependency violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn test_same_layer_dependencies_are_explicit() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let crates = get_workspace_crates(workspace_root);
    let allowed = parse_allowed_same_layer_dependencies(workspace_root);
    let mut actual = HashSet::new();
    let mut violations = Vec::new();

    for crate_name in &crates {
        let Some(crate_layer) = Layer::from_crate_name(crate_name) else {
            continue;
        };
        let cargo_toml = workspace_root.join(crate_name).join("Cargo.toml");
        for dependency in parse_neo_dependencies(&cargo_toml) {
            if Layer::from_crate_name(&dependency) != Some(crate_layer) {
                continue;
            }
            let edge = (crate_name.clone(), dependency.clone());
            actual.insert(edge.clone());
            if !allowed.contains(&edge) {
                violations.push(format!("{} -> {}", edge.0, edge.1));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Same-layer dependencies require an explicit architecture exception:\n{}",
        violations.join("\n")
    );

    let stale = allowed.difference(&actual).collect::<Vec<_>>();
    assert!(
        stale.is_empty(),
        "Remove stale same-layer dependency exceptions: {stale:?}"
    );
}

#[tokio::test]
async fn test_runtime_vocabulary_stays_below_composition_root() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();

    let runtime_deps =
        parse_neo_dependencies(&workspace_root.join("neo-runtime").join("Cargo.toml"));
    assert!(
        runtime_deps
            .iter()
            .all(|dep| matches!(Layer::from_crate_name(dep), Some(layer) if layer < Layer::DomainServices)),
        "neo-runtime must stay a shared lower-layer vocabulary crate, but it depends on: {:?}",
        runtime_deps
    );

    let system_deps = parse_neo_dependencies(&workspace_root.join("neo-system").join("Cargo.toml"));
    assert!(
        system_deps.iter().any(|dep| dep == "neo-runtime"),
        "neo-system should consume neo-runtime service traits instead of owning that vocabulary"
    );

    for service_crate in ["neo-network", "neo-blockchain", "neo-mempool"] {
        let deps = parse_neo_dependencies(&workspace_root.join(service_crate).join("Cargo.toml"));
        assert!(
            !deps.iter().any(|dep| dep == "neo-system"),
            "{service_crate} must not depend on neo-system; shared service vocabulary belongs in neo-runtime"
        );
    }
}

#[test]
fn test_runtime_does_not_reclaim_chain_config_or_node_aggregate_traits() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let runtime_root = workspace_root.join("neo-runtime");

    let runtime_deps = parse_neo_dependencies(&runtime_root.join("Cargo.toml"));
    assert!(
        !runtime_deps
            .iter()
            .any(|dependency| dependency == "neo-config"),
        "neo-runtime owns narrow service capabilities; immutable chain configuration belongs to neo-config"
    );

    assert!(
        !runtime_root.join("src/node/types.rs").exists(),
        "neo-runtime must not restore the removed node aggregate-trait module"
    );

    let checked_files = [
        "neo-runtime/src/lib.rs",
        "neo-runtime/src/node/mod.rs",
        "neo-runtime/src/node/providers.rs",
    ];
    let forbidden = [
        "ConfigProvider",
        "NodeTypes",
        "NeoNodeTypes",
        "pub trait ChainSpecProvider",
    ];
    let mut violations = Vec::new();
    for relative_path in checked_files {
        let source = fs::read_to_string(workspace_root.join(relative_path))
            .unwrap_or_else(|error| panic!("failed to read {relative_path}: {error}"));
        for marker in forbidden {
            if source.contains(marker) {
                violations.push(format!("{relative_path} contains `{marker}`"));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "neo-runtime must expose narrow capabilities instead of a duplicate node/config type family:\n{}",
        violations.join("\n")
    );
}
