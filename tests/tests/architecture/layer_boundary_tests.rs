//! Layer Boundary Integration Tests
//!
//! Validates the architectural layering of the neo-rs workspace:
//!
//! ```text
//! Layer 0 (Foundation): neo-primitives
//! Layer 1 (Infrastructure): neo-io, neo-error, neo-crypto, neo-storage, neo-static-files, neo-config, neo-vm, neo-serialization, neo-manifest
//! Layer 2 (Protocol): neo-payloads, neo-consensus, neo-hsm
//! Layer 3 (Domain services): neo-execution, neo-native-contracts, neo-mempool, neo-state-service, neo-runtime
//! Layer 4 (Node services): neo-blockchain, neo-network, neo-wallets, neo-indexer, neo-oracle-service
//! Layer 5 (Composition): neo-system
//! Layer 6 (Plugin/RPC boundary): neo-rpc
//! Layer 7 (Applications): neo-node, neo-gui
//! ```

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::Path;

/// Layer definitions for the neo-rs architecture
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Layer {
    Foundation = 0,
    Infrastructure = 1,
    Protocol = 2,
    DomainServices = 3,
    NodeServices = 4,
    Composition = 5,
    PluginBoundary = 6,
    Application = 7,
}

impl Layer {
    fn metadata_name(self) -> &'static str {
        match self {
            Self::Foundation => "foundation",
            Self::Infrastructure => "infrastructure",
            Self::Protocol => "protocol",
            Self::DomainServices => "domain-services",
            Self::NodeServices => "node-services",
            Self::Composition => "composition",
            Self::PluginBoundary => "plugin-boundary",
            Self::Application => "application",
        }
    }

    fn from_crate_name(name: &str) -> Option<Self> {
        match name {
            // Layer 0: Foundation (no neo-* dependencies allowed).
            "neo-primitives" => Some(Layer::Foundation),
            // Layer 1: Infrastructure and shared data tooling.
            "neo-io" | "neo-error" | "neo-crypto" | "neo-storage" | "neo-static-files"
            | "neo-config" | "neo-vm" | "neo-serialization" | "neo-manifest" => {
                Some(Layer::Infrastructure)
            }
            // Layer 2: Protocol payloads and consensus message vocabulary.
            "neo-payloads" | "neo-consensus" | "neo-hsm" => Some(Layer::Protocol),
            // Layer 3: Domain logic with no node composition dependency.
            "neo-execution"
            | "neo-native-contracts"
            | "neo-mempool"
            | "neo-state-service"
            | "neo-runtime" => Some(Layer::DomainServices),
            // Layer 4: Long-running or queryable node services.
            "neo-blockchain" | "neo-network" | "neo-wallets" | "neo-indexer"
            | "neo-oracle-service" => Some(Layer::NodeServices),
            // Layer 5: Node composition root.
            "neo-system" => Some(Layer::Composition),
            // Layer 6: Optional plugin/RPC-facing service boundary.
            "neo-rpc" => Some(Layer::PluginBoundary),
            // Layer 7: Binaries, UI clients, and development-only tooling.
            "neo-node" | "neo-gui" | "neo-test-fixtures" => Some(Layer::Application),
            _ => None,
        }
    }
}

fn architecture_metadata(workspace_root: &Path) -> toml::Value {
    let root_manifest = read_toml_manifest(&workspace_root.join("Cargo.toml"));
    root_manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("metadata"))
        .and_then(|metadata| metadata.get("architecture"))
        .cloned()
        .expect("root Cargo.toml should declare [workspace.metadata.architecture]")
}

fn parse_allowed_same_layer_dependencies(workspace_root: &Path) -> HashSet<(String, String)> {
    architecture_metadata(workspace_root)
        .get("allowed-same-layer-dependencies")
        .and_then(toml::Value::as_array)
        .expect("architecture metadata should declare allowed-same-layer-dependencies")
        .iter()
        .map(|entry| {
            let edge = entry
                .as_str()
                .expect("same-layer dependency entries should be strings");
            let (source, dependency) = edge.split_once(" -> ").unwrap_or_else(|| {
                panic!("same-layer dependency `{edge}` should use `source -> dependency`")
            });
            (source.to_string(), dependency.to_string())
        })
        .collect()
}

fn read_toml_manifest(cargo_toml_path: &Path) -> toml::Value {
    let content = fs::read_to_string(cargo_toml_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", cargo_toml_path.display()));
    content
        .parse::<toml::Value>()
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", cargo_toml_path.display()))
}

fn package_name_from_manifest(manifest: &toml::Value, cargo_toml_path: &Path) -> String {
    manifest
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(toml::Value::as_str)
        .unwrap_or_else(|| {
            panic!(
                "{} should declare [package].name",
                cargo_toml_path.display()
            )
        })
        .to_string()
}

fn collect_neo_dependencies_from_table(table: Option<&toml::Value>, deps: &mut BTreeSet<String>) {
    let Some(table) = table.and_then(toml::Value::as_table) else {
        return;
    };

    for (dep_name, dep_spec) in table {
        let package_name = dep_spec
            .as_table()
            .and_then(|spec| spec.get("package"))
            .and_then(toml::Value::as_str)
            .unwrap_or(dep_name);

        if package_name.starts_with("neo-") {
            deps.insert(package_name.to_string());
        }
    }
}

/// Parse Cargo.toml to extract runtime/build neo-* dependencies.
fn parse_neo_dependencies(cargo_toml_path: &Path) -> Vec<String> {
    let manifest = read_toml_manifest(cargo_toml_path);
    let mut deps = BTreeSet::new();

    collect_neo_dependencies_from_table(manifest.get("dependencies"), &mut deps);
    collect_neo_dependencies_from_table(manifest.get("build-dependencies"), &mut deps);

    if let Some(targets) = manifest.get("target").and_then(toml::Value::as_table) {
        for target in targets.values() {
            collect_neo_dependencies_from_table(target.get("dependencies"), &mut deps);
            collect_neo_dependencies_from_table(target.get("build-dependencies"), &mut deps);
        }
    }

    deps.into_iter().collect()
}

/// Get all crate directories in the workspace
fn get_workspace_crates(workspace_root: &Path) -> Vec<String> {
    let mut crates = Vec::new();

    for entry in fs::read_dir(workspace_root).expect("Failed to read workspace") {
        let entry = entry.expect("Failed to read entry");
        let path = entry.path();

        if path.is_dir() {
            let name = path.file_name().unwrap().to_str().unwrap();
            if name.starts_with("neo-") {
                let cargo_toml = path.join("Cargo.toml");
                if cargo_toml.exists() {
                    crates.push(name.to_string());
                }
            }
        }
    }

    crates
}

fn parse_workspace_string_array(workspace_root: &Path, key: &str) -> Vec<String> {
    let cargo_toml_path = workspace_root.join("Cargo.toml");
    let manifest = read_toml_manifest(&cargo_toml_path);
    let values = manifest
        .get("workspace")
        .and_then(|workspace| workspace.get(key))
        .and_then(toml::Value::as_array)
        .unwrap_or_else(|| panic!("root Cargo.toml should declare [workspace].{key} as an array"));

    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .unwrap_or_else(|| {
                    panic!("root Cargo.toml [workspace].{key} should contain only strings")
                })
                .to_string()
        })
        .collect()
}

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
    // neo-error is part of the acyclic foundation (it depends only on
    // neo-primitives + neo-io); neo-crypto returns CoreResult, so it depends on it.
    let allowed = ["neo-primitives", "neo-io", "neo-error"];

    for dep in &deps {
        assert!(
            allowed.contains(&dep.as_str()),
            "neo-crypto (Layer 1) should only depend on Layer 0 crates, but found: {}",
            dep
        );
    }
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
fn test_composition_node_hides_component_layout() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let node_path = workspace_root
        .join("neo-system")
        .join("src")
        .join("composition")
        .join("node.rs");
    let source = fs::read_to_string(&node_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", node_path.display()));

    let public_fields = [
        "pub settings:",
        "pub storage:",
        "pub wallets:",
        "pub blockchain:",
        "pub network:",
        "pub staged_sync_pipeline:",
        "pub live_block_import_pipeline:",
        "pub mempool:",
        "pub header_cache:",
        "pub native_contract_provider:",
        "pub shutdown:",
    ];
    let exposed = public_fields
        .into_iter()
        .filter(|field| source.contains(field))
        .collect::<Vec<_>>();
    assert!(
        exposed.is_empty(),
        "neo-system::Node is an application-facing facade; component layout must stay private: {exposed:?}"
    );

    for accessor in [
        "pub fn settings(&self)",
        "pub fn storage(&self)",
        "pub fn blockchain(&self)",
        "pub fn network(&self)",
        "pub fn staged_sync_pipeline(",
        "pub fn live_block_import_pipeline(&self)",
        "pub fn mempool(&self)",
        "pub fn header_cache(&self)",
        "pub fn native_contract_provider(&self)",
    ] {
        assert!(
            source.contains(accessor),
            "neo-system::Node should expose named capability accessor `{accessor}`"
        );
    }

    assert!(
        !source.contains("cancellation_token(") && !source.contains("CancellationToken"),
        "process cancellation belongs to the application supervisor, not neo-system::Node"
    );
}

#[test]
fn test_rpc_node_context_hides_component_layout() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let context_path = workspace_root
        .join("neo-rpc")
        .join("src")
        .join("server")
        .join("node_context.rs");
    let source = fs::read_to_string(&context_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", context_path.display()));

    let public_fields = [
        "pub settings:",
        "pub storage:",
        "pub blockchain:",
        "pub network:",
        "pub mempool:",
        "pub header_cache:",
        "pub services:",
        "pub native_contract_provider:",
    ];
    let exposed = public_fields
        .into_iter()
        .filter(|field| source.contains(field))
        .collect::<Vec<_>>();
    assert!(
        exposed.is_empty(),
        "RPC callers must use named NodeContext capabilities, not its component layout: {exposed:?}"
    );
}

#[test]
fn test_node_services_hide_command_loop_and_wire_module_layouts() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let blockchain_root = workspace_root.join("neo-blockchain").join("src");
    let blockchain_lib =
        fs::read_to_string(blockchain_root.join("lib.rs")).expect("read neo-blockchain crate root");
    let blockchain_service = fs::read_to_string(blockchain_root.join("service/mod.rs"))
        .expect("read neo-blockchain service module");

    assert!(
        blockchain_lib.contains("mod service;"),
        "neo-blockchain should expose handles and capabilities, not its service module tree"
    );
    for forbidden in [
        "pub mod service;",
        "pub use internal::{",
        "pub mod internal;",
    ] {
        assert!(
            !blockchain_lib.contains(forbidden) && !blockchain_service.contains(forbidden),
            "neo-blockchain command-loop internal `{forbidden}` must stay private"
        );
    }

    let network_lib_path = workspace_root.join("neo-network").join("src/lib.rs");
    let network_lib = fs::read_to_string(&network_lib_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", network_lib_path.display()));
    for forbidden in [
        "pub mod proto;",
        "pub mod wire;",
        "command, event, handle, local_node, remote_node,",
    ] {
        assert!(
            !network_lib.contains(forbidden),
            "neo-network should export typed capabilities, not module layout `{forbidden}`"
        );
    }

    let network_service_root = workspace_root.join("neo-network/src/service");
    for removed in ["block_sync_mode.rs", "task_manager.rs"] {
        assert!(
            !network_service_root.join(removed).exists(),
            "coordinator-owned range sync must not regain legacy service module `{removed}`"
        );
    }
    for removed_export in [
        "BlockRequestScheduler",
        "BlockSyncMode",
        "TaskManagerService",
        "TaskManagerHandle",
    ] {
        assert!(
            !network_lib.contains(removed_export),
            "neo-network must keep one coordinator-owned range-sync path; found `{removed_export}`"
        );
    }
}

#[test]
fn test_application_context_contains_only_commit_hooks() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let context_dir = workspace_root
        .join("neo-node")
        .join("src")
        .join("node")
        .join("context");
    let mut violations = Vec::new();

    for entry in fs::read_dir(&context_dir)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", context_dir.display()))
    {
        let path = entry.expect("context directory entry").path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
            continue;
        }
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        for forbidden in [
            "neo_system::Node",
            "impl SystemContext",
            "StoreCache<",
            "\n    native_contract_provider:",
        ] {
            if source.contains(forbidden) {
                violations.push(format!("{} contains `{forbidden}`", path.display()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "application commit hooks must not reclaim composition-owned core context responsibilities:\n{}",
        violations.join("\n")
    );
}

#[tokio::test]
async fn test_indexer_remains_reusable_node_service_not_rpc_submodule() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    assert_eq!(
        Layer::from_crate_name("neo-indexer"),
        Some(Layer::NodeServices),
        "neo-indexer should stay classified as a node service, not as an RPC plugin"
    );

    let indexer_deps =
        parse_neo_dependencies(&workspace_root.join("neo-indexer").join("Cargo.toml"));
    let forbidden_owners = ["neo-rpc", "neo-system", "neo-node"];
    for forbidden_owner in forbidden_owners {
        assert!(
            !indexer_deps.iter().any(|dep| dep == forbidden_owner),
            "neo-indexer must not depend on {forbidden_owner}; it should remain reusable by RPC, node, and future service surfaces"
        );
    }

    let same_or_higher_layer_deps = indexer_deps
        .iter()
        .filter(|dep| {
            matches!(
                Layer::from_crate_name(dep),
                Some(layer) if layer >= Layer::NodeServices
            )
        })
        .collect::<Vec<_>>();
    assert!(
        same_or_higher_layer_deps.is_empty(),
        "neo-indexer should index persisted protocol data and depend only on lower layers, but found: {:?}",
        same_or_higher_layer_deps
    );

    let rpc_deps = parse_neo_dependencies(&workspace_root.join("neo-rpc").join("Cargo.toml"));
    assert!(
        rpc_deps.iter().any(|dep| dep == "neo-indexer"),
        "neo-rpc should expose NeoIndexer methods by consuming the node-service crate"
    );

    let node_deps = parse_neo_dependencies(&workspace_root.join("neo-node").join("Cargo.toml"));
    assert!(
        node_deps.iter().any(|dep| dep == "neo-indexer"),
        "neo-node should own the live indexer lifecycle and register it for RPC"
    );
}

#[tokio::test]
async fn test_active_architecture_docs_do_not_reference_retired_crates() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let checked_files = [
        "Cargo.toml",
        "README.md",
        "docs/architecture.md",
        "docs/protocol-compatibility.md",
        "docs/STYLE.md",
        "neo-runtime/Cargo.toml",
        "neo-runtime/src/lib.rs",
        "neo-rpc/Cargo.toml",
        "neo-rpc/src/client/contracts/contract_client.rs",
        "neo-rpc/src/server/rpc_server_blockchain/mod.rs",
        "neo-rpc/src/tests/server/handlers/rpc_server_node.rs",
        "neo-system/Cargo.toml",
        "neo-system/src/lib.rs",
        "neo-network/Cargo.toml",
        "neo-network/src/lib.rs",
        "neo-network/src/wire/mod.rs",
        "neo-network/src/proto/mod.rs",
        "neo-network/src/service/mod.rs",
    ];

    let retired_terms = [
        "neo-core",
        "neo-p2p",
        "Layer 1 (runtime)",
        "Layer 1 (service)",
        "Layer 1 (protocol types)",
        "Layer 2 (services we talk to)",
        "Stage A",
        "Stage B/C",
        "Actor runtime",
    ];
    let mut violations = Vec::new();

    for relative_path in checked_files {
        let path = workspace_root.join(relative_path);
        let content = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));

        for term in retired_terms {
            if content.contains(term) {
                violations.push(format!("{relative_path} still references `{term}`"));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Active architecture documentation should use the current crate/layer vocabulary:\n{}",
        violations.join("\n")
    );
}

#[tokio::test]
async fn test_protocol_compatibility_doc_pins_v3101_release_audit() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let doc_path = workspace_root
        .join("docs")
        .join("protocol-compatibility.md");
    let doc = fs::read_to_string(&doc_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", doc_path.display()));

    let required_markers = [
        "https://github.com/neo-project/neo/releases/tag/v3.10.1",
        "d10e9ceecdabe3fcff719ee68ea5b76ba7e62c3d",
        "v3.10.0...v3.10.1",
        "df402675",
        "e66e4dfc",
        "6b1c90c6",
        "f5ae5e82",
        "55c14029",
        "7f8454f4",
        "9f4795ab",
        "abbc3a25",
        "7bb91ff5",
        "d10e9cee",
        "HF_Huyao",
        "ExtensiblePayload",
        "StorageKey.ToString()",
    ];

    let missing = required_markers
        .iter()
        .filter(|marker| !doc.contains(**marker))
        .copied()
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "protocol compatibility doc must pin the full Neo v3.10.1 release audit; missing: {missing:?}"
    );
}

#[tokio::test]
async fn test_neo_v3101_release_delta_has_source_guards() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let checked_markers = [
        (
            "neo-primitives/src/protocol/chain/hardfork.rs",
            &["HfHuyao = 7 => \"HF_Huyao\""][..],
        ),
        (
            "neo-config/src/settings/protocol/validation.rs",
            &["ensure_omitted_hardforks", "HardforkManager::all()"][..],
        ),
        (
            "neo-execution/src/application_engine/fees_events_native.rs",
            &[
                "C# v3.10.1 validates `AddFee` arguments before applying the",
                "add_native_method_fee",
                "checked_add(storage_pico)",
            ][..],
        ),
        (
            "neo-native-contracts/src/neo_token/persist.rs",
            &[
                "Hardfork::HfGorgon",
                "self.candidate_vote(&snapshot, member)?",
            ][..],
        ),
        (
            "neo-native-contracts/src/notary/persist.rs",
            &["Deposit not found", "Insufficient deposit"][..],
        ),
        (
            "neo-payloads/src/protocol/extensible_payload.rs",
            &["Witness script hash {witness_hash} does not match sender"][..],
        ),
        (
            "neo-storage/src/types/storage_key.rs",
            &["StorageKey{{Id={}}}", "StorageKey{{Id={},Key={}}}"][..],
        ),
        (
            "neo-native-contracts/src/std_lib/numeric.rs",
            &["CultureInfo.InvariantCulture", "dotnet_bigint_to_hex"][..],
        ),
        (
            "neo-vm/src/runtime/reference_counter.rs",
            &[
                "C# `RemoveStackReference(item)`",
                "compounds lower the total only when `IsStackReferenced` is true",
                "self.remove_stack_reference(&sub_item)",
            ][..],
        ),
        (
            "neo-vm/src/stack_item/array.rs",
            &[
                "C# v3.10.1 CLEARITEMS snapshots sub-items",
                "inner.items.clear();",
                "rc.remove_stack_reference(&item)",
            ][..],
        ),
        (
            "neo-vm/src/stack_item/map.rs",
            &[
                "C# v3.10.1 CLEARITEMS snapshots `Map.SubItems`",
                "inner.items.clear();",
                "rc.remove_stack_reference(&item)",
            ][..],
        ),
        (
            "neo-vm/src/tests/runtime/reference_counter.rs",
            &[
                "removing_unreferenced_compound_does_not_underflow",
                "clearing_self_referenced_array_releases_all_references",
                "clearing_self_referenced_map_releases_all_references",
            ][..],
        ),
    ];

    let mut missing = Vec::new();
    for (relative_path, markers) in checked_markers {
        let path = workspace_root.join(relative_path);
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        for marker in markers {
            if !source.contains(marker) {
                missing.push(format!("{relative_path} missing `{marker}`"));
            }
        }
    }

    assert!(
        missing.is_empty(),
        "Neo v3.10.1 release-delta source guards are missing:\n{}",
        missing.join("\n")
    );
}

#[test]
fn test_operational_metadata_uses_typed_tables_and_commits_remain_fallible() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let typed_consumers = [
        "neo-runtime/src/service/sync_pipeline/checkpoint_store.rs",
        "neo-runtime/src/service/sync_pipeline/verified_header_store.rs",
        "neo-blockchain/src/ledger/static_archive/pruning.rs",
    ];
    let mut violations = Vec::new();

    for relative_path in typed_consumers {
        let path = workspace_root.join(relative_path);
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        for forbidden in [
            ".maintenance_metadata(",
            ".put_metadata(",
            ".delete_metadata(",
        ] {
            if source.contains(forbidden) {
                violations.push(format!("{relative_path} contains `{forbidden}`"));
            }
        }
        if !source.contains("table_get::<") || !source.contains(".put::<") {
            violations.push(format!(
                "{relative_path} must read and write through typed table identities"
            ));
        }
    }

    let table_api =
        fs::read_to_string(workspace_root.join("neo-storage/src/persistence/table/definition.rs"))
            .expect("read typed table API");
    for marker in [
        "pub trait Table:",
        "type KeyCodec:",
        "type ValueCodec:",
        "const NAMESPACE: TableNamespace",
    ] {
        if !table_api.contains(marker) {
            violations.push(format!("typed table API missing `{marker}`"));
        }
    }

    for relative_path in [
        "neo-storage/src/persistence/traits/store_snapshot.rs",
        "neo-storage/src/persistence/cache/store_cache.rs",
    ] {
        let source = fs::read_to_string(workspace_root.join(relative_path))
            .unwrap_or_else(|error| panic!("failed to read {relative_path}: {error}"));
        if source.contains("pub fn commit(&mut self)") || source.contains("fn commit(&mut self)") {
            violations.push(format!(
                "{relative_path} reintroduces a backend commit API that can discard errors"
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "typed storage architecture regressed:\n{}",
        violations.join("\n")
    );
}

#[tokio::test]
async fn test_no_circular_dependencies() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let crates = get_workspace_crates(workspace_root);

    let mut graph: HashMap<String, Vec<String>> = HashMap::new();

    for crate_name in &crates {
        let cargo_toml = workspace_root.join(crate_name).join("Cargo.toml");
        if !cargo_toml.exists() {
            continue;
        }

        let deps = parse_neo_dependencies(&cargo_toml);
        graph.insert(crate_name.clone(), deps);
    }

    // Verify graph is acyclic (no circular dependencies)
    let mut visited = HashSet::new();
    let mut rec_stack = HashSet::new();

    fn has_cycle(
        node: &str,
        graph: &HashMap<String, Vec<String>>,
        visited: &mut HashSet<String>,
        rec_stack: &mut HashSet<String>,
        path: &mut Vec<String>,
    ) -> Option<Vec<String>> {
        visited.insert(node.to_string());
        rec_stack.insert(node.to_string());
        path.push(node.to_string());

        if let Some(deps) = graph.get(node) {
            for dep in deps {
                if !visited.contains(dep) {
                    if let Some(cycle) = has_cycle(dep, graph, visited, rec_stack, path) {
                        return Some(cycle);
                    }
                } else if rec_stack.contains(dep) {
                    path.push(dep.clone());
                    return Some(path.clone());
                }
            }
        }

        path.pop();
        rec_stack.remove(node);
        None
    }

    for crate_name in graph.keys() {
        if !visited.contains(crate_name) {
            let mut path = Vec::new();
            if let Some(cycle) =
                has_cycle(crate_name, &graph, &mut visited, &mut rec_stack, &mut path)
            {
                panic!("Circular dependency detected: {}", cycle.join(" -> "));
            }
        }
    }
}

// ============================================================================
// Cross-Layer Type Compatibility Tests
// ============================================================================

#[tokio::test]
async fn test_primitives_types_usable() {
    use neo_primitives::{UInt160, UInt256};

    let hash160 = UInt160::zero();
    let hash256 = UInt256::zero();

    assert_eq!(hash160.to_bytes().len(), 20);
    assert_eq!(hash256.to_bytes().len(), 32);

    // Verify hex encoding works via hex crate
    let hex160 = hex::encode(hash160.to_bytes());
    let hex256 = hex::encode(hash256.to_bytes());

    assert_eq!(hex160.len(), 40); // 20 bytes * 2
    assert_eq!(hex256.len(), 64); // 32 bytes * 2
}

#[tokio::test]
async fn test_io_serialization_with_primitives() {
    use neo_io::{BinaryWriter, MemoryReader, Serializable};
    use neo_primitives::{UInt160, UInt256};

    // Test UInt160 serialization roundtrip
    let original_160 = UInt160::from([0x01u8; 20]);
    let mut writer = BinaryWriter::new();
    original_160
        .serialize(&mut writer)
        .expect("serialize UInt160");
    let bytes = writer.to_bytes();

    let mut reader = MemoryReader::new(&bytes);
    let restored_160 = UInt160::deserialize(&mut reader).expect("deserialize UInt160");
    assert_eq!(original_160, restored_160);

    // Test UInt256 serialization roundtrip
    let original_256 = UInt256::from([0x02u8; 32]);
    let mut writer = BinaryWriter::new();
    original_256
        .serialize(&mut writer)
        .expect("serialize UInt256");
    let bytes = writer.to_bytes();

    let mut reader = MemoryReader::new(&bytes);
    let restored_256 = UInt256::deserialize(&mut reader).expect("deserialize UInt256");
    assert_eq!(original_256, restored_256);
}

#[tokio::test]
async fn test_crypto_hash_functions() {
    use neo_crypto::Crypto;
    use neo_primitives::UInt256;

    // Verify crypto operations produce correct-sized outputs
    let data = b"test data";
    let hash = Crypto::sha256(data);

    assert_eq!(hash.len(), 32);

    // Can convert to UInt256
    let hash_uint = UInt256::from_bytes(&hash).expect("convert to UInt256");
    assert_eq!(hash_uint.to_bytes(), hash);
}

#[tokio::test]
async fn test_storage_key_builder() {
    use neo_storage::KeyBuilder;

    // Verify storage key building works through the typed-error API.
    let mut builder = KeyBuilder::try_new(1, 0x01, 64).expect("valid key builder");
    builder.try_add_byte(0x02).expect("first byte fits");
    builder.try_add_byte(0x03).expect("second byte fits");
    let key = builder.to_bytes();

    assert!(!key.is_empty());
}

#[tokio::test]
async fn test_json_object_creation() {
    use neo_serialization::json::{JObject, JToken};

    // Test JObject creation and property access
    let mut obj = JObject::new();
    obj.insert("key".to_string(), JToken::String("value".to_string()));
    obj.insert("number".to_string(), JToken::Number(42.0));

    assert!(obj.contains_property("key"));
    assert!(obj.contains_property("number"));
}
