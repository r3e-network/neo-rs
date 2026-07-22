use super::*;
use neo_io::var_int::VarInt;
use std::thread;

// ============================================================================
// UT_Node.cs Tests (20 tests)
// ============================================================================

#[test]
fn test_hash_serialize() {
    let node = prepare_mpt_node1();
    let data = node.to_array().unwrap();
    assert!(!data.is_empty());

    let deserialized = deserialize_node(&data);
    assert_eq!(deserialized.node_type, NodeType::HashNode);
    assert_eq!(deserialized.hash(), node.hash());
}

#[test]
fn cached_hash_is_consistent_across_concurrent_readers() {
    let node = Arc::new(prepare_mpt_node3());
    let expected = node.hash();

    thread::scope(|scope| {
        for _ in 0..8 {
            let node = Arc::clone(&node);
            scope.spawn(move || {
                for _ in 0..1_000 {
                    assert_eq!(node.try_hash().unwrap(), expected);
                }
            });
        }
    });
}

#[test]
fn dirty_node_recomputes_cached_hash() {
    let mut node = Node::new_leaf(vec![1, 2, 3]);
    let original = node.try_hash().unwrap();

    node.value.push(4);
    node.set_dirty();
    let updated = node.try_hash().unwrap();

    assert_ne!(original, updated);
    assert!(node.hash_is_cached());
}

#[test]
fn test_empty_serialize() {
    let node = Node::new();
    let data = node.to_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0], NodeType::Empty as u8);

    let deserialized = deserialize_node(&data);
    assert_eq!(deserialized.node_type, NodeType::Empty);
    assert!(deserialized.is_empty());
}

#[test]
fn test_leaf_serialize() {
    let node = prepare_mpt_node2();
    let data = node.to_array().unwrap();
    assert!(!data.is_empty());

    let deserialized = deserialize_node(&data);
    assert_eq!(deserialized.node_type, NodeType::LeafNode);
    assert_eq!(deserialized.value, vec![0x12, 0x34]);
}

#[test]
fn leaf_bytes_and_hash_match_csharp_v3101_vector() {
    let node = prepare_mpt_node2();

    assert_eq!(
        node.to_array().unwrap(),
        [0x02, 0x02, 0x12, 0x34, 0x01],
        "LeafNode type, var-bytes value, and reference must retain Neo C# byte order"
    );
    assert_eq!(
        node.to_array_without_reference().unwrap(),
        [0x02, 0x02, 0x12, 0x34],
        "the MPT hash preimage excludes the reference count"
    );
    assert_eq!(
        node.try_hash().unwrap().to_bytes(),
        hex::decode("e9e57e883fa9f438b48bac8c7abd1536115892e9137d410dc56c1452375e5020")
            .expect("fixed hash vector is valid hex"),
        "Hash256 over the C# leaf preimage must remain byte-for-byte stable"
    );
}

#[test]
fn test_leaf_serialize_as_child() {
    let node = prepare_mpt_node2();
    let buffer = serialize_child(&node);
    assert!(!buffer.is_empty());
}

#[test]
fn test_extension_serialize() {
    let leaf = prepare_mpt_node2();
    let ext = Node::new_extension(vec![0x01, 0x02], leaf).unwrap();
    let data = ext.to_array().unwrap();
    assert!(!data.is_empty());

    let deserialized = deserialize_node(&data);
    assert_eq!(deserialized.node_type, NodeType::ExtensionNode);
    assert_eq!(deserialized.key, vec![0x01, 0x02]);
    assert!(deserialized.next.is_some());
}

#[test]
fn test_extension_serialize_as_child() {
    let leaf = prepare_mpt_node2();
    let ext = Node::new_extension(vec![0x01], leaf).unwrap();
    let buffer = serialize_child(&ext);
    assert!(!buffer.is_empty());
}

#[test]
fn test_branch_serialize() {
    let branch = prepare_mpt_node3();
    let data = branch.to_array().unwrap();
    assert!(!data.is_empty());

    let deserialized = deserialize_node(&data);
    assert_eq!(deserialized.node_type, NodeType::BranchNode);
    assert_eq!(deserialized.children.len(), 17);
}

#[test]
fn test_branch_serialize_as_child() {
    let branch = prepare_mpt_node3();
    let buffer = serialize_child(&branch);
    assert!(!buffer.is_empty());
}

#[test]
fn test_clone_branch() {
    let branch1 = prepare_mpt_node3();
    let branch2 = branch1.clone();
    assert_eq!(branch1.node_type, branch2.node_type);
    assert_eq!(branch1.hash(), branch2.hash());
    assert_eq!(branch2.children[2].node_type, NodeType::HashNode);
    assert_eq!(branch2.children[2].hash(), branch1.children[2].hash());
    assert!(
        Arc::ptr_eq(&branch1.children[0], &branch2.children[0]),
        "empty branch children are immutable and should retain C# CloneAsChild identity"
    );
    assert!(
        !Arc::ptr_eq(&branch1.children[2], &branch2.children[2]),
        "materialized children must become independent hash nodes"
    );
}

#[test]
fn cloned_shared_empty_child_uses_copy_on_write() {
    let branch1 = Node::new_branch();
    let mut branch2 = branch1.clone();

    assert!(Arc::ptr_eq(&branch1.children[0], &branch2.children[0]));
    *branch2
        .get_child_mut(0)
        .expect("branch clone retains child slot") = Node::new_leaf(vec![1, 2, 3]);

    assert!(branch1.children[0].is_empty());
    assert_eq!(branch2.children[0].node_type, NodeType::LeafNode);
    assert!(!Arc::ptr_eq(&branch1.children[0], &branch2.children[0]));
}

#[test]
fn test_clone_extension() {
    let leaf = prepare_mpt_node2();
    let ext1 = Node::new_extension(vec![0x01, 0x02], leaf).unwrap();
    let ext2 = ext1.clone();
    assert_eq!(ext1.node_type, ext2.node_type);
    assert_eq!(ext1.key, ext2.key);
    assert_eq!(ext1.hash(), ext2.hash());
    let next = ext2.next.as_ref().expect("cloned extension keeps child");
    assert_eq!(next.node_type, NodeType::HashNode);
    assert_eq!(
        next.hash(),
        ext1.next.as_ref().expect("source extension child").hash()
    );
}

#[test]
fn clone_extension_shares_immutable_hash_child() {
    let ext1 = Node::new_extension(vec![0x01], Node::new_hash(UInt256::zero())).unwrap();
    let ext2 = ext1.clone();

    assert!(Arc::ptr_eq(
        ext1.next.as_ref().expect("source extension child"),
        ext2.next.as_ref().expect("cloned extension child")
    ));
}

#[test]
fn test_clone_leaf() {
    let leaf1 = prepare_mpt_node2();
    let leaf2 = leaf1.clone();
    assert_eq!(leaf1.value, leaf2.value);
    assert_eq!(leaf1.hash(), leaf2.hash());
}

#[test]
fn test_new_extension_exception() {
    // Extension with empty key should fail
    let leaf = prepare_mpt_node2();
    let result = Node::new_extension(vec![], leaf);
    assert!(result.is_err());
}

#[test]
fn test_new_hash_exception() {
    // C# throws on null input; Rust cannot represent null hashes.
    let hash = UInt256::zero();
    let node = Node::new_hash(hash);
    assert_eq!(node.node_type, NodeType::HashNode);
    assert_eq!(node.hash(), hash);
}

#[test]
fn test_new_leaf_exception() {
    // C# throws on null input; empty values are valid and should serialize.
    let leaf = Node::new_leaf(vec![]);
    assert_eq!(leaf.node_type, NodeType::LeafNode);
    assert_eq!(leaf.value.len(), 0);
}

#[test]
fn test_size() {
    let hash_node = prepare_mpt_node1();
    let hash_size = hash_node.to_array().unwrap().len();
    assert!(hash_size > 0);

    let leaf = prepare_mpt_node2();
    let leaf_size = leaf.to_array().unwrap().len();
    assert!(leaf_size > 0);

    let branch = prepare_mpt_node3();
    let branch_size = branch.to_array().unwrap().len();
    assert!(branch_size > leaf_size);
}

#[test]
fn node_without_reference_serialization_allocates_exact_size() {
    for node in [
        Node::new(),
        prepare_mpt_node1(),
        prepare_mpt_node2(),
        Node::new_extension(vec![0x01, 0x02], prepare_mpt_node2()).unwrap(),
        prepare_mpt_node3(),
    ] {
        let data = node.to_array_without_reference().unwrap();
        assert_eq!(
            data.capacity(),
            data.len(),
            "node type {:?} should preallocate the exact hash payload size",
            node.node_type
        );
    }
}

#[test]
fn negative_reference_rejects_serialization_like_csharp_var_int() {
    let mut node = Node::new_leaf(b"overflowed-reference".to_vec());
    node.reference = -1;

    let mut writer = BinaryWriter::new();
    let error = node
        .serialize(&mut writer)
        .expect_err("C# WriteVarInt rejects a negative reference");
    assert!(
        error
            .to_string()
            .contains("reference count cannot be negative")
    );
    assert_eq!(
        node.byte_size(),
        node.to_array_without_reference().unwrap().len() + 1,
        "C# Node.Size still reports a one-byte var-int for a negative int"
    );
}

#[test]
fn sizing_embedded_child_does_not_compute_hash() {
    let node = prepare_mpt_node2();

    assert_eq!(node.byte_size_as_child(), 1 + neo_primitives::UINT256_SIZE);
    assert!(
        !node.hash_is_cached(),
        "child size estimation should not force hash serialization"
    );
}

#[test]
fn test_from_replica() {
    let original = prepare_mpt_node3();
    let data = original.to_array().unwrap();
    let replica = deserialize_node(&data);
    assert_eq!(original.node_type, replica.node_type);
    assert_eq!(original.hash(), replica.hash());
}

#[test]
fn test_empty_leaf() {
    let empty = Node::new();
    assert!(empty.is_empty());
    assert_eq!(empty.node_type, NodeType::Empty);

    let leaf = Node::new_leaf(vec![]);
    assert_eq!(leaf.node_type, NodeType::LeafNode);
    assert!(!leaf.is_empty());
}

#[test]
fn deserialize_rejects_nesting_deeper_than_max_key_length_like_csharp_v3101() {
    let entry = malicious_nested_extension_entry(MAX_KEY_LENGTH + 1);
    let mut reader = MemoryReader::new(&entry);
    assert!(
        Node::deserialize(&mut reader).is_err(),
        "C# Neo.Cryptography.MPT v3.10.1 rejects MPT node nesting deeper than MaxKeyLength"
    );
}

#[test]
fn split_serialized_reference_round_trips_borrowed_and_owned_payloads() {
    let mut leaf = Node::new_leaf(vec![0x12, 0x34]);
    leaf.reference = 7;
    let extension = Node::new_extension(vec![0x01, 0x02], leaf.clone()).unwrap();
    let nodes = [
        Node::new(),
        Node::new_hash(UInt256::zero()),
        leaf,
        extension,
        prepare_mpt_node3(),
    ];

    for node in nodes {
        let serialized = node.to_array().unwrap();
        let (node_type, reference, payload) =
            Node::split_serialized_reference(&serialized).unwrap();
        let rebuilt = Node::array_from_payload_parts(node_type, reference, &payload).unwrap();
        assert_eq!(
            rebuilt, serialized,
            "borrowed split changed {:?}",
            node_type
        );

        let (owned_type, owned_reference, owned_payload) =
            Node::split_serialized_reference_owned(serialized.clone()).unwrap();
        assert_eq!(
            (owned_type, owned_reference),
            (node_type, reference),
            "owned and borrowed split metadata disagree for {:?}",
            node_type
        );
        assert_eq!(
            owned_payload, payload,
            "owned and borrowed split payloads disagree for {:?}",
            node_type
        );
        let rebuilt_owned =
            Node::array_from_payload_parts_owned(owned_type, owned_reference, owned_payload)
                .unwrap();
        assert_eq!(
            rebuilt_owned, serialized,
            "owned split changed {:?}",
            node_type
        );
    }
}

#[test]
fn split_serialized_reference_rejects_trailing_bytes_and_excessive_depth() {
    let mut malformed = Node::new_leaf(vec![0xAA]).to_array().unwrap();
    malformed.push(0xFF);
    assert!(
        Node::split_serialized_reference(&malformed)
            .expect_err("trailing bytes must be rejected")
            .to_string()
            .contains("trailing bytes")
    );

    let nested = malicious_nested_extension_entry(MAX_KEY_LENGTH + 1);
    assert!(
        Node::split_serialized_reference(&nested)
            .expect_err("excessive nesting must be rejected")
            .to_string()
            .contains("nesting depth")
    );
}

#[test]
fn persisted_node_validation_binds_canonical_bytes_to_storage_hash() {
    for mut node in [
        Node::new_leaf(vec![0x12, 0x34]),
        Node::new_extension(vec![0x01, 0x02], prepare_mpt_node2()).unwrap(),
        prepare_mpt_node3(),
    ] {
        node.reference = 7;
        let expected_hash = node.try_hash().unwrap();
        let bytes = node.to_array().unwrap();

        Node::validate_persisted(&bytes, expected_hash).unwrap();
        let decoded = Node::deserialize_persisted(&bytes, expected_hash).unwrap();
        assert_eq!(decoded.node_type, node.node_type);
        assert_eq!(decoded.reference, 7);
        assert_eq!(decoded.try_hash().unwrap(), expected_hash);
    }
}

#[test]
fn persisted_node_validation_rejects_wrong_hash_and_trailing_bytes() {
    let node = Node::new_leaf(vec![0xAA]);
    let expected_hash = node.try_hash().unwrap();
    let bytes = node.to_array().unwrap();

    assert!(
        Node::validate_persisted(&bytes, UInt256::zero())
            .expect_err("storage key must bind the row payload")
            .to_string()
            .contains("does not match")
    );

    let mut trailing = bytes;
    trailing.push(0xFF);
    assert!(
        Node::validate_persisted(&trailing, expected_hash)
            .expect_err("trailing bytes must be rejected")
            .to_string()
            .contains("trailing bytes")
    );
}

#[test]
fn persisted_node_validation_rejects_non_rows_and_invalid_references() {
    for node in [Node::new(), Node::new_hash(UInt256::zero())] {
        let bytes = node.to_array().unwrap();
        assert!(
            Node::validate_persisted(&bytes, node.try_hash().unwrap())
                .expect_err("only materialized nodes may occupy persisted rows")
                .to_string()
                .contains("materialized node")
        );
    }

    let leaf = Node::new_leaf(vec![0xBB]);
    let expected_hash = leaf.try_hash().unwrap();
    let payload = leaf.to_array_without_reference().unwrap();

    let mut zero_reference = payload.clone();
    VarInt::write_var_int(0, &mut zero_reference);
    assert!(
        Node::validate_persisted(&zero_reference, expected_hash)
            .expect_err("zero references cannot be persisted")
            .to_string()
            .contains("positive int32")
    );

    let mut oversized_reference = payload.clone();
    VarInt::write_var_int(i32::MAX as u64 + 1, &mut oversized_reference);
    assert!(
        Node::validate_persisted(&oversized_reference, expected_hash)
            .expect_err("persisted references must fit the C# int domain")
            .to_string()
            .contains("positive int32")
    );

    let mut noncanonical_reference = payload;
    noncanonical_reference.extend_from_slice(&[0xFD, 0x01, 0x00]);
    assert!(
        Node::validate_persisted(&noncanonical_reference, expected_hash)
            .expect_err("persisted references must use canonical var-int encoding")
            .to_string()
            .contains("canonically encoded")
    );
}
