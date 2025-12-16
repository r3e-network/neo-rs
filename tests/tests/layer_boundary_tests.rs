//! Layer Boundary Integration Tests
//!
//! Validates the architectural layering of the neo-rs workspace:
//!
//! ```text
//! Layer 0 (Foundation - no neo-* deps): neo-primitives, neo-json, neo-storage, neo-io
//! Layer 1 (Crypto): neo-crypto (depends on Layer 0)
//! Layer 2 (Protocol): neo-vm, neo-p2p, neo-consensus, neo-core
//! Layer 3 (State): neo-state, neo-mempool, neo-chain
//! Layer 4 (Services): neo-rpc, neo-config, neo-telemetry
//! Layer 5 (Application): neo-node, neo-cli
//! ```

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

/// Layer definitions for the neo-rs architecture
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Layer {
    Foundation = 0,
    Crypto = 1,
    Protocol = 2,
    State = 3,
    Services = 4,
    Application = 5,
}

impl Layer {
    fn from_crate_name(name: &str) -> Option<Self> {
        match name {
            // Layer 0: Foundation (no neo-* dependencies allowed)
            "neo-primitives" | "neo-json" | "neo-storage" => Some(Layer::Foundation),
            // neo-io is special: can depend on neo-primitives only
            "neo-io" => Some(Layer::Foundation),
            // Layer 1: Crypto (depends on Layer 0 only)
            "neo-crypto" => Some(Layer::Crypto),
            // Layer 2: Protocol
            "neo-vm" | "neo-p2p" | "neo-consensus" | "neo-core" => Some(Layer::Protocol),
            // Layer 3: State
            "neo-state" | "neo-mempool" | "neo-chain" => Some(Layer::State),
            // Layer 4: Services
            "neo-rpc" | "neo-config" | "neo-telemetry" | "neo-tee" => Some(Layer::Services),
            // Layer 5: Application
            "neo-node" | "neo-cli" => Some(Layer::Application),
            _ => None,
        }
    }
}

/// Parse Cargo.toml to extract neo-* dependencies
fn parse_neo_dependencies(cargo_toml_path: &Path) -> Vec<String> {
    let content = fs::read_to_string(cargo_toml_path).expect("Failed to read Cargo.toml");
    let mut deps = Vec::new();

    let mut in_dependencies = false;
    let mut in_dev_dependencies = false;

    for line in content.lines() {
        let trimmed = line.trim();

        // Track section headers
        if trimmed.starts_with("[dependencies]") {
            in_dependencies = true;
            in_dev_dependencies = false;
            continue;
        }
        if trimmed.starts_with("[dev-dependencies]") {
            in_dependencies = false;
            in_dev_dependencies = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_dependencies = false;
            in_dev_dependencies = false;
            continue;
        }

        // Only check [dependencies], not [dev-dependencies]
        if in_dependencies && !in_dev_dependencies {
            // Match lines like: neo-primitives = { workspace = true }
            // or: neo-vm = { path = "../neo-vm" }
            if trimmed.starts_with("neo-") {
                if let Some(name) = trimmed.split('=').next() {
                    let dep_name = name.trim().to_string();
                    if dep_name.starts_with("neo-") {
                        deps.push(dep_name);
                    }
                }
            }
        }
    }

    deps
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

#[test]
fn test_layer_0_has_no_neo_dependencies() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();

    // These Layer 0 crates should have NO neo-* dependencies (strict foundation)
    // neo-primitives: absolute foundation, no neo-* deps
    // neo-json: JSON handling, no neo-* deps
    let strict_layer_0 = ["neo-primitives", "neo-json"];

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

#[test]
fn test_storage_only_depends_on_primitives() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let cargo_toml = workspace_root.join("neo-storage").join("Cargo.toml");

    if !cargo_toml.exists() {
        return;
    }

    let deps = parse_neo_dependencies(&cargo_toml);

    // neo-storage can depend on neo-primitives (for UInt160/UInt256 key types)
    for dep in &deps {
        assert_eq!(
            dep, "neo-primitives",
            "neo-storage should only depend on neo-primitives, but found: {}",
            dep
        );
    }
}

#[test]
fn test_neo_io_only_depends_on_primitives() {
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

#[test]
fn test_crypto_only_depends_on_layer_0() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let cargo_toml = workspace_root.join("neo-crypto").join("Cargo.toml");

    if !cargo_toml.exists() {
        return;
    }

    let deps = parse_neo_dependencies(&cargo_toml);
    let allowed = ["neo-primitives", "neo-io"];

    for dep in &deps {
        assert!(
            allowed.contains(&dep.as_str()),
            "neo-crypto (Layer 1) should only depend on Layer 0 crates, but found: {}",
            dep
        );
    }
}

#[test]
fn test_no_upward_dependencies() {
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
fn test_no_circular_dependencies() {
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
