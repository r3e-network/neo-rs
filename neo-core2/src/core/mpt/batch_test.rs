use std::collections::HashMap;
use hex;
use std::fmt;
use crate::core::storage;
use crate::core::mpt::{Trie, Node, EmptyNode, HashNode, BranchNode, ExtensionNode, LeafNode, ModeAll, Batch, keyValue};
use crate::core::mpt::MapToMPTBatch;
use crate::core::mpt::newTestStore;
use crate::core::mpt::makeStorageKey;
use crate::core::mpt::isEmpty;

#[test]
fn test_batch_add() {
    let b = MapToMPTBatch(HashMap::from([
        ("a\x01".to_string(), vec![2]),
        ("a\x02\x10".to_string(), vec![3]),
        ("a\x00\x01".to_string(), vec![5]),
        ("a\x02\x00".to_string(), vec![6]),
    ]));

    let expected = vec![
        keyValue { key: vec![0, 0, 0, 1], value: vec![5] },
        keyValue { key: vec![0, 1], value: vec![2] },
        keyValue { key: vec![0, 2, 0, 0], value: vec![6] },
        keyValue { key: vec![0, 2, 1, 0], value: vec![3] },
    ];
    assert_eq!(expected, b.kv);
}

type Pairs = Vec<[Vec<u8>; 2]>;

fn test_incomplete_put(ps: Pairs, n: usize, tr1: &mut Trie, tr2: &mut Trie) {
    let mut m = HashMap::new();
    for (i, p) in ps.iter().enumerate() {
        if i < n {
            if p[1].is_empty() {
                assert!(tr1.delete(&p[0]).is_ok(), "item {}", i);
            } else {
                assert!(tr1.put(&p[0], &p[1]).is_ok(), "item {}", i);
            }
        } else if i == n {
            if p[1].is_empty() {
                assert!(tr1.delete(&p[0]).is_err(), "item {}", i);
            } else {
                assert!(tr1.put(&p[0], &p[1]).is_err(), "item {}", i);
            }
        }
        m.insert(format!("a{}", String::from_utf8_lossy(&p[0])), p[1].clone());
    }

    let b = MapToMPTBatch(m);
    let (num, err) = tr2.put_batch(b);
    if n == ps.len() {
        assert!(err.is_none());
    } else {
        assert!(err.is_some());
    }
    assert_eq!(n, num);
    assert_eq!(tr1.state_root(), tr2.state_root());

    #[test]
    fn test_restore() {
        tr2.flush(0);
        let mut tr3 = Trie::new(HashNode::new(tr2.state_root()), ModeAll, storage::new_mem_cached_store(tr2.store()));
        for p in &ps[..n] {
            let (val, err) = tr3.get(&p[0]);
            if p[1].is_empty() {
                assert!(err.is_some());
                continue;
            }
            assert!(err.is_none(), "key: {}", hex::encode(&p[0]));
            assert_eq!(p[1], val);
        }
    }
}

fn test_put(ps: Pairs, tr1: &mut Trie, tr2: &mut Trie) {
    test_incomplete_put(ps, ps.len(), tr1, tr2);
}

#[test]
fn test_trie_put_batch_leaf() {
    fn prepare_leaf() -> (Trie, Trie) {
        let mut tr1 = Trie::new(EmptyNode::new(), ModeAll, newTestStore());
        let mut tr2 = Trie::new(EmptyNode::new(), ModeAll, newTestStore());
        assert!(tr1.put(&vec![0], &b"value".to_vec()).is_ok());
        assert!(tr2.put(&vec![0], &b"value".to_vec()).is_ok());
        (tr1, tr2)
    }

    #[test]
    fn remove() {
        let (mut tr1, mut tr2) = prepare_leaf();
        let ps = vec![vec![vec![0], vec![]]];
        test_put(ps, &mut tr1, &mut tr2);
    }

    #[test]
    fn empty_value() {
        let (mut tr1, mut tr2) = prepare_leaf();
        let ps = vec![vec![vec![0], vec![]]];
        test_put(ps, &mut tr1, &mut tr2);
    }

    #[test]
    fn replace() {
        let (mut tr1, mut tr2) = prepare_leaf();
        let ps = vec![vec![vec![0], b"replace".to_vec()]];
        test_put(ps, &mut tr1, &mut tr2);
    }

    #[test]
    fn remove_and_replace() {
        let (mut tr1, mut tr2) = prepare_leaf();
        let ps = vec![
            vec![vec![0], vec![]],
            vec![vec![0, 2], b"replace2".to_vec()],
        ];
        test_put(ps, &mut tr1, &mut tr2);
    }

    #[test]
    fn empty_value_and_replace() {
        let (mut tr1, mut tr2) = prepare_leaf();
        let ps = vec![
            vec![vec![0], vec![]],
            vec![vec![0, 2], b"replace2".to_vec()],
        ];
        test_put(ps, &mut tr1, &mut tr2);
    }
}

#[test]
fn test_trie_put_batch_extension() {
    fn prepare_extension() -> (Trie, Trie) {
        let mut tr1 = Trie::new(EmptyNode::new(), ModeAll, newTestStore());
        let mut tr2 = Trie::new(EmptyNode::new(), ModeAll, newTestStore());
        assert!(tr1.put(&vec![1, 2], &b"value1".to_vec()).is_ok());
        assert!(tr2.put(&vec![1, 2], &b"value1".to_vec()).is_ok());
        (tr1, tr2)
    }

    #[test]
    fn split_key_len_gt_1() {
        let (mut tr1, mut tr2) = prepare_extension();
        let ps = vec![vec![vec![2, 3], b"value2".to_vec()]];
        test_put(ps, &mut tr1, &mut tr2);
    }

    #[test]
    fn split_key_len_eq_1() {
        let (mut tr1, mut tr2) = prepare_extension();
        let ps = vec![vec![vec![1, 3], b"value2".to_vec()]];
        test_put(ps, &mut tr1, &mut tr2);
    }

    #[test]
    fn add_to_next() {
        let (mut tr1, mut tr2) = prepare_extension();
        let ps = vec![vec![vec![1, 2, 3], b"value2".to_vec()]];
        test_put(ps, &mut tr1, &mut tr2);
    }

    #[test]
    fn add_to_next_with_leaf() {
        let (mut tr1, mut tr2) = prepare_extension();
        let ps = vec![
            vec![vec![0], b"value3".to_vec()],
            vec![vec![1, 2, 3], b"value2".to_vec()],
        ];
        test_put(ps, &mut tr1, &mut tr2);
    }

    #[test]
    fn remove_value() {
        let (mut tr1, mut tr2) = prepare_extension();
        let ps = vec![vec![vec![1, 2], vec![]]];
        test_put(ps, &mut tr1, &mut tr2);
    }

    #[test]
    fn empty_value() {
        let (mut tr1, mut tr2) = prepare_extension();
        let ps = vec![vec![vec![1, 2], vec![]]];
        test_put(ps, &mut tr1, &mut tr2);
    }

    #[test]
    fn add_to_next_merge_extension() {
        let (mut tr1, mut tr2) = prepare_extension();
        let ps = vec![
            vec![vec![1, 2], vec![]],
            vec![vec![1, 2, 3], b"value2".to_vec()],
        ];
        test_put(ps, &mut tr1, &mut tr2);
    }
}

#[test]
fn test_trie_put_batch_branch() {
    fn prepare_branch() -> (Trie, Trie) {
        let mut tr1 = Trie::new(EmptyNode::new(), ModeAll, newTestStore());
        let mut tr2 = Trie::new(EmptyNode::new(), ModeAll, newTestStore());
        assert!(tr1.put(&vec![0x00, 2], &b"value1".to_vec()).is_ok());
        assert!(tr2.put(&vec![0x00, 2], &b"value1".to_vec()).is_ok());
        assert!(tr1.put(&vec![0x10, 3], &b"value2".to_vec()).is_ok());
        assert!(tr2.put(&vec![0x10, 3], &b"value2".to_vec()).is_ok());
        (tr1, tr2)
    }

    #[test]
    fn simple_add() {
        let (mut tr1, mut tr2) = prepare_branch();
        let ps = vec![vec![vec![0x20, 4], b"value3".to_vec()]];
        test_put(ps, &mut tr1, &mut tr2);
    }

    #[test]
    fn remove_1_transform_to_extension() {
        let (mut tr1, mut tr2) = prepare_branch();
        let ps = vec![vec![vec![0x00, 2], vec![]]];
        test_put(ps, &mut tr1, &mut tr2);

        #[test]
        fn non_empty_child_is_hash_node() {
            let (mut tr1, mut tr2) = prepare_branch();
            tr1.flush(0);
            tr1.collapse(1);
            tr2.flush(0);
            tr2.collapse(1);

            let ps = vec![vec![vec![0x00, 2], vec![]]];
            test_put(ps, &mut tr1, &mut tr2);
            assert!(matches!(tr1.root(), Some(Node::ExtensionNode(_))));
        }

        #[test]
        fn non_empty_child_is_last_node() {
            let mut tr1 = Trie::new(EmptyNode::new(), ModeAll, newTestStore());
            let mut tr2 = Trie::new(EmptyNode::new(), ModeAll, newTestStore());
            assert!(tr1.put(&vec![0x00, 2], &b"value1".to_vec()).is_ok());
            assert!(tr2.put(&vec![0x00, 2], &b"value1".to_vec()).is_ok());
            assert!(tr1.put(&vec![0x00], &b"value2".to_vec()).is_ok());
            assert!(tr2.put(&vec![0x00], &b"value2".to_vec()).is_ok());

            tr1.flush(0);
            tr1.collapse(1);
            tr2.flush(0);
            tr2.collapse(1);

            let ps = vec![vec![vec![0x00, 2], vec![]]];
            test_put(ps, &mut tr1, &mut tr2);
        }
    }

    #[test]
    fn incomplete_put_transform_to_extension() {
        let (mut tr1, mut tr2) = prepare_branch();
        let ps = vec![
            vec![vec![0x00, 2], vec![]],
            vec![vec![0x20, 2], vec![]],
            vec![vec![0x30, 3], b"won't be put".to_vec()],
        ];
        test_incomplete_put(ps, 3, &mut tr1, &mut tr2);
    }

    #[test]
    fn incomplete_put_transform_to_empty() {
        let (mut tr1, mut tr2) = prepare_branch();
        let ps = vec![
            vec![vec![0x00, 2], vec![]],
            vec![vec![0x10, 3], vec![]],
            vec![vec![0x20, 2], vec![]],
            vec![vec![0x30, 3], b"won't be put".to_vec()],
        ];
        test_incomplete_put(ps, 4, &mut tr1, &mut tr2);
    }

    #[test]
    fn remove_2_become_empty() {
        let (mut tr1, mut tr2) = prepare_branch();
        let ps = vec![
            vec![vec![0x00, 2], vec![]],
            vec![vec![0x10, 3], vec![]],
        ];
        test_put(ps, &mut tr1, &mut tr2);
    }
}

#[test]
fn test_trie_put_batch_hash() {
    fn prepare_hash() -> (Trie, Trie) {
        let mut tr1 = Trie::new(EmptyNode::new(), ModeAll, newTestStore());
        let mut tr2 = Trie::new(EmptyNode::new(), ModeAll, newTestStore());
        assert!(tr1.put(&vec![0x10], &b"value1".to_vec()).is_ok());
        assert!(tr2.put(&vec![0x10], &b"value1".to_vec()).is_ok());
        assert!(tr1.put(&vec![0x20], &b"value2".to_vec()).is_ok());
        assert!(tr2.put(&vec![0x20], &b"value2".to_vec()).is_ok());
        tr1.flush(0);
        tr2.flush(0);
        (tr1, tr2)
    }

    #[test]
    fn good() {
        let (mut tr1, mut tr2) = prepare_hash();
        let ps = vec![vec![vec![2], b"value2".to_vec()]];
        tr1.collapse(0);
        tr1.collapse(0);
        test_put(ps, &mut tr1, &mut tr2);
    }

    #[test]
    fn incomplete_second_hash_not_found() {
        let (mut tr1, mut tr2) = prepare_hash();
        let ps = vec![
            vec![vec![0x10], b"replace1".to_vec()],
            vec![vec![0x20], b"replace2".to_vec()],
        ];
        tr1.collapse(1);
        tr2.collapse(1);
        let key = makeStorageKey(tr1.root().unwrap().as_branch().unwrap().children[2].hash());
        tr1.store().delete(&key);
        tr2.store().delete(&key);
        test_incomplete_put(ps, 1, &mut tr1, &mut tr2);
    }
}

#[test]
fn test_trie_put_batch_empty() {
    #[test]
    fn good() {
        let mut tr1 = Trie::new(EmptyNode::new(), ModeAll, newTestStore());
        let mut tr2 = Trie::new(EmptyNode::new(), ModeAll, newTestStore());
        let ps = vec![
            vec![vec![0], b"value0".to_vec()],
            vec![vec![1], b"value1".to_vec()],
            vec![vec![3], b"value3".to_vec()],
        ];
        test_put(ps, &mut tr1, &mut tr2);
    }

    #[test]
    fn incomplete() {
        let ps = vec![
            vec![vec![0], b"replace0".to_vec()],
            vec![vec![1], b"replace1".to_vec()],
            vec![vec![2], vec![]],
            vec![vec![3], b"replace3".to_vec()],
        ];
        let mut tr1 = Trie::new(EmptyNode::new(), ModeAll, newTestStore());
        let mut tr2 = Trie::new(EmptyNode::new(), ModeAll, newTestStore());
        test_incomplete_put(ps, 4, &mut tr1, &mut tr2);
    }
}

// For the sake of coverage.
#[test]
fn test_trie_invalid_node_type() {
    let mut tr = Trie::new(EmptyNode::new(), ModeAll, newTestStore());
    let b = Batch { kv: vec![keyValue {
        key: vec![0, 1],
        value: b"value".to_vec(),
    }] };
    tr.set_root(None);
    assert!(std::panic::catch_unwind(|| { tr.put_batch(b) }).is_err());
}

#[test]
fn test_trie_put_batch() {
    let mut tr1 = Trie::new(EmptyNode::new(), ModeAll, newTestStore());
    let mut tr2 = Trie::new(EmptyNode::new(), ModeAll, newTestStore());
    let ps = vec![
        vec![vec![1], vec![1]],
        vec![vec![2], vec![3]],
        vec![vec![4], vec![5]],
    ];
    test_put(ps, &mut tr1, &mut tr2);

    let ps = vec![vec![vec![4], vec![6]]];
    test_put(ps, &mut tr1, &mut tr2);

    let ps = vec![vec![vec![4], vec![]]];
    test_put(ps, &mut tr1, &mut tr2);

    test_put(vec![], &mut tr1, &mut tr2);
}

fn print_node(prefix: &str, n: &Node) {
    match n {
        Node::EmptyNode(_) => println!("{} empty", prefix),
        Node::HashNode(tn) => println!("{} {}", prefix, tn.hash().to_string_le()),
        Node::BranchNode(tn) => {
            for (i, c) in tn.children.iter().enumerate() {
                if isEmpty(c) {
                    continue;
                }
                println!("{} [{}] ->", prefix, i);
                print_node(&format!("{} ", prefix), c);
            }
        }
        Node::ExtensionNode(tn) => {
import (
	"encoding/hex"
	"fmt"
	"testing"

	"github.com/nspcc-dev/neo-go/pkg/core/storage"
	"github.com/stretchr/testify/require"
)

func TestBatchAdd(t *testing.T) {
	b := MapToMPTBatch(map[string][]byte{
		"a\x01":     {2},
		"a\x02\x10": {3},
		"a\x00\x01": {5},
		"a\x02\x00": {6},
	})

	expected := []keyValue{
		{[]byte{0, 0, 0, 1}, []byte{5}},
		{[]byte{0, 1}, []byte{2}},
		{[]byte{0, 2, 0, 0}, []byte{6}},
		{[]byte{0, 2, 1, 0}, []byte{3}},
	}
	require.Equal(t, expected, b.kv)
}

type pairs = [][2][]byte

func testIncompletePut(t *testing.T, ps pairs, n int, tr1, tr2 *Trie) {
	var m = make(map[string][]byte)
	for i, p := range ps {
		if i < n {
			if p[1] == nil {
				require.NoError(t, tr1.Delete(p[0]), "item %d", i)
			} else {
				require.NoError(t, tr1.Put(p[0], p[1]), "item %d", i)
			}
		} else if i == n {
			if p[1] == nil {
				require.Error(t, tr1.Delete(p[0]), "item %d", i)
			} else {
				require.Error(t, tr1.Put(p[0], p[1]), "item %d", i)
			}
		}
		m["a"+string(p[0])] = p[1]
	}

	b := MapToMPTBatch(m)
	num, err := tr2.PutBatch(b)
	if n == len(ps) {
		require.NoError(t, err)
	} else {
		require.Error(t, err)
	}
	require.Equal(t, n, num)
	require.Equal(t, tr1.StateRoot(), tr2.StateRoot())

	t.Run("test restore", func(t *testing.T) {
		tr2.Flush(0)
		tr3 := NewTrie(NewHashNode(tr2.StateRoot()), ModeAll, storage.NewMemCachedStore(tr2.Store))
		for _, p := range ps[:n] {
			val, err := tr3.Get(p[0])
			if p[1] == nil {
				require.Error(t, err)
				continue
			}
			require.NoError(t, err, "key: %s", hex.EncodeToString(p[0]))
			require.Equal(t, p[1], val)
		}
	})
}

func testPut(t *testing.T, ps pairs, tr1, tr2 *Trie) {
	testIncompletePut(t, ps, len(ps), tr1, tr2)
}

func TestTrie_PutBatchLeaf(t *testing.T) {
	prepareLeaf := func(t *testing.T) (*Trie, *Trie) {
		tr1 := NewTrie(EmptyNode{}, ModeAll, newTestStore())
		tr2 := NewTrie(EmptyNode{}, ModeAll, newTestStore())
		require.NoError(t, tr1.Put([]byte{0}, []byte("value")))
		require.NoError(t, tr2.Put([]byte{0}, []byte("value")))
		return tr1, tr2
	}

	t.Run("remove", func(t *testing.T) {
		tr1, tr2 := prepareLeaf(t)
		var ps = pairs{{[]byte{0}, nil}}
		testPut(t, ps, tr1, tr2)
	})
	t.Run("empty value", func(t *testing.T) {
		tr1, tr2 := prepareLeaf(t)
		var ps = pairs{{[]byte{0}, []byte{}}}
		testPut(t, ps, tr1, tr2)
	})
	t.Run("replace", func(t *testing.T) {
		tr1, tr2 := prepareLeaf(t)
		var ps = pairs{{[]byte{0}, []byte("replace")}}
		testPut(t, ps, tr1, tr2)
	})
	t.Run("remove and replace", func(t *testing.T) {
		tr1, tr2 := prepareLeaf(t)
		var ps = pairs{
			{[]byte{0}, nil},
			{[]byte{0, 2}, []byte("replace2")},
		}
		testPut(t, ps, tr1, tr2)
	})
	t.Run("empty value and replace", func(t *testing.T) {
		tr1, tr2 := prepareLeaf(t)
		var ps = pairs{
			{[]byte{0}, []byte{}},
			{[]byte{0, 2}, []byte("replace2")},
		}
		testPut(t, ps, tr1, tr2)
	})
}

func TestTrie_PutBatchExtension(t *testing.T) {
	prepareExtension := func(t *testing.T) (*Trie, *Trie) {
		tr1 := NewTrie(EmptyNode{}, ModeAll, newTestStore())
		tr2 := NewTrie(EmptyNode{}, ModeAll, newTestStore())
		require.NoError(t, tr1.Put([]byte{1, 2}, []byte("value1")))
		require.NoError(t, tr2.Put([]byte{1, 2}, []byte("value1")))
		return tr1, tr2
	}

	t.Run("split, key len > 1", func(t *testing.T) {
		tr1, tr2 := prepareExtension(t)
		var ps = pairs{{[]byte{2, 3}, []byte("value2")}}
		testPut(t, ps, tr1, tr2)
	})
	t.Run("split, key len = 1", func(t *testing.T) {
		tr1, tr2 := prepareExtension(t)
		var ps = pairs{{[]byte{1, 3}, []byte("value2")}}
		testPut(t, ps, tr1, tr2)
	})
	t.Run("add to next", func(t *testing.T) {
		tr1, tr2 := prepareExtension(t)
		var ps = pairs{{[]byte{1, 2, 3}, []byte("value2")}}
		testPut(t, ps, tr1, tr2)
	})
	t.Run("add to next with leaf", func(t *testing.T) {
		tr1, tr2 := prepareExtension(t)
		var ps = pairs{
			{[]byte{0}, []byte("value3")},
			{[]byte{1, 2, 3}, []byte("value2")},
		}
		testPut(t, ps, tr1, tr2)
	})
	t.Run("remove value", func(t *testing.T) {
		tr1, tr2 := prepareExtension(t)
		var ps = pairs{{[]byte{1, 2}, nil}}
		testPut(t, ps, tr1, tr2)
	})
	t.Run("empty value", func(t *testing.T) {
		tr1, tr2 := prepareExtension(t)
		var ps = pairs{{[]byte{1, 2}, []byte{}}}
		testPut(t, ps, tr1, tr2)
	})
	t.Run("add to next, merge extension", func(t *testing.T) {
		tr1, tr2 := prepareExtension(t)
		var ps = pairs{
			{[]byte{1, 2}, nil},
			{[]byte{1, 2, 3}, []byte("value2")},
		}
		testPut(t, ps, tr1, tr2)
	})
}

func TestTrie_PutBatchBranch(t *testing.T) {
	prepareBranch := func(t *testing.T) (*Trie, *Trie) {
		tr1 := NewTrie(EmptyNode{}, ModeAll, newTestStore())
		tr2 := NewTrie(EmptyNode{}, ModeAll, newTestStore())
		require.NoError(t, tr1.Put([]byte{0x00, 2}, []byte("value1")))
		require.NoError(t, tr2.Put([]byte{0x00, 2}, []byte("value1")))
		require.NoError(t, tr1.Put([]byte{0x10, 3}, []byte("value2")))
		require.NoError(t, tr2.Put([]byte{0x10, 3}, []byte("value2")))
		return tr1, tr2
	}

	t.Run("simple add", func(t *testing.T) {
		tr1, tr2 := prepareBranch(t)
		var ps = pairs{{[]byte{0x20, 4}, []byte("value3")}}
		testPut(t, ps, tr1, tr2)
	})
	t.Run("remove 1, transform to extension", func(t *testing.T) {
		tr1, tr2 := prepareBranch(t)
		var ps = pairs{{[]byte{0x00, 2}, nil}}
		testPut(t, ps, tr1, tr2)

		t.Run("non-empty child is hash node", func(t *testing.T) {
			tr1, tr2 := prepareBranch(t)
			tr1.Flush(0)
			tr1.Collapse(1)
			tr2.Flush(0)
			tr2.Collapse(1)

			var ps = pairs{{[]byte{0x00, 2}, nil}}
			testPut(t, ps, tr1, tr2)
			require.IsType(t, (*ExtensionNode)(nil), tr1.root)
		})
		t.Run("non-empty child is last node", func(t *testing.T) {
			tr1 := NewTrie(EmptyNode{}, ModeAll, newTestStore())
			tr2 := NewTrie(EmptyNode{}, ModeAll, newTestStore())
			require.NoError(t, tr1.Put([]byte{0x00, 2}, []byte("value1")))
			require.NoError(t, tr2.Put([]byte{0x00, 2}, []byte("value1")))
			require.NoError(t, tr1.Put([]byte{0x00}, []byte("value2")))
			require.NoError(t, tr2.Put([]byte{0x00}, []byte("value2")))

			tr1.Flush(0)
			tr1.Collapse(1)
			tr2.Flush(0)
			tr2.Collapse(1)

			var ps = pairs{{[]byte{0x00, 2}, nil}}
			testPut(t, ps, tr1, tr2)
		})
	})
	t.Run("incomplete put, transform to extension", func(t *testing.T) {
		tr1, tr2 := prepareBranch(t)
		var ps = pairs{
			{[]byte{0x00, 2}, nil},
			{[]byte{0x20, 2}, nil},
			{[]byte{0x30, 3}, []byte("won't be put")},
		}
		testIncompletePut(t, ps, 3, tr1, tr2)
	})
	t.Run("incomplete put, transform to empty", func(t *testing.T) {
		tr1, tr2 := prepareBranch(t)
		var ps = pairs{
			{[]byte{0x00, 2}, nil},
			{[]byte{0x10, 3}, nil},
			{[]byte{0x20, 2}, nil},
			{[]byte{0x30, 3}, []byte("won't be put")},
		}
		testIncompletePut(t, ps, 4, tr1, tr2)
	})
	t.Run("remove 2, become empty", func(t *testing.T) {
		tr1, tr2 := prepareBranch(t)
		var ps = pairs{
			{[]byte{0x00, 2}, nil},
			{[]byte{0x10, 3}, nil},
		}
		testPut(t, ps, tr1, tr2)
	})
}

func TestTrie_PutBatchHash(t *testing.T) {
	prepareHash := func(t *testing.T) (*Trie, *Trie) {
		tr1 := NewTrie(EmptyNode{}, ModeAll, newTestStore())
		tr2 := NewTrie(EmptyNode{}, ModeAll, newTestStore())
		require.NoError(t, tr1.Put([]byte{0x10}, []byte("value1")))
		require.NoError(t, tr2.Put([]byte{0x10}, []byte("value1")))
		require.NoError(t, tr1.Put([]byte{0x20}, []byte("value2")))
		require.NoError(t, tr2.Put([]byte{0x20}, []byte("value2")))
		tr1.Flush(0)
		tr2.Flush(0)
		return tr1, tr2
	}

	t.Run("good", func(t *testing.T) {
		tr1, tr2 := prepareHash(t)
		var ps = pairs{{[]byte{2}, []byte("value2")}}
		tr1.Collapse(0)
		tr1.Collapse(0)
		testPut(t, ps, tr1, tr2)
	})
	t.Run("incomplete, second hash not found", func(t *testing.T) {
		tr1, tr2 := prepareHash(t)
		var ps = pairs{
			{[]byte{0x10}, []byte("replace1")},
			{[]byte{0x20}, []byte("replace2")},
		}
		tr1.Collapse(1)
		tr2.Collapse(1)
		key := makeStorageKey(tr1.root.(*BranchNode).Children[2].Hash())
		tr1.Store.Delete(key)
		tr2.Store.Delete(key)
		testIncompletePut(t, ps, 1, tr1, tr2)
	})
}

func TestTrie_PutBatchEmpty(t *testing.T) {
	t.Run("good", func(t *testing.T) {
		tr1 := NewTrie(EmptyNode{}, ModeAll, newTestStore())
		tr2 := NewTrie(EmptyNode{}, ModeAll, newTestStore())
		var ps = pairs{
			{[]byte{0}, []byte("value0")},
			{[]byte{1}, []byte("value1")},
			{[]byte{3}, []byte("value3")},
		}
		testPut(t, ps, tr1, tr2)
	})
	t.Run("incomplete", func(t *testing.T) {
		var ps = pairs{
			{[]byte{0}, []byte("replace0")},
			{[]byte{1}, []byte("replace1")},
			{[]byte{2}, nil},
			{[]byte{3}, []byte("replace3")},
		}
		tr1 := NewTrie(EmptyNode{}, ModeAll, newTestStore())
		tr2 := NewTrie(EmptyNode{}, ModeAll, newTestStore())
		testIncompletePut(t, ps, 4, tr1, tr2)
	})
}

// For the sake of coverage.
func TestTrie_InvalidNodeType(t *testing.T) {
	tr := NewTrie(EmptyNode{}, ModeAll, newTestStore())
	var b = Batch{kv: []keyValue{{
		key:   []byte{0, 1},
		value: []byte("value"),
	}}}
	tr.root = Node(nil)
	require.Panics(t, func() { _, _ = tr.PutBatch(b) })
}

func TestTrie_PutBatch(t *testing.T) {
	tr1 := NewTrie(EmptyNode{}, ModeAll, newTestStore())
	tr2 := NewTrie(EmptyNode{}, ModeAll, newTestStore())
	var ps = pairs{
		{[]byte{1}, []byte{1}},
		{[]byte{2}, []byte{3}},
		{[]byte{4}, []byte{5}},
	}
	testPut(t, ps, tr1, tr2)

	ps = pairs{[2][]byte{{4}, {6}}}
	testPut(t, ps, tr1, tr2)

	ps = pairs{[2][]byte{{4}, nil}}
	testPut(t, ps, tr1, tr2)

	testPut(t, pairs{}, tr1, tr2)
}

var _ = printNode

// This function is unused, but is helpful for debugging
// as it provides more readable Trie representation compared to
// `spew.Dump()`.
func printNode(prefix string, n Node) {
	switch tn := n.(type) {
	case EmptyNode:
		fmt.Printf("%s empty\n", prefix)
		return
	case *HashNode:
		fmt.Printf("%s %s\n", prefix, tn.Hash().StringLE())
	case *BranchNode:
		for i, c := range tn.Children {
			if isEmpty(c) {
				continue
			}
			fmt.Printf("%s [%2d] ->\n", prefix, i)
			printNode(prefix+" ", c)
		}
	case *ExtensionNode:
		fmt.Printf("%s extension-> %s\n", prefix, hex.EncodeToString(tn.key))
		printNode(prefix+" ", tn.next)
	case *LeafNode:
		fmt.Printf("%s leaf-> %s\n", prefix, hex.EncodeToString(tn.value))
	}
}
