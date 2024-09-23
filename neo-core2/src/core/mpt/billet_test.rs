use std::convert::TryInto;
use std::panic;

use crate::core::mpt::*;
use crate::core::storage;
use crate::core::util;
use crate::io;
use crate::util::test_store::new_test_store;
use crate::util::to_nibbles;
use crate::util::uint256::Uint256;

#[test]
fn test_billet_restore_hash_node() {
    fn check(tr: &Billet, expected_root: &Node, expected_node: &Node, expected_ref_count: u32) {
        let _ = expected_root.hash();
        let _ = tr.root.hash();
        assert_eq!(expected_root, &tr.root);
        let expected_bytes = tr.store.get(&make_storage_key(expected_node.hash()));
        if expected_ref_count != 0 {
            assert!(expected_bytes.is_ok());
            let expected_bytes = expected_bytes.unwrap();
            assert_eq!(
                expected_ref_count,
                u32::from_le_bytes(expected_bytes[expected_bytes.len() - 4..].try_into().unwrap())
            );
        } else {
            assert!(matches!(expected_bytes, Err(storage::Error::KeyNotFound)));
        }
    }

    #[test]
    fn parent_is_extension() {
        #[test]
        fn restore_branch() {
            let mut b = BranchNode::new();
            b.children[0] = Some(Box::new(ExtensionNode::new(
                vec![0x01],
                Box::new(LeafNode::new(vec![0xAB, 0xCD])),
            )));
            b.children[5] = Some(Box::new(ExtensionNode::new(
                vec![0x01],
                Box::new(LeafNode::new(vec![0xAB, 0xDE])),
            )));
            let path = to_nibbles(&[0xAC]);
            let e = ExtensionNode::new(path.clone(), Box::new(HashNode::new(b.hash())));
            let mut tr = Billet::new(e.hash(), Mode::Latest, storage::StorageType::TempStorage, new_test_store());
            tr.root = Box::new(e.clone());

            // OK
            let mut n = NodeObject::new();
            n.decode_binary(&mut io::BinReader::new(b.bytes()));
            assert!(tr.restore_hash_node(&path, &n.node).is_ok());
            let expected = ExtensionNode::new(path.clone(), Box::new(n.node.clone()));
            check(&tr, &expected, &n.node, 1);

            // One more time (already restored) => panic expected, no refcount changes
            assert!(panic::catch_unwind(|| {
                tr.restore_hash_node(&path, &n.node).unwrap();
            })
            .is_err());
            check(&tr, &expected, &n.node, 1);

            // Same path, but wrong hash => error expected, no refcount changes
            assert_eq!(
                tr.restore_hash_node(&path, &BranchNode::new()).unwrap_err(),
                Error::RestoreFailed
            );
            check(&tr, &expected, &n.node, 1);

            // New path (changes in the MPT structure are not allowed) => error expected, no refcount changes
            assert_eq!(
                tr.restore_hash_node(&to_nibbles(&[0xAB]), &n.node).unwrap_err(),
                Error::RestoreFailed
            );
            check(&tr, &expected, &n.node, 1);
        }

        #[test]
        fn restore_leaf() {
            let l = LeafNode::new(vec![0xAB, 0xCD]);
            let path = to_nibbles(&[0xAC]);
            let e = ExtensionNode::new(path.clone(), Box::new(HashNode::new(l.hash())));
            let mut tr = Billet::new(e.hash(), Mode::Latest, storage::StorageType::TempStorage, new_test_store());
            tr.root = Box::new(e.clone());

            // OK
            assert!(tr.restore_hash_node(&path, &l).is_ok());
            let mut expected = HashNode::new(e.hash());
            expected.collapsed = true;
            check(&tr, &expected, &l, 1);

            // One more time (already restored and collapsed) => error expected, no refcount changes
            assert!(tr.restore_hash_node(&path, &l).is_err());
            check(&tr, &expected, &l, 1);

            // Same path, but wrong hash => error expected, no refcount changes
            assert_eq!(
                tr.restore_hash_node(&path, &LeafNode::new(vec![0xAB, 0xEF])).unwrap_err(),
                Error::RestoreFailed
            );
            check(&tr, &expected, &l, 1);

            // New path (changes in the MPT structure are not allowed) => error expected, no refcount changes
            assert_eq!(
                tr.restore_hash_node(&to_nibbles(&[0xAB]), &l).unwrap_err(),
                Error::RestoreFailed
            );
            check(&tr, &expected, &l, 1);
        }

        #[test]
        fn restore_hash() {
            let h = HashNode::new(Uint256::from([1, 2, 3]));
            let path = to_nibbles(&[0xAC]);
            let e = ExtensionNode::new(path.clone(), Box::new(h.clone()));
            let mut tr = Billet::new(e.hash(), Mode::Latest, storage::StorageType::TempStorage, new_test_store());
            tr.root = Box::new(e.clone());

            // no-op
            assert_eq!(
                tr.restore_hash_node(&path, &h).unwrap_err(),
                Error::RestoreFailed
            );
            check(&tr, &e, &h, 0);
        }
    }

    #[test]
    fn parent_is_leaf() {
        let l = LeafNode::new(vec![0xAB, 0xCD]);
        let path = vec![];
        let mut tr = Billet::new(l.hash(), Mode::Latest, storage::StorageType::TempStorage, new_test_store());
        tr.root = Box::new(l.clone());

        // Already restored => panic expected
        assert!(panic::catch_unwind(|| {
            tr.restore_hash_node(&path, &l).unwrap();
        })
        .is_err());

        // Same path, but wrong hash => error expected, no refcount changes
        assert_eq!(
            tr.restore_hash_node(&path, &LeafNode::new(vec![0xAB, 0xEF])).unwrap_err(),
            Error::RestoreFailed
        );

        // Non-nil path, but MPT structure can't be changed => error expected, no refcount changes
        assert_eq!(
            tr.restore_hash_node(&to_nibbles(&[0xAC]), &LeafNode::new(vec![0xAB, 0xEF])).unwrap_err(),
            Error::RestoreFailed
        );
    }

    #[test]
    fn parent_is_branch() {
        #[test]
        fn middle_child() {
            let l1 = LeafNode::new(vec![0xAB, 0xCD]);
            let l2 = LeafNode::new(vec![0xAB, 0xDE]);
            let mut b = BranchNode::new();
            b.children[5] = Some(Box::new(HashNode::new(l1.hash())));
            b.children[last_child] = Some(Box::new(HashNode::new(l2.hash())));
            let mut tr = Billet::new(b.hash(), Mode::Latest, storage::StorageType::TempStorage, new_test_store());
            tr.root = Box::new(b.clone());

            // OK
            let path = vec![0x05];
            assert!(tr.restore_hash_node(&path, &l1).is_ok());
            check(&tr, &b, &l1, 1);

            // One more time (already restored) => panic expected.
            // It's an MPT pool duty to avoid such situations during real restore process.
            assert!(panic::catch_unwind(|| {
                tr.restore_hash_node(&path, &l1).unwrap();
            })
            .is_err());
            // No refcount changes expected.
            check(&tr, &b, &l1, 1);

            // Same path, but wrong hash => error expected, no refcount changes
            assert_eq!(
                tr.restore_hash_node(&path, &LeafNode::new(vec![0xAD])).unwrap_err(),
                Error::RestoreFailed
            );
            check(&tr, &b, &l1, 1);

            // New path pointing to the empty HashNode (changes in the MPT structure are not allowed) => error expected, no refcount changes
            assert_eq!(
                tr.restore_hash_node(&vec![0x01], &l1).unwrap_err(),
                Error::RestoreFailed
            );
            check(&tr, &b, &l1, 1);
        }

        #[test]
        fn last_child() {
            let l1 = LeafNode::new(vec![0xAB, 0xCD]);
            let l2 = LeafNode::new(vec![0xAB, 0xDE]);
            let mut b = BranchNode::new();
            b.children[5] = Some(Box::new(HashNode::new(l1.hash())));
            b.children[last_child] = Some(Box::new(HashNode::new(l2.hash())));
            let mut tr = Billet::new(b.hash(), Mode::Latest, storage::StorageType::TempStorage, new_test_store());
            tr.root = Box::new(b.clone());

            // OK
            let path = vec![];
            assert!(tr.restore_hash_node(&path, &l2).is_ok());
            check(&tr, &b, &l2, 1);

            // One more time (already restored) => panic expected.
            // It's an MPT pool duty to avoid such situations during real restore process.
            assert!(panic::catch_unwind(|| {
                tr.restore_hash_node(&path, &l2).unwrap();
            })
            .is_err());
            // No refcount changes expected.
            check(&tr, &b, &l2, 1);

            // Same path, but wrong hash => error expected, no refcount changes
            assert_eq!(
                tr.restore_hash_node(&path, &LeafNode::new(vec![0xAD])).unwrap_err(),
                Error::RestoreFailed
            );
            check(&tr, &b, &l2, 1);
        }

        #[test]
        fn two_children_with_same_hash() {
            let l = LeafNode::new(vec![0xAB, 0xCD]);
            let mut b = BranchNode::new();
            // two same hashnodes => leaf's refcount expected to be 2 in the end.
            b.children[3] = Some(Box::new(HashNode::new(l.hash())));
            b.children[4] = Some(Box::new(HashNode::new(l.hash())));
            let mut tr = Billet::new(b.hash(), Mode::Latest, storage::StorageType::TempStorage, new_test_store());
            tr.root = Box::new(b.clone());

            // OK
            assert!(tr.restore_hash_node(&vec![0x03], &l).is_ok());
            let mut expected = b.clone();
            if let Some(HashNode { collapsed, .. }) = expected.children[3].as_mut().map(|n| n.as_mut()) {
                *collapsed = true;
            }
            check(&tr, &b, &l, 1);

            // Restore another node with the same hash => no error expected, refcount should be incremented.
            // Branch node should be collapsed.
            assert!(tr.restore_hash_node(&vec![0x04], &l).is_ok());
            let mut res = HashNode::new(b.hash());
            res.collapsed = true;
            check(&tr, &res, &l, 2);
        }
    }

    #[test]
    fn parent_is_hash() {
        let l = LeafNode::new(vec![0xAB, 0xCD]);
        let mut b = BranchNode::new();
        b.children[3] = Some(Box::new(HashNode::new(l.hash())));
        b.children[4] = Some(Box::new(HashNode::new(l.hash())));
        let mut tr = Billet::new(b.hash(), Mode::Latest, storage::StorageType::TempStorage, new_test_store());

        // Should fail, because if it's a hash node with non-empty path, then the node
        // has already been collapsed.
        assert!(tr.restore_hash_node(&vec![0x03], &l).is_err());
    }
}
