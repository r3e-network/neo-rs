//! Dependency-cycle checks and cross-layer API smoke tests.

use super::*;

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
