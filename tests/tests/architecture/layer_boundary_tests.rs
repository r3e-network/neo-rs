//! Layer Boundary Integration Tests
//!
//! Validates the architectural layering of the neo-rs workspace:
//!
//! ```text
//! Layer 0 (Foundation): neo-primitives
//! Layer 1 (Infrastructure): neo-io, neo-error, neo-crypto, neo-trie, neo-storage, neo-static-files, neo-state-packs, neo-checkpoint, neo-config, neo-vm, neo-serialization, neo-manifest
//! Layer 2 (Protocol): neo-payloads, neo-consensus, neo-hsm
//! Layer 3 (Domain services): neo-execution, neo-native-contracts, neo-mempool, neo-state-service, neo-runtime
//! Layer 4 (Node services): neo-blockchain, neo-network, neo-wallets, neo-indexer, neo-oracle-service
//! Layer 5 (Composition): neo-system
//! Layer 6 (Plugin/RPC boundary): neo-rpc
//! Layer 7 (Applications): neo-node, neo-gui
//! ```

mod layer_boundary_ownership;
mod layer_boundary_services;
mod layer_boundary_smoke;
mod layer_boundary_workspace;

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
            "neo-io" | "neo-error" | "neo-crypto" | "neo-trie" | "neo-storage"
            | "neo-static-files" | "neo-state-packs" | "neo-checkpoint" | "neo-config"
            | "neo-vm" | "neo-serialization" | "neo-manifest" => Some(Layer::Infrastructure),
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
