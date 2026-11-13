//! Four-layer architecture guard using Cargo's complete dependency declarations.
//! L0: foundations; L1: protocol/execution; L2: services; L3: assembly.
//! Numeric layer order never grants an edge. Every production edge is reviewed.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layer {
    Foundation,
    ProtocolExecution,
    Services,
    Assembly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MemberKind {
    Production(Layer),
    Tests,
    Tool,
    Bench,
    Fuzz,
}

impl MemberKind {
    fn is_production(self) -> bool {
        matches!(self, Self::Production(_))
    }
}

fn classify(name: &str, relative_manifest: &Path, package: &Value) -> Result<MemberKind, String> {
    use Layer::*;
    let production = match name {
        "neo-primitives" | "neo-json" | "neo-io" | "neo-crypto" | "neo-config" | "neo-storage" => {
            Some(Foundation)
        }
        "neo-vm"
        | "neo-protocol"
        | "neo-smart-contract"
        | "neo-native-contracts"
        | "neo-ledger"
        // Generic actor runtime (task 189): protocol-agnostic, zero neo-* deps,
        // so it declares no ALLOWED_EDGES entry of its own.
        | "neo-actors" => Some(ProtocolExecution),
        "neo-p2p" | "neo-consensus" | "neo-rpc" | "neo-telemetry" | "neo-tee" | "neo-hsm"
        | "neo-ai-agent" | "neo-probe" | "neo-mempool" | "neo-oracle"
        | "neo-state-service" | "neo-wallets" | "neo-system" => Some(Services),
        "neo-node" | "neo-signer-server" | "neo-validator" => Some(Assembly),
        _ => None,
    };
    if let Some(layer) = production {
        return Ok(MemberKind::Production(layer));
    }
    // Nonproduction exceptions are exact identities AND workspace-relative paths.
    // No directory-prefix, publish=false, or unknown-name fallback is sufficient.
    let kind = match (name, relative_manifest.to_str()) {
        ("neo-tests", Some("tests/Cargo.toml")) => MemberKind::Tests,
        ("neo-benches", Some("benches-package/Cargo.toml")) => MemberKind::Bench,
        ("validation-diff", Some("tools/validation-diff/Cargo.toml"))
        | ("state-root-diff", Some("tools/state-root-diff/Cargo.toml")) => MemberKind::Tool,
        ("neo-rs-fuzz", Some("fuzz/Cargo.toml")) if package["metadata"]["cargo-fuzz"] == true => {
            MemberKind::Fuzz
        }
        _ => {
            return Err(format!(
                "Unclassified workspace member {name} at {}",
                relative_manifest.display()
            ));
        }
    };
    // Existing tools/bench have no publish restriction in their manifests;
    // their exemption is the exact reviewed name/path, not a publish heuristic.
    if matches!(kind, MemberKind::Tests | MemberKind::Fuzz)
        && package["publish"] != serde_json::json!([])
    {
        return Err(format!("Nonproduction member {name} must be publish=false"));
    }
    Ok(kind)
}

// Reviewed production edges, including explicit same-layer edges. Planned names
// above reserve ownership only; no new crates/manifests are created by this test.
const ALLOWED_EDGES: &[(&str, &str)] = &[
    ("neo-io", "neo-primitives"),
    ("neo-storage", "neo-primitives"),
    ("neo-storage", "neo-io"),
    ("neo-crypto", "neo-io"),
    ("neo-crypto", "neo-primitives"),
    ("neo-vm", "neo-crypto"),
    ("neo-vm", "neo-io"),
    ("neo-vm", "neo-primitives"),
    ("neo-protocol", "neo-config"),
    ("neo-protocol", "neo-crypto"),
    ("neo-protocol", "neo-primitives"),
    ("neo-p2p", "neo-crypto"),
    ("neo-p2p", "neo-io"),
    ("neo-p2p", "neo-primitives"),
    ("neo-p2p", "neo-protocol"),
    ("neo-consensus", "neo-crypto"),
    ("neo-consensus", "neo-io"),
    ("neo-consensus", "neo-primitives"),
    ("neo-consensus", "neo-vm"),
    ("neo-rpc", "neo-config"),
    ("neo-rpc", "neo-crypto"),
    ("neo-rpc", "neo-io"),
    ("neo-rpc", "neo-json"),
    ("neo-rpc", "neo-ledger"),
    ("neo-rpc", "neo-primitives"),
    ("neo-rpc", "neo-protocol"),
    ("neo-rpc", "neo-storage"),
    ("neo-rpc", "neo-tee"),
    ("neo-rpc", "neo-vm"),
    // Task 206: the RPC server actor relay binds directly to the generic actor
    // runtime (downward edge, same pattern as neo-vm).
    ("neo-rpc", "neo-actors"),
    // Same-layer service edges exercised by RPC request handlers.
    ("neo-rpc", "neo-wallets"),
    ("neo-rpc", "neo-system"),
    ("neo-tee", "neo-crypto"),
    ("neo-tee", "neo-primitives"),
    ("neo-hsm", "neo-crypto"),
    ("neo-probe", "neo-ai-agent"),
    ("neo-probe", "neo-storage"),
    ("neo-probe", "neo-vm"),
    // Probe inspects execution artifacts; reviewed downward edges.
    ("neo-probe", "neo-native-contracts"),
    ("neo-probe", "neo-primitives"),
    ("neo-probe", "neo-protocol"),
    ("neo-probe", "neo-smart-contract"),
    ("neo-node", "neo-ai-agent"),
    ("neo-node", "neo-consensus"),
    ("neo-node", "neo-crypto"),
    ("neo-node", "neo-hsm"),
    ("neo-node", "neo-p2p"),
    ("neo-node", "neo-probe"),
    ("neo-node", "neo-rpc"),
    ("neo-node", "neo-tee"),
    ("neo-node", "neo-telemetry"),
    ("neo-node", "neo-vm"),
    // Task 206: consensus binds directly to the generic actor runtime
    // (Assembly -> ProtocolExecution downward edge, same pattern as neo-vm).
    ("neo-node", "neo-actors"),
    // Assembly composes lower layers directly; reviewed downward edges.
    ("neo-node", "neo-io"),
    ("neo-node", "neo-primitives"),
    ("neo-node", "neo-protocol"),
    ("neo-node", "neo-wallets"),
    ("neo-node", "neo-storage"),
    ("neo-node", "neo-system"),
    // Wave B/C consumer repoints: direct deps on authoritative crates.
    ("neo-node", "neo-native-contracts"),
    ("neo-node", "neo-state-service"),
    ("neo-node", "neo-ledger"),
    ("neo-node", "neo-mempool"),
    ("neo-node", "neo-smart-contract"),
    ("neo-node", "neo-config"),
    ("neo-node", "neo-json"),
    // Validator is an assembly binary: it composes the canonical execution and
    // storage owners directly and must not introduce a parallel state model.
    ("neo-validator", "neo-crypto"),
    ("neo-validator", "neo-io"),
    ("neo-validator", "neo-ledger"),
    ("neo-validator", "neo-native-contracts"),
    ("neo-validator", "neo-primitives"),
    ("neo-validator", "neo-protocol"),
    ("neo-validator", "neo-storage"),
    ("neo-validator", "neo-system"),
    ("neo-validator", "neo-vm"),
    ("neo-signer-server", "neo-crypto"),
    ("neo-signer-server", "neo-tee"),
    ("neo-signer-server", "neo-wallets"),
    ("neo-rpc", "neo-native-contracts"),
    ("neo-rpc", "neo-state-service"),
    ("neo-rpc", "neo-smart-contract"),
    ("neo-rpc", "neo-mempool"),
    ("neo-rpc", "neo-oracle"),
    // Native implementations may use the engine, never the reverse direction.
    ("neo-native-contracts", "neo-smart-contract"),
    // Decomposed protocol/execution crates (Phase 2), verified one by one
    // against locked `cargo metadata` declarations; downward edges only.
    ("neo-protocol", "neo-io"),
    ("neo-protocol", "neo-vm"),
    ("neo-ledger", "neo-actors"),
    ("neo-ledger", "neo-crypto"),
    ("neo-ledger", "neo-io"),
    ("neo-ledger", "neo-native-contracts"),
    ("neo-ledger", "neo-primitives"),
    ("neo-ledger", "neo-protocol"),
    ("neo-ledger", "neo-smart-contract"),
    ("neo-ledger", "neo-storage"),
    ("neo-ledger", "neo-vm"),
    ("neo-smart-contract", "neo-crypto"),
    ("neo-smart-contract", "neo-io"),
    ("neo-smart-contract", "neo-primitives"),
    ("neo-smart-contract", "neo-protocol"),
    ("neo-smart-contract", "neo-storage"),
    ("neo-smart-contract", "neo-vm"),
    ("neo-native-contracts", "neo-config"),
    ("neo-native-contracts", "neo-crypto"),
    ("neo-native-contracts", "neo-io"),
    ("neo-native-contracts", "neo-primitives"),
    ("neo-native-contracts", "neo-protocol"),
    ("neo-native-contracts", "neo-storage"),
    ("neo-native-contracts", "neo-vm"),
    // Test-only edge (kind dev): wallet fixtures are exercised in tests.
    ("neo-native-contracts", "neo-wallets"),
    // Decomposed service crates, same metadata-verified review.
    ("neo-mempool", "neo-primitives"),
    // Test-only edge (kind dev): mempool tests build VM scripts directly.
    ("neo-mempool", "neo-vm"),
    // Mempool validates and stores transactions; reviewed downward edges.
    ("neo-mempool", "neo-ledger"),
    ("neo-mempool", "neo-native-contracts"),
    ("neo-mempool", "neo-protocol"),
    ("neo-mempool", "neo-storage"),
    // Test-only edge (kind dev): mempool tests use wallet fixtures.
    ("neo-mempool", "neo-wallets"),
    ("neo-oracle", "neo-crypto"),
    ("neo-oracle", "neo-io"),
    ("neo-oracle", "neo-json"),
    ("neo-oracle", "neo-primitives"),
    ("neo-oracle", "neo-protocol"),
    // Oracle builds request scripts and signs callbacks; reviewed edges.
    ("neo-oracle", "neo-smart-contract"),
    ("neo-oracle", "neo-vm"),
    ("neo-oracle", "neo-wallets"),
    ("neo-state-service", "neo-crypto"),
    ("neo-state-service", "neo-ledger"),
    ("neo-state-service", "neo-primitives"),
    ("neo-state-service", "neo-protocol"),
    ("neo-state-service", "neo-storage"),
    // State service executes contracts against cached state; downward edges.
    ("neo-state-service", "neo-io"),
    ("neo-state-service", "neo-native-contracts"),
    ("neo-state-service", "neo-smart-contract"),
    ("neo-state-service", "neo-vm"),
    // Test-only edge (kind dev): state-service tests use wallet fixtures.
    ("neo-state-service", "neo-wallets"),
    ("neo-wallets", "neo-crypto"),
    ("neo-wallets", "neo-primitives"),
    ("neo-wallets", "neo-protocol"),
    // Wallet helpers construct and sign transactions/contracts; downward edges.
    ("neo-wallets", "neo-io"),
    ("neo-wallets", "neo-native-contracts"),
    ("neo-wallets", "neo-smart-contract"),
    ("neo-wallets", "neo-storage"),
    ("neo-wallets", "neo-vm"),
    ("neo-system", "neo-crypto"),
    ("neo-system", "neo-ledger"),
    ("neo-system", "neo-native-contracts"),
    ("neo-system", "neo-storage"),
    // Task W3-C2: the moved converters (and NeoSystem unit tests) build
    // execution payload types directly.
    ("neo-system", "neo-protocol"),
    // Task W3-C2: the P2P actors moved in and bind to the shared caches
    // (HashSetCache/RelayCache) exposed by the neutral codec crate.
    ("neo-system", "neo-io"),
    // Task W3-C2: `oracle_service` moved in and composes the neo-oracle engine
    // through production adapters (snapshot/wallet/signer/tx-builder/relay).
    ("neo-system", "neo-oracle"),
    ("neo-system", "neo-p2p"),
    ("neo-system", "neo-primitives"),
    // Task 208: state verification service moved from neo-core into
    // neo-system; it binds directly to the unified signing service (now in
    // neo-wallets) and builds witness invocation scripts with VM opcodes.
    ("neo-system", "neo-wallets"),
    ("neo-system", "neo-vm"),
    // Wave D1-L1 (task 215): the moved node actors bind directly to the generic
    // actor runtime instead of the neo-core `runtime` compatibility re-export
    // (same types, TypeId-preserving).
    ("neo-system", "neo-actors"),
    // Task 230 (D2 phase 1): authoritative CoreError/CoreResult live in
    // neo-smart-contract; neo-system binds them directly instead of the
    // neo-core error re-export shim.
    ("neo-system", "neo-smart-contract"),
];

// Policy edges that exist ONLY as Cargo dev-dependencies (test-only). Cargo
// permits a dev-dependency cycle, so the acyclic guard ignores dev edges. The
// static-policy fixture must mark these kind=dev as well; otherwise it flattens
// them into normal edges and fabricates a cycle that cannot occur in the real
// metadata graph (e.g. neo-native-contracts <-> neo-wallets).
const DEV_ONLY_EDGES: &[(&str, &str)] = &[
    ("neo-native-contracts", "neo-wallets"),
    ("neo-mempool", "neo-vm"),
    ("neo-mempool", "neo-wallets"),
    ("neo-state-service", "neo-wallets"),
];

#[derive(Debug)]
struct Member {
    name: String,
    kind: MemberKind,
}

#[derive(Debug)]
struct Edge {
    from: String,
    to: String,
    alias: String,
    kind: String,
    optional: bool,
    target: Option<String>,
}

#[derive(Debug)]
struct WorkspaceGraph {
    members: BTreeMap<String, Member>,
    edges: Vec<Edge>,
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("Metadata field {key} must be a string"))
}

fn array<'a>(value: &'a Value, key: &str) -> Result<&'a Vec<Value>, String> {
    value[key]
        .as_array()
        .ok_or_else(|| format!("Metadata field {key} must be an array"))
}

fn nullable_text(value: &Value, key: &str) -> Result<Option<String>, String> {
    match value.get(key) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        _ => Err(format!(
            "Metadata field {key} must be present and null or a string"
        )),
    }
}

fn normalized_path(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err(format!(
            "Metadata path must be absolute: {}",
            path.display()
        ));
    }
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !result.pop() {
                    return Err("Metadata path escapes its root".into());
                }
            }
            _ => result.push(component.as_os_str()),
        }
    }
    Ok(result)
}

impl WorkspaceGraph {
    fn from_metadata(metadata: &Value) -> Result<Self, String> {
        if metadata["version"] != 1 {
            return Err("Expected Cargo metadata format version 1".into());
        }
        let root = normalized_path(Path::new(text(metadata, "workspace_root")?))?;
        let mut packages = BTreeMap::new();
        for package in array(metadata, "packages")? {
            let id = text(package, "id")?;
            if packages.insert(id, package).is_some() {
                return Err(format!("Duplicate package identity {id}"));
            }
        }
        let mut members = BTreeMap::new();
        let mut paths = BTreeMap::new();
        let mut names = BTreeMap::new();
        for id in array(metadata, "workspace_members")? {
            let id = id
                .as_str()
                .ok_or("Workspace member identity must be a string")?;
            let package = packages
                .get(id)
                .ok_or_else(|| format!("Missing workspace package identity {id}"))?;
            let name = text(package, "name")?;
            let manifest = normalized_path(Path::new(text(package, "manifest_path")?))?;
            if manifest.file_name().and_then(|n| n.to_str()) != Some("Cargo.toml") {
                return Err(format!("Invalid manifest for {id}: {}", manifest.display()));
            }
            let relative = manifest
                .strip_prefix(&root)
                .map_err(|_| format!("Member {id} is outside workspace root"))?;
            // Normalize separators for exact nonproduction paths on Windows.
            let relative = PathBuf::from(relative.to_string_lossy().replace('\\', "/"));
            let kind = classify(name, &relative, package)?;
            if paths
                .insert(manifest.parent().unwrap().to_path_buf(), id.to_owned())
                .is_some()
                || names.insert(name.to_owned(), id.to_owned()).is_some()
                || members
                    .insert(
                        id.to_owned(),
                        Member {
                            name: name.to_owned(),
                            kind,
                        },
                    )
                    .is_some()
            {
                return Err(format!("Duplicate workspace name/path/identity: {id}"));
            }
        }
        if members.is_empty() {
            return Err("Workspace has no classified members".into());
        }
        let mut edges = Vec::new();
        for id in members.keys() {
            for dep in array(packages[id.as_str()], "dependencies")? {
                let name = text(dep, "name")?;
                let alias = nullable_text(dep, "rename")?.unwrap_or_else(|| name.to_owned());
                let kind = nullable_text(dep, "kind")?.unwrap_or_else(|| "normal".into());
                if !["normal", "build", "dev"].contains(&kind.as_str()) {
                    return Err(format!("Unsupported dependency kind {kind}"));
                }
                let optional = dep["optional"]
                    .as_bool()
                    .ok_or("Dependency optional must be a bool")?;
                let target = nullable_text(dep, "target")?;
                let source = nullable_text(dep, "source")?;
                // Cargo omits `path` altogether for registry/git declarations.
                let path = if dep.get("path").is_some() {
                    nullable_text(dep, "path")?
                } else {
                    None
                };
                let by_name = names.get(name);
                let by_path = path
                    .as_deref()
                    .map(|p| normalized_path(Path::new(p)))
                    .transpose()?
                    .and_then(|p| paths.get(&p));
                let to = match (by_name, by_path) {
                    (Some(named), Some(located)) if named == located && source.is_none() => located,
                    (None, None)
                        if path.is_none() && source.is_some() && !name.starts_with("neo-") =>
                    {
                        continue;
                    }
                    _ => {
                        return Err(format!(
                            "Internal dependency identity/path mismatch: {} -> {name} (alias {alias}, path {path:?})",
                            members[id].name
                        ));
                    }
                };
                // Declarations, not resolve.nodes, preserve every optional/target
                // edge even when default features or the host do not enable it.
                edges.push(Edge {
                    from: id.clone(),
                    to: to.clone(),
                    alias,
                    kind,
                    optional,
                    target,
                });
            }
        }
        Ok(Self { members, edges })
    }

    fn dependencies(&self, name: &str) -> Vec<&str> {
        let id = self
            .members
            .iter()
            .find(|(_, member)| member.name == name)
            .map(|(id, _)| id)
            .unwrap_or_else(|| panic!("Required workspace member missing: {name}"));
        self.edges
            .iter()
            .filter(|edge| &edge.from == id)
            .map(|edge| self.members[&edge.to].name.as_str())
            .collect()
    }

    fn validate_edges(&self, allowed: &[(&str, &str)]) -> Result<(), String> {
        for edge in &self.edges {
            let from = &self.members[&edge.from];
            let to = &self.members[&edge.to];
            let pair = (from.name.as_str(), to.name.as_str());
            if from.kind.is_production() && (!to.kind.is_production() || !allowed.contains(&pair)) {
                return Err(format!(
                    "Forbidden dependency {} -> {} (alias {}, kind {}, optional {}, target {:?})",
                    from.name, to.name, edge.alias, edge.kind, edge.optional, edge.target
                ));
            }
        }
        self.validate_acyclic()
    }

    fn validate_acyclic(&self) -> Result<(), String> {
        fn visit<'a>(
            graph: &'a WorkspaceGraph,
            id: &'a str,
            done: &mut BTreeSet<&'a str>,
            active: &mut Vec<&'a str>,
        ) -> Result<(), String> {
            if let Some(start) = active.iter().position(|node| *node == id) {
                let mut cycle: Vec<_> = active[start..]
                    .iter()
                    .map(|node| graph.members[*node].name.as_str())
                    .collect();
                cycle.push(graph.members[id].name.as_str());
                return Err(format!("Circular dependency: {}", cycle.join(" -> ")));
            }
            if done.contains(id) {
                return Ok(());
            }
            active.push(id);
            // Cargo permits dev-dependency cycles (a crate's tests may depend on
            // a crate that depends back on it). Only normal and build edges form
            // a real compile-time cycle, so dev edges are excluded here.
            for edge in graph
                .edges
                .iter()
                .filter(|edge| edge.from == id && edge.kind != "dev")
            {
                visit(graph, &edge.to, done, active)?;
            }
            active.pop();
            done.insert(id);
            Ok(())
        }
        let mut done = BTreeSet::new();
        for id in self.members.keys() {
            visit(self, id, &mut done, &mut Vec::new())?;
        }
        Ok(())
    }
}

fn workspace_graph() -> &'static WorkspaceGraph {
    static GRAPH: OnceLock<WorkspaceGraph> = OnceLock::new();
    GRAPH.get_or_init(|| {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("Cargo.toml");
        // Use the Cargo executable that compiled this target; never a network or
        // unlocked fallback. --no-deps needs no resolved third-party packages.
        let output = Command::new(env!("CARGO"))
            .args([
                "metadata",
                "--format-version",
                "1",
                "--no-deps",
                "--locked",
                "--offline",
                "--manifest-path",
            ])
            .arg(manifest)
            .output()
            .expect("Failed to execute locked/offline Cargo metadata");
        assert!(
            output.status.success(),
            "Cargo metadata failed (no network/unlocked retry): {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let metadata: Value =
            serde_json::from_slice(&output.stdout).expect("Invalid Cargo metadata JSON");
        WorkspaceGraph::from_metadata(&metadata)
            .unwrap_or_else(|error| panic!("Architecture metadata rejected: {error}"))
    })
}

#[test]
fn test_layer_0_has_no_neo_dependencies() {
    for name in ["neo-primitives", "neo-json"] {
        assert!(
            workspace_graph().dependencies(name).is_empty(),
            "Strict foundation {name} has internal dependencies"
        );
    }
}

#[test]
fn test_storage_only_depends_on_primitives_and_neutral_io() {
    let dependencies: BTreeSet<_> = workspace_graph()
        .dependencies("neo-storage")
        .into_iter()
        .collect();
    assert_eq!(
        dependencies,
        BTreeSet::from(["neo-primitives", "neo-io"]),
        "neo-storage must keep exactly the reviewed primitive and neutral codec edges"
    );
}

#[test]
fn test_storage_to_io_is_an_exact_one_way_neutral_edge() {
    let graph = fixture_pair(
        "neo-storage",
        "neo-io",
        fixture_dependency("neo-io", "neo-io"),
    );
    graph.validate_edges(ALLOWED_EDGES).unwrap();
    for (from, to) in [
        ("neo-io", "neo-storage"),
        ("neo-storage", "neo-crypto"),
        ("neo-storage", "neo-vm"),
    ] {
        let graph = fixture_pair(from, to, fixture_dependency(to, to));
        assert!(graph.validate_edges(ALLOWED_EDGES).is_err());
    }
}

fn manifest_assignment(relative: &str, section: &str, key: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let source = std::fs::read_to_string(root.join(relative)).unwrap();
    let mut active = false;
    let mut values = Vec::new();
    for line in source.lines() {
        let line = line.split('#').next().unwrap().trim();
        if line.starts_with('[') {
            active = line == section;
        } else if active
            && let Some((name, value)) = line.split_once('=')
            && name.trim() == key
        {
            values.push(value.trim().to_owned());
        }
    }
    assert_eq!(values.len(), 1, "{relative}: expected one {section} {key}");
    values.pop().unwrap()
}

#[test]
fn test_rocksdb_features_are_opt_in_at_the_storage_owner() {
    assert_eq!(
        manifest_assignment("neo-storage/Cargo.toml", "[features]", "default"),
        "[]"
    );
    assert_eq!(
        manifest_assignment("neo-storage/Cargo.toml", "[features]", "rocksdb"),
        "[\"dep:rocksdb\"]"
    );
    assert_eq!(
        manifest_assignment("tests/Cargo.toml", "[features]", "default"),
        "[]"
    );
    assert_eq!(
        manifest_assignment("neo-storage/Cargo.toml", "[dependencies]", "rocksdb"),
        "{ workspace = true, optional = true }"
    );
    assert_eq!(
        manifest_assignment("neo-node/Cargo.toml", "[features]", "full"),
        "[\"neo-storage/rocksdb\"]"
    );
    assert_eq!(
        manifest_assignment("neo-validator/Cargo.toml", "[features]", "rocksdb"),
        "[\"neo-storage/rocksdb\"]"
    );
    assert_eq!(
        manifest_assignment("neo-storage/Cargo.toml", "[dependencies]", "neo-io"),
        "{ workspace = true }"
    );
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let relative = "neo-storage/src/persistence/providers/mod.rs";
    let source = std::fs::read_to_string(root.join(relative)).unwrap();
    let compact: String = source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .flat_map(|line| line.chars().filter(|c| !c.is_whitespace()))
        .collect();
    assert!(
        compact.contains("#[cfg(feature=\"rocksdb\")]pubmodrocksdb;"),
        "{relative}: provider module must be opt-in"
    );
    assert!(
        compact.contains("#[cfg(feature=\"rocksdb\")]pubuserocksdb::RocksDBStoreProvider;"),
        "{relative}: provider export must be opt-in"
    );
}

#[test]
fn test_neo_io_only_depends_on_primitives() {
    for dep in workspace_graph().dependencies("neo-io") {
        assert_eq!(
            dep, "neo-primitives",
            "neo-io has an unauthorized dependency"
        );
    }
}

#[test]
fn test_crypto_only_depends_on_layer_0() {
    for dep in workspace_graph().dependencies("neo-crypto") {
        assert!(
            ["neo-primitives", "neo-io"].contains(&dep),
            "neo-crypto has an unauthorized dependency: {dep}"
        );
    }
}

#[test]
fn test_no_upward_dependencies() {
    // Preserves the original guard's intent, strengthened to explicit edges,
    // exact member classification, all declarations, and no missing-crate skips.
    workspace_graph().validate_edges(ALLOWED_EDGES).unwrap();
}

#[test]
fn test_no_circular_dependencies() {
    workspace_graph().validate_acyclic().unwrap();
}

// Fixtures contain Cargo metadata declarations only: no temporary manifests,
// project creation, downloads, feature activation, or compilation subprocesses.
fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("metadata-fixture-not-created")
}

fn fixture_package(name: &str, relative: &str) -> Value {
    serde_json::json!({
        "id": format!("opaque-package-id:{name}"), "name": name,
        "manifest_path": fixture_root().join(relative).join("Cargo.toml"),
        "publish": [], "metadata": {}, "dependencies": []
    })
}

fn fixture_dependency(name: &str, relative: &str) -> Value {
    serde_json::json!({
        "name": name, "rename": null, "kind": null, "optional": false,
        "source": null, "target": null, "path": fixture_root().join(relative)
    })
}

fn fixture_metadata(packages: Vec<Value>) -> Value {
    let members: Vec<_> = packages
        .iter()
        .map(|package| package["id"].clone())
        .collect();
    serde_json::json!({
        "version": 1, "workspace_root": fixture_root(), "workspace_members": members,
        "packages": packages, "resolve": null
    })
}

fn fixture_pair(from: &str, to: &str, dependency: Value) -> WorkspaceGraph {
    let mut source = fixture_package(from, from);
    source["dependencies"] = serde_json::json!([dependency]);
    WorkspaceGraph::from_metadata(&fixture_metadata(vec![source, fixture_package(to, to)])).unwrap()
}

#[test]
fn test_metadata_alias_uses_package_identity_not_dependency_key() {
    let mut dep = fixture_dependency("neo-io", "nested/renamed-io-directory");
    dep["rename"] = serde_json::json!("neo_io_crate");
    let mut vm = fixture_package("neo-vm", "execution/runtime");
    vm["dependencies"] = serde_json::json!([dep]);
    let graph = WorkspaceGraph::from_metadata(&fixture_metadata(vec![
        vm,
        fixture_package("neo-io", "nested/renamed-io-directory"),
    ]))
    .unwrap();
    assert_eq!(graph.dependencies("neo-vm"), ["neo-io"]);
    assert_eq!(graph.edges[0].alias, "neo_io_crate");
    graph.validate_edges(ALLOWED_EDGES).unwrap();
    let mut illegal = fixture_dependency("neo-node", "neo-node");
    illegal["rename"] = serde_json::json!("innocent_alias");
    let graph = fixture_pair("neo-vm", "neo-node", illegal);
    assert!(
        graph
            .validate_edges(ALLOWED_EDGES)
            .unwrap_err()
            .contains("innocent_alias")
    );
}

#[test]
fn test_metadata_normal_dependency_is_checked() {
    let graph = fixture_pair(
        "neo-storage",
        "neo-vm",
        fixture_dependency("neo-vm", "neo-vm"),
    );
    assert_eq!(graph.edges.len(), 1);
    assert!(
        graph
            .validate_edges(ALLOWED_EDGES)
            .unwrap_err()
            .contains("kind normal")
    );
}

#[test]
fn test_metadata_build_dependency_is_checked() {
    let mut dep = fixture_dependency("neo-node", "neo-node");
    dep["kind"] = serde_json::json!("build");
    let graph = fixture_pair("neo-vm", "neo-node", dep);
    assert!(
        graph
            .validate_edges(ALLOWED_EDGES)
            .unwrap_err()
            .contains("kind build")
    );
}

#[test]
fn test_metadata_target_dependency_is_checked_without_host_filtering() {
    let mut dep = fixture_dependency("neo-node", "neo-node");
    dep["target"] = serde_json::json!("cfg(target_os = \"fixture-non-host\")");
    let graph = fixture_pair("neo-storage", "neo-node", dep);
    assert!(
        graph
            .validate_edges(ALLOWED_EDGES)
            .unwrap_err()
            .contains("fixture-non-host")
    );
}

#[test]
fn test_metadata_disabled_optional_dependency_is_checked_without_resolve() {
    let mut dep = fixture_dependency("neo-node", "neo-node");
    dep["optional"] = serde_json::json!(true);
    let graph = fixture_pair("neo-vm", "neo-node", dep);
    assert!(
        graph
            .validate_edges(ALLOWED_EDGES)
            .unwrap_err()
            .contains("optional true")
    );
}

#[test]
fn test_metadata_dev_dependency_cannot_hide_a_production_reverse_edge() {
    let mut dep = fixture_dependency("neo-vm", "neo-vm");
    dep["kind"] = serde_json::json!("dev");
    let graph = fixture_pair("neo-storage", "neo-vm", dep);
    assert!(
        graph
            .validate_edges(ALLOWED_EDGES)
            .unwrap_err()
            .contains("kind dev")
    );
}

#[test]
fn test_metadata_combined_alias_build_optional_target_declaration_is_retained() {
    let mut dep = fixture_dependency("neo-node", "neo-node");
    dep["rename"] = serde_json::json!("hidden_node");
    dep["kind"] = serde_json::json!("build");
    dep["optional"] = serde_json::json!(true);
    dep["target"] = serde_json::json!("cfg(any())");
    let graph = fixture_pair("neo-vm", "neo-node", dep);
    assert_eq!(graph.edges.len(), 1);
    let error = graph.validate_edges(ALLOWED_EDGES).unwrap_err();
    for detail in ["hidden_node", "kind build", "optional true", "cfg(any())"] {
        assert!(error.contains(detail));
    }
}

#[test]
fn test_metadata_nested_members_and_signer_server_are_classified() {
    let mut tool = fixture_package("validation-diff", "tools/validation-diff");
    tool["publish"] = Value::Null;
    tool["dependencies"] = serde_json::json!([fixture_dependency("neo-primitives", "primitives")]);
    let graph = WorkspaceGraph::from_metadata(&fixture_metadata(vec![
        tool,
        fixture_package("state-root-diff", "tools/state-root-diff"),
        fixture_package("neo-primitives", "primitives"),
        fixture_package("neo-signer-server", "apps/signer"),
    ]))
    .unwrap();
    assert_eq!(graph.members.len(), 4);
    assert_eq!(
        graph.members["opaque-package-id:validation-diff"].kind,
        MemberKind::Tool
    );
    assert_eq!(
        graph.members["opaque-package-id:neo-signer-server"].kind,
        MemberKind::Production(Layer::Assembly)
    );
    assert_eq!(graph.dependencies("validation-diff"), ["neo-primitives"]);
    graph.validate_edges(ALLOWED_EDGES).unwrap();
}

#[test]
fn test_metadata_tests_bench_and_fuzz_have_explicit_nonproduction_roles() {
    let mut fuzz = fixture_package("neo-rs-fuzz", "fuzz");
    fuzz["metadata"] = serde_json::json!({"cargo-fuzz": true});
    let mut bench = fixture_package("neo-benches", "benches-package");
    bench["publish"] = Value::Null;
    let graph = WorkspaceGraph::from_metadata(&fixture_metadata(vec![
        fixture_package("neo-tests", "tests"),
        bench,
        fuzz,
    ]))
    .unwrap();
    for (name, kind) in [
        ("neo-tests", MemberKind::Tests),
        ("neo-benches", MemberKind::Bench),
        ("neo-rs-fuzz", MemberKind::Fuzz),
    ] {
        assert_eq!(
            graph.members[&format!("opaque-package-id:{name}")].kind,
            kind
        );
    }
}

#[test]
fn test_metadata_unknown_production_fails_closed_even_under_tools() {
    for name in ["neo-new-production", "unprefixed-production"] {
        let error = WorkspaceGraph::from_metadata(&fixture_metadata(vec![fixture_package(
            name,
            "tools/unknown",
        )]))
        .unwrap_err();
        assert!(error.contains("Unclassified workspace member"));
    }
}

#[test]
fn test_metadata_tool_identity_cannot_be_reused_as_a_production_exemption() {
    let error = WorkspaceGraph::from_metadata(&fixture_metadata(vec![fixture_package(
        "validation-diff",
        "production/validation",
    )]))
    .unwrap_err();
    assert!(error.contains("Unclassified"));
    let mut tests = fixture_package("neo-tests", "tests");
    tests["publish"] = Value::Null;
    assert!(
        WorkspaceGraph::from_metadata(&fixture_metadata(vec![tests]))
            .unwrap_err()
            .contains("publish=false")
    );
}

#[test]
fn test_metadata_dependency_path_and_package_name_must_agree() {
    let dep = fixture_dependency("neo-io", "neo-primitives");
    let mut vm = fixture_package("neo-vm", "neo-vm");
    vm["dependencies"] = serde_json::json!([dep]);
    let metadata = fixture_metadata(vec![
        vm,
        fixture_package("neo-io", "neo-io"),
        fixture_package("neo-primitives", "neo-primitives"),
    ]);
    assert!(
        WorkspaceGraph::from_metadata(&metadata)
            .unwrap_err()
            .contains("identity/path mismatch")
    );
    let mut missing_path = fixture_dependency("neo-io", "neo-io");
    missing_path["path"] = Value::Null;
    let mut vm = fixture_package("neo-vm", "neo-vm");
    vm["dependencies"] = serde_json::json!([missing_path]);
    assert!(
        WorkspaceGraph::from_metadata(&fixture_metadata(vec![
            vm,
            fixture_package("neo-io", "neo-io")
        ]))
        .is_err()
    );
}

#[test]
fn test_metadata_registry_dependencies_may_omit_path_but_local_paths_cannot() {
    let external = serde_json::json!({
        "name": "bitflags", "source": "registry+https://github.com/rust-lang/crates.io-index",
        "req": "^2.4", "kind": null, "rename": null, "optional": false,
        "uses_default_features": true, "features": [], "target": null, "registry": null
    });
    let mut package = fixture_package("neo-primitives", "neo-primitives");
    package["dependencies"] = serde_json::json!([external]);
    let graph = WorkspaceGraph::from_metadata(&fixture_metadata(vec![package.clone()])).unwrap();
    assert!(graph.edges.is_empty());
    graph.validate_edges(ALLOWED_EDGES).unwrap();
    package["dependencies"][0]["source"] = Value::Null;
    assert!(WorkspaceGraph::from_metadata(&fixture_metadata(vec![package])).is_err());
}

#[test]
fn test_metadata_invalid_or_incomplete_declarations_fail_closed() {
    let base = fixture_dependency("neo-io", "neo-io");
    for field in ["name", "rename", "kind", "optional", "target", "source"] {
        let mut dep = base.clone();
        dep.as_object_mut().unwrap().remove(field);
        let mut package = fixture_package("neo-vm", "neo-vm");
        package["dependencies"] = serde_json::json!([dep]);
        assert!(
            WorkspaceGraph::from_metadata(&fixture_metadata(vec![
                package,
                fixture_package("neo-io", "neo-io")
            ]))
            .is_err(),
            "Missing field accepted: {field}"
        );
    }
    let mut metadata = fixture_metadata(vec![fixture_package("neo-primitives", "neo-primitives")]);
    metadata["version"] = serde_json::json!(2);
    assert!(WorkspaceGraph::from_metadata(&metadata).is_err());
    assert!(WorkspaceGraph::from_metadata(&fixture_metadata(vec![])).is_err());
}

#[test]
fn test_metadata_member_ids_are_authoritative_and_missing_ids_fail() {
    let mut metadata = fixture_metadata(vec![fixture_package("neo-primitives", "types")]);
    metadata["packages"]
        .as_array_mut()
        .unwrap()
        .push(fixture_package("external-not-a-member", "external"));
    let graph = WorkspaceGraph::from_metadata(&metadata).unwrap();
    assert_eq!(graph.members.len(), 1);
    metadata["workspace_members"] = serde_json::json!(["missing-package-id"]);
    assert!(
        WorkspaceGraph::from_metadata(&metadata)
            .unwrap_err()
            .contains("Missing workspace package identity")
    );
}

#[test]
fn test_metadata_duplicate_id_name_or_manifest_fails_closed() {
    let package = fixture_package("neo-io", "io");
    assert!(
        WorkspaceGraph::from_metadata(&fixture_metadata(vec![package.clone(), package])).is_err()
    );
    let mut second = fixture_package("neo-json", "io");
    let first = fixture_package("neo-io", "io");
    assert!(
        WorkspaceGraph::from_metadata(&fixture_metadata(vec![first.clone(), second.clone()]))
            .is_err()
    );
    second["manifest_path"] = serde_json::json!(fixture_root().join("json/Cargo.toml"));
    second["name"] = serde_json::json!("neo-io");
    assert!(WorkspaceGraph::from_metadata(&fixture_metadata(vec![first, second])).is_err());
}

#[test]
fn test_illegal_same_layer_edge_is_not_granted_by_layer_order() {
    let graph = fixture_pair(
        "neo-json",
        "neo-primitives",
        fixture_dependency("neo-primitives", "neo-primitives"),
    );
    assert!(
        graph
            .validate_edges(ALLOWED_EDGES)
            .unwrap_err()
            .contains("Forbidden dependency")
    );
    let allowed = fixture_pair(
        "neo-crypto",
        "neo-io",
        fixture_dependency("neo-io", "neo-io"),
    );
    allowed.validate_edges(ALLOWED_EDGES).unwrap();
}

#[test]
fn test_same_layer_cycle_fails_even_when_individual_edges_are_permitted() {
    let mut io = fixture_package("neo-io", "neo-io");
    let mut crypto = fixture_package("neo-crypto", "neo-crypto");
    io["dependencies"] = serde_json::json!([fixture_dependency("neo-crypto", "neo-crypto")]);
    let mut hidden = fixture_dependency("neo-io", "neo-io");
    hidden["rename"] = serde_json::json!("hidden_io");
    hidden["kind"] = serde_json::json!("build");
    hidden["optional"] = serde_json::json!(true);
    hidden["target"] = serde_json::json!("cfg(any())");
    crypto["dependencies"] = serde_json::json!([hidden]);
    let graph = WorkspaceGraph::from_metadata(&fixture_metadata(vec![io, crypto])).unwrap();
    let error = graph
        .validate_edges(&[("neo-io", "neo-crypto"), ("neo-crypto", "neo-io")])
        .unwrap_err();
    assert!(error.contains("Circular dependency"));
    assert!(error.contains("neo-io") && error.contains("neo-crypto"));
}

#[test]
fn test_native_to_smart_contract_is_one_way_and_other_reverse_edges_fail() {
    let graph = fixture_pair(
        "neo-native-contracts",
        "neo-smart-contract",
        fixture_dependency("neo-smart-contract", "neo-smart-contract"),
    );
    graph.validate_edges(ALLOWED_EDGES).unwrap();
    for (from, to) in [
        ("neo-smart-contract", "neo-native-contracts"),
        ("neo-ledger", "neo-mempool"),
        ("neo-protocol", "neo-wallets"),
        ("neo-wallets", "neo-system"),
        ("neo-wallets", "neo-consensus"),
        ("neo-smart-contract", "neo-oracle"),
    ] {
        let graph = fixture_pair(from, to, fixture_dependency(to, to));
        assert!(
            graph.validate_edges(ALLOWED_EDGES).is_err(),
            "Reverse edge escaped: {from} -> {to}"
        );
    }
}

#[test]
fn test_production_cannot_depend_on_nonproduction_members() {
    let mut vm = fixture_package("neo-vm", "neo-vm");
    vm["dependencies"] = serde_json::json!([fixture_dependency("neo-tests", "tests")]);
    let graph = WorkspaceGraph::from_metadata(&fixture_metadata(vec![
        vm,
        fixture_package("neo-tests", "tests"),
    ]))
    .unwrap();
    assert!(graph.validate_edges(ALLOWED_EDGES).is_err());
}

#[test]
fn test_static_policy_is_acyclic() {
    let mut packages = BTreeMap::new();
    let mut pairs = BTreeSet::new();
    for &(from, to) in ALLOWED_EDGES.iter() {
        assert!(
            pairs.insert((from, to)),
            "Duplicate policy edge {from} -> {to}"
        );
        for name in [from, to] {
            packages
                .entry(name)
                .or_insert_with(|| fixture_package(name, name));
        }
        packages.get_mut(from).unwrap()["dependencies"]
            .as_array_mut()
            .unwrap()
            .push({
                let mut dependency = fixture_dependency(to, to);
                if DEV_ONLY_EDGES.contains(&(from, to)) {
                    dependency["kind"] = serde_json::json!("dev");
                }
                dependency
            });
    }
    // Keep the dev-only whitelist honest: every entry must be a real policy edge.
    for &(from, to) in DEV_ONLY_EDGES {
        assert!(
            pairs.contains(&(from, to)),
            "Stale dev-only edge {from} -> {to} is not an allowed edge"
        );
    }
    let graph =
        WorkspaceGraph::from_metadata(&fixture_metadata(packages.into_values().collect())).unwrap();
    graph.validate_edges(ALLOWED_EDGES).unwrap();
}

// ============================================================================
// Cross-Layer Type Compatibility Tests
// ============================================================================

#[test]
fn test_primitives_types_usable() {
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

#[test]
fn test_io_serialization_with_primitives() {
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

#[test]
fn test_crypto_hash_functions() {
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

#[test]
fn test_storage_key_builder() {
    use neo_storage::KeyBuilder;

    // Verify storage key building works
    // KeyBuilder::new(id, prefix, max_length)
    let mut builder = KeyBuilder::new(1, 0x01, 64);
    builder.add_byte(0x02);
    builder.add_byte(0x03);
    let key = builder.to_bytes();

    assert!(!key.is_empty());
}

#[test]
fn test_json_object_creation() {
    use neo_json::{JObject, JToken};

    // Test JObject creation and property access
    let mut obj = JObject::new();
    obj.insert("key".to_string(), JToken::String("value".to_string()));
    obj.insert("number".to_string(), JToken::Number(42.0));

    assert!(obj.contains_property("key"));
    assert!(obj.contains_property("number"));
}
