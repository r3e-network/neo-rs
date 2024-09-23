use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use crate::mpt::{Trie, Node, BranchNode, ExtensionNode, LeafNode, HashNode, Mode, new_test_store, MaxKeyLength, MaxValueLength};
use crate::mpt::verify_proof::VerifyProof;
use crate::mpt::proof::GetProof;
use crate::mpt::find::Find;
use crate::mpt::put::Put;
use crate::mpt::delete::Delete;
use crate::mpt::flush::Flush;
use crate::mpt::copy::CopyTrie;
use crate::mpt::check_batch_size::CheckBatchSize;
use crate::mpt::new_filled_trie::NewFilledTrie;
use crate::mpt::test_get_proof::TestGetProof;

fn prepare_mpt_compat() -> Trie {
    let b = BranchNode::new();
    let r = ExtensionNode::new(vec![0x0a, 0x0c], b.clone());
    let v1 = LeafNode::new(vec![0xab, 0xcd]); // key=ac01
    let v2 = LeafNode::new(vec![0x22, 0x22]); // key=ac
    let v3 = LeafNode::new(b"existing".to_vec()); // key=acae
    let v4 = LeafNode::new(b"missing".to_vec());
    let h3 = HashNode::new(v3.hash());
    let e1 = ExtensionNode::new(vec![0x01], v1.clone());
    let e3 = ExtensionNode::new(vec![0x0e], h3.clone());
    let e4 = ExtensionNode::new(vec![0x01], v4.clone());
    b.children[0] = Some(e1.clone());
    b.children[10] = Some(e3.clone());
    b.children[16] = Some(v2.clone());
    b.children[15] = Some(HashNode::new(e4.hash()));

    let mut tr = Trie::new(r.clone(), Mode::Latest, new_test_store());
    tr.put_to_store(r);
    tr.put_to_store(b);
    tr.put_to_store(e1);
    tr.put_to_store(e3);
    tr.put_to_store(v1);
    tr.put_to_store(v2);
    tr.put_to_store(v3);

    tr
}

#[test]
fn test_compatibility() {
    let main_trie = prepare_mpt_compat();

    #[test]
    fn try_get() {
        let tr = main_trie.copy();
        tr.test_has(vec![0xac, 0x01], vec![0xab, 0xcd]);
        tr.test_has(vec![0xac], vec![0x22, 0x22]);
        tr.test_has(vec![0xab, 0x99], None);
        tr.test_has(vec![0xac, 0x39], None);
        tr.test_has(vec![0xac, 0x02], None);
        tr.test_has(vec![0xac, 0x01, 0x00], None);
        tr.test_has(vec![0xac, 0x99, 0x10], None);
        tr.test_has(vec![0xac, 0xf1], None);
        tr.test_has(vec![0; MaxKeyLength], None);
    }

    #[test]
    fn try_get_resolve() {
        let tr = main_trie.copy();
        tr.test_has(vec![0xac, 0xae], b"existing".to_vec());
    }

    #[test]
    fn try_put() {
        let tr = new_filled_trie(
            vec![0xac, 0x01], vec![0xab, 0xcd],
            vec![0xac], vec![0x22, 0x22],
            vec![0xac, 0xae], b"existing".to_vec(),
            vec![0xac, 0xf1], b"missing".to_vec()
        );

        assert_eq!(main_trie.root.hash(), tr.root.hash());
        assert!(tr.put(None, vec![0x01]).is_err());
        assert!(tr.put(vec![0x01], None).is_err());
        assert!(tr.put(vec![0; MaxKeyLength + 1], None).is_err());
        assert!(tr.put(vec![0x01], vec![0; MaxValueLength + 1]).is_err());
        assert_eq!(main_trie.root.hash(), tr.root.hash());
        assert!(tr.put(vec![0x01], vec![]).is_ok());
        assert!(tr.put(vec![0xac, 0x01], vec![0xab]).is_ok());
    }

    #[test]
    fn put_cant_resolve() {
        let tr = main_trie.copy();
        assert!(tr.put(vec![0xac, 0xf1, 0x11], vec![1]).is_err());
    }

    #[test]
    fn try_delete() {
        let tr = main_trie.copy();
        tr.test_has(vec![0xac], vec![0x22, 0x22]);
        assert!(tr.delete(vec![0x0c, 0x99]).is_ok());
        assert!(tr.delete(None).is_ok());
        assert!(tr.delete(vec![0xac, 0x20]).is_ok());

        assert!(tr.delete(vec![0xac, 0xf1]).is_err()); // error for can't resolve
        assert!(tr.delete(vec![0; MaxKeyLength + 1]).is_err()); // error for too big key

        // In our implementation missing keys are ignored.
        assert!(tr.delete(vec![0xac]).is_ok());
        assert!(tr.delete(vec![0xac, 0xae, 0x01]).is_ok());
        assert!(tr.delete(vec![0xac, 0xae]).is_ok());

        assert_eq!("cb06925428b7c727375c7fdd943a302fe2c818cf2e2eaf63a7932e3fd6cb3408",
            tr.root.hash().to_string());
    }

    #[test]
    fn delete_remain_can_resolve() {
        let tr = new_filled_trie(
            vec![0xac, 0x00], vec![0xab, 0xcd],
            vec![0xac, 0x10], vec![0xab, 0xcd]
        );
        tr.flush(0);

        let tr2 = tr.copy();
        assert!(tr2.delete(vec![0xac, 0x00]).is_ok());

        tr2.flush(0);
        assert!(tr2.delete(vec![0xac, 0x10]).is_ok());
    }

    #[test]
    fn delete_remain_cant_resolve() {
        let b = BranchNode::new();
        let r = ExtensionNode::new(vec![0x0a, 0x0c], b.clone());
        let v1 = LeafNode::new(vec![0xab, 0xcd]);
        let v4 = LeafNode::new(b"missing".to_vec());
        let e1 = ExtensionNode::new(vec![0x01], v1.clone());
        let e4 = ExtensionNode::new(vec![0x01], v4.clone());
        b.children[0] = Some(e1.clone());
        b.children[15] = Some(HashNode::new(e4.hash()));

        let mut tr = Trie::new(HashNode::new(r.hash()), Mode::All, new_test_store());
        tr.put_to_store(r);
        tr.put_to_store(b);
        tr.put_to_store(e1);
        tr.put_to_store(v1);

        assert!(tr.delete(vec![0xac, 0x01]).is_err());
    }

    #[test]
    fn delete_same_value() {
        let tr = new_filled_trie(
            vec![0xac, 0x01], vec![0xab, 0xcd],
            vec![0xac, 0x02], vec![0xab, 0xcd]
        );
        tr.test_has(vec![0xac, 0x01], vec![0xab, 0xcd]);
        tr.test_has(vec![0xac, 0x02], vec![0xab, 0xcd]);

        assert!(tr.delete(vec![0xac, 0x01]).is_ok());
        tr.test_has(vec![0xac, 0x02], vec![0xab, 0xcd]);
        tr.flush(0);

        let tr2 = Trie::new(HashNode::new(tr.root.hash()), Mode::All, tr.store.clone());
        tr2.test_has(vec![0xac, 0x02], vec![0xab, 0xcd]);
    }

    #[test]
    fn branch_node_remain_value() {
        let tr = new_filled_trie(
            vec![0xac, 0x11], vec![0xac, 0x11],
            vec![0xac, 0x22], vec![0xac, 0x22],
            vec![0xac], vec![0xac]
        );
        tr.flush(0);
        check_batch_size(tr, 7);

        assert!(tr.delete(vec![0xac, 0x11]).is_ok());
        tr.flush(0);
        check_batch_size(tr, 5);

        assert!(tr.delete(vec![0xac, 0x22]).is_ok());
        tr.flush(0);
        check_batch_size(tr, 2);
    }

    #[test]
    fn get_proof() {
        let b = BranchNode::new();
        let r = ExtensionNode::new(vec![0x0a, 0x0c], b.clone());
        let v1 = LeafNode::new(vec![0xab, 0xcd]); // key=ac01
        let v2 = LeafNode::new(vec![0x22, 0x22]); // key=ac
        let v3 = LeafNode::new(b"existing".to_vec()); // key=acae
        let v4 = LeafNode::new(b"missing".to_vec());
        let h3 = HashNode::new(v3.hash());
        let e1 = ExtensionNode::new(vec![0x01], v1.clone());
        let e3 = ExtensionNode::new(vec![0x0e], h3.clone());
        let e4 = ExtensionNode::new(vec![0x01], v4.clone());
        b.children[0] = Some(e1.clone());
        b.children[10] = Some(e3.clone());
        b.children[16] = Some(v2.clone());
        b.children[15] = Some(HashNode::new(e4.hash()));

        let tr = Trie::new(HashNode::new(r.hash()), Mode::Latest, main_trie.store.clone());
        assert_eq!(r.hash(), tr.root.hash());

        let proof = test_get_proof(tr, vec![0xac, 0x01], 4);
        assert_eq!(r.bytes(), proof[0]);
        assert_eq!(b.bytes(), proof[1]);
        assert_eq!(e1.bytes(), proof[2]);
        assert_eq!(v1.bytes(), proof[3]);

        test_get_proof(tr, vec![0xac], 3);
        test_get_proof(tr, vec![0xac, 0x10], 0);
        test_get_proof(tr, vec![0xac, 0xae], 4);
        test_get_proof(tr, None, 0);
        test_get_proof(tr, vec![0xac, 0x01, 0x00], 0);
        test_get_proof(tr, vec![0xac, 0xf1], 0);
        test_get_proof(tr, vec![0; MaxKeyLength], 0);
    }

    #[test]
    fn verify_proof() {
        let tr = main_trie.copy();
        let proof = test_get_proof(tr, vec![0xac, 0x01], 4);
        let (value, ok) = verify_proof(tr.root.hash(), vec![0xac, 0x01], proof);
        assert!(ok);
        assert_eq!(vec![0xab, 0xcd], value);
    }

    #[test]
    fn add_longer_key() {
        let tr = new_filled_trie(
            vec![0xab], vec![0x01],
            vec![0xab, 0xcd], vec![0x02]
        );
        tr.test_has(vec![0xab], vec![0x01]);
    }

    #[test]
    fn split_key() {
        let tr = new_filled_trie(
            vec![0xab, 0xcd], vec![0x01],
            vec![0xab], vec![0x02]
        );
        test_get_proof(tr, vec![0xab, 0xcd], 4);

        let tr2 = new_filled_trie(
            vec![0xab], vec![0x02],
            vec![0xab, 0xcd], vec![0x01]
        );
        test_get_proof(tr, vec![0xab, 0xcd], 4);

        assert_eq!(tr.root.hash(), tr2.root.hash());
    }

    #[test]
    fn reference() {
        let tr = new_filled_trie(
            vec![0xa1, 0x01], vec![0x01],
            vec![0xa2, 0x01], vec![0x01],
            vec![0xa3, 0x01], vec![0x01]
        );
        tr.flush(0);

        let tr2 = tr.copy();
        assert!(tr2.delete(vec![0xa3, 0x01]).is_ok());
        tr2.flush(0);

        let tr3 = tr2.copy();
        assert!(tr3.delete(vec![0xa2, 0x01]).is_ok());
        tr3.test_has(vec![0xa1, 0x01], vec![0x01]);
    }

    #[test]
    fn reference2() {
        let tr = new_filled_trie(
            vec![0xa1, 0x01], vec![0x01],
            vec![0xa2, 0x01], vec![0x01],
            vec![0xa3, 0x01], vec![0x01]
        );
        tr.flush(0);
        check_batch_size(tr, 4);

        assert!(tr.delete(vec![0xa3, 0x01]).is_ok());
        tr.flush(0);
        check_batch_size(tr, 4);

        assert!(tr.delete(vec![0xa2, 0x01]).is_ok());
        tr.flush(0);
        check_batch_size(tr, 2);
        tr.test_has(vec![0xa1, 0x01], vec![0x01]);
    }

    #[test]
    fn extension_delete_dirty() {
        let tr = new_filled_trie(
            vec![0xa1], vec![0x01],
            vec![0xa2], vec![0x02]
        );
        tr.flush(0);
        check_batch_size(tr, 4);

        let tr1 = tr.copy();
        assert!(tr1.delete(vec![0xa1]).is_ok());
        tr1.flush(0);
        assert_eq!(2, tr1.store.get_batch().put.len());

        let tr2 = tr1.copy();
        assert!(tr2.delete(vec![0xa2]).is_ok());
        tr2.flush(0);
        assert_eq!(0, tr2.store.get_batch().put.len());
    }

    #[test]
    fn branch_delete_dirty() {
        let tr = new_filled_trie(
            vec![0x10], vec![0x01],
            vec![0x20], vec![0x02],
            vec![0x30], vec![0x03]
        );
        tr.flush(0);
        check_batch_size(tr, 7);

        let tr1 = tr.copy();
        assert!(tr1.delete(vec![0x10]).is_ok());
        tr1.flush(0);

        let tr2 = tr1.copy();
        assert!(tr2.delete(vec![0x20]).is_ok());
        tr2.flush(0);
        assert_eq!(2, tr2.store.get_batch().put.len());

        let tr3 = tr2.copy();
        assert!(tr3.delete(vec![0x30]).is_ok());
        tr3.flush(0);
        assert_eq!(0, tr3.store.get_batch().put.len());
    }

    #[test]
    fn extension_put_dirty() {
        let tr = new_filled_trie(
            vec![0xa1], vec![0x01],
            vec![0xa2], vec![0x02]
        );
        tr.flush(0);
        check_batch_size(tr, 4);

        let tr1 = tr.copy();
        assert!(tr1.put(vec![0xa3], vec![0x03]).is_ok());
        tr1.flush(0);
        assert_eq!(5, tr1.store.get_batch().put.len());
    }

    #[test]
    fn branch_put_dirty() {
        let tr = new_filled_trie(
            vec![0x10], vec![0x01],
            vec![0x20], vec![0x02]
        );
        tr.flush(0);
        check_batch_size(tr, 5);

        let tr1 = tr.copy();
        assert!(tr1.put(vec![0x30], vec![0x03]).is_ok());
        tr1.flush(0);
        check_batch_size(tr1, 7);
    }

    #[test]
    fn empty_value_issue633() {
        let tr = new_filled_trie(
            vec![0x01], vec![]
        );
        tr.flush(0);
        check_batch_size(tr, 2);

        let proof = test_get_proof(tr, vec![0x01], 2);
        let (value, ok) = verify_proof(tr.root.hash(), vec![0x01], proof);
        assert!(ok);
        assert_eq!(vec![], value);
    }
}

#[test]
fn test_compatibility_find() {
    fn check(from: Option<Vec<u8>>, expected_res_len: usize) {
        let tr = Trie::new(None, Mode::All, new_test_store());
        assert!(tr.put(b"aa".to_vec(), b"02".to_vec()).is_ok());
        assert!(tr.put(b"aa10".to_vec(), b"03".to_vec()).is_ok());
        assert!(tr.put(b"aa50".to_vec(), b"04".to_vec()).is_ok());
        let res = tr.find(b"aa".to_vec(), from, 10).unwrap();
func prepareMPTCompat() *Trie {
	b := NewBranchNode()
	r := NewExtensionNode([]byte{0x0a, 0x0c}, b)
	v1 := NewLeafNode([]byte{0xab, 0xcd}) //key=ac01
	v2 := NewLeafNode([]byte{0x22, 0x22}) //key=ac
	v3 := NewLeafNode([]byte("existing")) //key=acae
	v4 := NewLeafNode([]byte("missing"))
	h3 := NewHashNode(v3.Hash())
	e1 := NewExtensionNode([]byte{0x01}, v1)
	e3 := NewExtensionNode([]byte{0x0e}, h3)
	e4 := NewExtensionNode([]byte{0x01}, v4)
	b.Children[0] = e1
	b.Children[10] = e3
	b.Children[16] = v2
	b.Children[15] = NewHashNode(e4.Hash())

	tr := NewTrie(r, ModeLatest, newTestStore())
	tr.putToStore(r)
	tr.putToStore(b)
	tr.putToStore(e1)
	tr.putToStore(e3)
	tr.putToStore(v1)
	tr.putToStore(v2)
	tr.putToStore(v3)

	return tr
}

// TestCompatibility contains tests present in C# implementation.
// https://github.com/neo-project/neo-modules/blob/master/tests/Neo.Plugins.StateService.Tests/MPT/UT_MPTTrie.cs
// There are some differences, though:
//  1. In our implementation, delete is silent, i.e. we do not return an error if the key is missing or empty.
//     However, we do return an error when the contents of the hash node are missing from the store
//     (corresponds to exception in C# implementation). However, if the key is too big, an error is returned
//     (corresponds to exception in C# implementation).
//  2. In our implementation, put returns an error if something goes wrong, while C# implementation throws
//     an exception and returns nothing.
//  3. In our implementation, get does not immediately return any error in case of an empty key. An error is returned
//     only if the value is missing from the storage. C# implementation checks that the key is not empty and throws an error
//     otherwise. However, if the key is too big, an error is returned (corresponds to exception in C# implementation).
func TestCompatibility(t *testing.T) {
	mainTrie := prepareMPTCompat()

	t.Run("TryGet", func(t *testing.T) {
		tr := copyTrie(mainTrie)
		tr.testHas(t, []byte{0xac, 0x01}, []byte{0xab, 0xcd})
		tr.testHas(t, []byte{0xac}, []byte{0x22, 0x22})
		tr.testHas(t, []byte{0xab, 0x99}, nil)
		tr.testHas(t, []byte{0xac, 0x39}, nil)
		tr.testHas(t, []byte{0xac, 0x02}, nil)
		tr.testHas(t, []byte{0xac, 0x01, 0x00}, nil)
		tr.testHas(t, []byte{0xac, 0x99, 0x10}, nil)
		tr.testHas(t, []byte{0xac, 0xf1}, nil)
		tr.testHas(t, make([]byte, MaxKeyLength), nil)
	})

	t.Run("TryGetResolve", func(t *testing.T) {
		tr := copyTrie(mainTrie)
		tr.testHas(t, []byte{0xac, 0xae}, []byte("existing"))
	})

	t.Run("TryPut", func(t *testing.T) {
		tr := newFilledTrie(t,
			[]byte{0xac, 0x01}, []byte{0xab, 0xcd},
			[]byte{0xac}, []byte{0x22, 0x22},
			[]byte{0xac, 0xae}, []byte("existing"),
			[]byte{0xac, 0xf1}, []byte("missing"))

		require.Equal(t, mainTrie.root.Hash(), tr.root.Hash())
		require.Error(t, tr.Put(nil, []byte{0x01}))
		require.Error(t, tr.Put([]byte{0x01}, nil))
		require.Error(t, tr.Put(make([]byte, MaxKeyLength+1), nil))
		require.Error(t, tr.Put([]byte{0x01}, make([]byte, MaxValueLength+1)))
		require.Equal(t, mainTrie.root.Hash(), tr.root.Hash())
		require.NoError(t, tr.Put([]byte{0x01}, []byte{}))
		require.NoError(t, tr.Put([]byte{0xac, 0x01}, []byte{0xab}))
	})

	t.Run("PutCantResolve", func(t *testing.T) {
		tr := copyTrie(mainTrie)
		require.Error(t, tr.Put([]byte{0xac, 0xf1, 0x11}, []byte{1}))
	})

	t.Run("TryDelete", func(t *testing.T) {
		tr := copyTrie(mainTrie)
		tr.testHas(t, []byte{0xac}, []byte{0x22, 0x22})
		require.NoError(t, tr.Delete([]byte{0x0c, 0x99}))
		require.NoError(t, tr.Delete(nil))
		require.NoError(t, tr.Delete([]byte{0xac, 0x20}))

		require.Error(t, tr.Delete([]byte{0xac, 0xf1}))           // error for can't resolve
		require.Error(t, tr.Delete(make([]byte, MaxKeyLength+1))) // error for too big key

		// In our implementation missing keys are ignored.
		require.NoError(t, tr.Delete([]byte{0xac}))
		require.NoError(t, tr.Delete([]byte{0xac, 0xae, 0x01}))
		require.NoError(t, tr.Delete([]byte{0xac, 0xae}))

		require.Equal(t, "cb06925428b7c727375c7fdd943a302fe2c818cf2e2eaf63a7932e3fd6cb3408",
			tr.root.Hash().StringLE())
	})

	t.Run("DeleteRemainCanResolve", func(t *testing.T) {
		tr := newFilledTrie(t,
			[]byte{0xac, 0x00}, []byte{0xab, 0xcd},
			[]byte{0xac, 0x10}, []byte{0xab, 0xcd})
		tr.Flush(0)

		tr2 := copyTrie(tr)
		require.NoError(t, tr2.Delete([]byte{0xac, 0x00}))

		tr2.Flush(0)
		require.NoError(t, tr2.Delete([]byte{0xac, 0x10}))
	})

	t.Run("DeleteRemainCantResolve", func(t *testing.T) {
		b := NewBranchNode()
		r := NewExtensionNode([]byte{0x0a, 0x0c}, b)
		v1 := NewLeafNode([]byte{0xab, 0xcd})
		v4 := NewLeafNode([]byte("missing"))
		e1 := NewExtensionNode([]byte{0x01}, v1)
		e4 := NewExtensionNode([]byte{0x01}, v4)
		b.Children[0] = e1
		b.Children[15] = NewHashNode(e4.Hash())

		tr := NewTrie(NewHashNode(r.Hash()), ModeAll, newTestStore())
		tr.putToStore(r)
		tr.putToStore(b)
		tr.putToStore(e1)
		tr.putToStore(v1)

		require.Error(t, tr.Delete([]byte{0xac, 0x01}))
	})

	t.Run("DeleteSameValue", func(t *testing.T) {
		tr := newFilledTrie(t,
			[]byte{0xac, 0x01}, []byte{0xab, 0xcd},
			[]byte{0xac, 0x02}, []byte{0xab, 0xcd})
		tr.testHas(t, []byte{0xac, 0x01}, []byte{0xab, 0xcd})
		tr.testHas(t, []byte{0xac, 0x02}, []byte{0xab, 0xcd})

		require.NoError(t, tr.Delete([]byte{0xac, 0x01}))
		tr.testHas(t, []byte{0xac, 0x02}, []byte{0xab, 0xcd})
		tr.Flush(0)

		tr2 := NewTrie(NewHashNode(tr.root.Hash()), ModeAll, tr.Store)
		tr2.testHas(t, []byte{0xac, 0x02}, []byte{0xab, 0xcd})
	})

	t.Run("BranchNodeRemainValue", func(t *testing.T) {
		tr := newFilledTrie(t,
			[]byte{0xac, 0x11}, []byte{0xac, 0x11},
			[]byte{0xac, 0x22}, []byte{0xac, 0x22},
			[]byte{0xac}, []byte{0xac})
		tr.Flush(0)
		checkBatchSize(t, tr, 7)

		require.NoError(t, tr.Delete([]byte{0xac, 0x11}))
		tr.Flush(0)
		checkBatchSize(t, tr, 5)

		require.NoError(t, tr.Delete([]byte{0xac, 0x22}))
		tr.Flush(0)
		checkBatchSize(t, tr, 2)
	})

	t.Run("GetProof", func(t *testing.T) {
		b := NewBranchNode()
		r := NewExtensionNode([]byte{0x0a, 0x0c}, b)
		v1 := NewLeafNode([]byte{0xab, 0xcd}) //key=ac01
		v2 := NewLeafNode([]byte{0x22, 0x22}) //key=ac
		v3 := NewLeafNode([]byte("existing")) //key=acae
		v4 := NewLeafNode([]byte("missing"))
		h3 := NewHashNode(v3.Hash())
		e1 := NewExtensionNode([]byte{0x01}, v1)
		e3 := NewExtensionNode([]byte{0x0e}, h3)
		e4 := NewExtensionNode([]byte{0x01}, v4)
		b.Children[0] = e1
		b.Children[10] = e3
		b.Children[16] = v2
		b.Children[15] = NewHashNode(e4.Hash())

		tr := NewTrie(NewHashNode(r.Hash()), ModeLatest, mainTrie.Store)
		require.Equal(t, r.Hash(), tr.root.Hash())

		proof := testGetProof(t, tr, []byte{0xac, 0x01}, 4)
		require.Equal(t, r.Bytes(), proof[0])
		require.Equal(t, b.Bytes(), proof[1])
		require.Equal(t, e1.Bytes(), proof[2])
		require.Equal(t, v1.Bytes(), proof[3])

		testGetProof(t, tr, []byte{0xac}, 3)
		testGetProof(t, tr, []byte{0xac, 0x10}, 0)
		testGetProof(t, tr, []byte{0xac, 0xae}, 4)
		testGetProof(t, tr, nil, 0)
		testGetProof(t, tr, []byte{0xac, 0x01, 0x00}, 0)
		testGetProof(t, tr, []byte{0xac, 0xf1}, 0)
		testGetProof(t, tr, make([]byte, MaxKeyLength), 0)
	})

	t.Run("VerifyProof", func(t *testing.T) {
		tr := copyTrie(mainTrie)
		proof := testGetProof(t, tr, []byte{0xac, 0x01}, 4)
		value, ok := VerifyProof(tr.root.Hash(), []byte{0xac, 0x01}, proof)
		require.True(t, ok)
		require.Equal(t, []byte{0xab, 0xcd}, value)
	})

	t.Run("AddLongerKey", func(t *testing.T) {
		tr := newFilledTrie(t,
			[]byte{0xab}, []byte{0x01},
			[]byte{0xab, 0xcd}, []byte{0x02})
		tr.testHas(t, []byte{0xab}, []byte{0x01})
	})

	t.Run("SplitKey", func(t *testing.T) {
		tr := newFilledTrie(t,
			[]byte{0xab, 0xcd}, []byte{0x01},
			[]byte{0xab}, []byte{0x02})
		testGetProof(t, tr, []byte{0xab, 0xcd}, 4)

		tr2 := newFilledTrie(t,
			[]byte{0xab}, []byte{0x02},
			[]byte{0xab, 0xcd}, []byte{0x01})
		testGetProof(t, tr, []byte{0xab, 0xcd}, 4)

		require.Equal(t, tr.root.Hash(), tr2.root.Hash())
	})

	t.Run("Reference", func(t *testing.T) {
		tr := newFilledTrie(t,
			[]byte{0xa1, 0x01}, []byte{0x01},
			[]byte{0xa2, 0x01}, []byte{0x01},
			[]byte{0xa3, 0x01}, []byte{0x01})
		tr.Flush(0)

		tr2 := copyTrie(tr)
		require.NoError(t, tr2.Delete([]byte{0xa3, 0x01}))
		tr2.Flush(0)

		tr3 := copyTrie(tr2)
		require.NoError(t, tr3.Delete([]byte{0xa2, 0x01}))
		tr3.testHas(t, []byte{0xa1, 0x01}, []byte{0x01})
	})

	t.Run("Reference2", func(t *testing.T) {
		tr := newFilledTrie(t,
			[]byte{0xa1, 0x01}, []byte{0x01},
			[]byte{0xa2, 0x01}, []byte{0x01},
			[]byte{0xa3, 0x01}, []byte{0x01})
		tr.Flush(0)
		checkBatchSize(t, tr, 4)

		require.NoError(t, tr.Delete([]byte{0xa3, 0x01}))
		tr.Flush(0)
		checkBatchSize(t, tr, 4)

		require.NoError(t, tr.Delete([]byte{0xa2, 0x01}))
		tr.Flush(0)
		checkBatchSize(t, tr, 2)
		tr.testHas(t, []byte{0xa1, 0x01}, []byte{0x01})
	})

	t.Run("ExtensionDeleteDirty", func(t *testing.T) {
		tr := newFilledTrie(t,
			[]byte{0xa1}, []byte{0x01},
			[]byte{0xa2}, []byte{0x02})
		tr.Flush(0)
		checkBatchSize(t, tr, 4)

		tr1 := copyTrie(tr)
		require.NoError(t, tr1.Delete([]byte{0xa1}))
		tr1.Flush(0)
		require.Equal(t, 2, len(tr1.Store.GetBatch().Put))

		tr2 := copyTrie(tr1)
		require.NoError(t, tr2.Delete([]byte{0xa2}))
		tr2.Flush(0)
		require.Equal(t, 0, len(tr2.Store.GetBatch().Put))
	})

	t.Run("BranchDeleteDirty", func(t *testing.T) {
		tr := newFilledTrie(t,
			[]byte{0x10}, []byte{0x01},
			[]byte{0x20}, []byte{0x02},
			[]byte{0x30}, []byte{0x03})
		tr.Flush(0)
		checkBatchSize(t, tr, 7)

		tr1 := copyTrie(tr)
		require.NoError(t, tr1.Delete([]byte{0x10}))
		tr1.Flush(0)

		tr2 := copyTrie(tr1)
		require.NoError(t, tr2.Delete([]byte{0x20}))
		tr2.Flush(0)
		require.Equal(t, 2, len(tr2.Store.GetBatch().Put))

		tr3 := copyTrie(tr2)
		require.NoError(t, tr3.Delete([]byte{0x30}))
		tr3.Flush(0)
		require.Equal(t, 0, len(tr3.Store.GetBatch().Put))
	})

	t.Run("ExtensionPutDirty", func(t *testing.T) {
		tr := newFilledTrie(t,
			[]byte{0xa1}, []byte{0x01},
			[]byte{0xa2}, []byte{0x02})
		tr.Flush(0)
		checkBatchSize(t, tr, 4)

		tr1 := copyTrie(tr)
		require.NoError(t, tr1.Put([]byte{0xa3}, []byte{0x03}))
		tr1.Flush(0)
		require.Equal(t, 5, len(tr1.Store.GetBatch().Put))
	})

	t.Run("BranchPutDirty", func(t *testing.T) {
		tr := newFilledTrie(t,
			[]byte{0x10}, []byte{0x01},
			[]byte{0x20}, []byte{0x02})
		tr.Flush(0)
		checkBatchSize(t, tr, 5)

		tr1 := copyTrie(tr)
		require.NoError(t, tr1.Put([]byte{0x30}, []byte{0x03}))
		tr1.Flush(0)
		checkBatchSize(t, tr1, 7)
	})

	t.Run("EmptyValueIssue633", func(t *testing.T) {
		tr := newFilledTrie(t,
			[]byte{0x01}, []byte{})
		tr.Flush(0)
		checkBatchSize(t, tr, 2)

		proof := testGetProof(t, tr, []byte{0x01}, 2)
		value, ok := VerifyProof(tr.root.Hash(), []byte{0x01}, proof)
		require.True(t, ok)
		require.Equal(t, []byte{}, value)
	})
}

func copyTrie(t *Trie) *Trie {
	return NewTrie(NewHashNode(t.root.Hash()), t.mode, t.Store)
}

func checkBatchSize(t *testing.T, tr *Trie, n int) {
	require.Equal(t, n, len(tr.Store.GetBatch().Put))
}

func testGetProof(t *testing.T, tr *Trie, key []byte, size int) [][]byte {
	proof, err := tr.GetProof(key)
	if size == 0 {
		require.Error(t, err)
		return proof
	}

	require.NoError(t, err)
	require.Equal(t, size, len(proof))
	return proof
}

func newFilledTrie(t *testing.T, args ...[]byte) *Trie {
	tr := NewTrie(nil, ModeLatest, newTestStore())
	for i := 0; i < len(args); i += 2 {
		require.NoError(t, tr.Put(args[i], args[i+1]))
	}
	return tr
}

func TestCompatibility_Find(t *testing.T) {
	check := func(t *testing.T, from []byte, expectedResLen int) {
		tr := NewTrie(nil, ModeAll, newTestStore())
		require.NoError(t, tr.Put([]byte("aa"), []byte("02")))
		require.NoError(t, tr.Put([]byte("aa10"), []byte("03")))
		require.NoError(t, tr.Put([]byte("aa50"), []byte("04")))
		res, err := tr.Find([]byte("aa"), from, 10)
		require.NoError(t, err)
		require.Equal(t, expectedResLen, len(res))
	}
	t.Run("no from", func(t *testing.T) {
		check(t, nil, 3)
	})
	t.Run("from is not in tree", func(t *testing.T) {
		t.Run("matching", func(t *testing.T) {
			check(t, []byte("30"), 1)
		})
		t.Run("non-matching", func(t *testing.T) {
			check(t, []byte("60"), 0)
		})
	})
	t.Run("from is in tree", func(t *testing.T) {
		check(t, []byte("10"), 1) // without `from` key
	})
	t.Run("from matching start", func(t *testing.T) {
		check(t, []byte{}, 2) // without `from` key
	})
	t.Run("TestFindStatesIssue652", func(t *testing.T) {
		tr := NewTrie(nil, ModeAll, newTestStore())
		// root is an extension node with key=abc; next=branch
		require.NoError(t, tr.Put([]byte("abc1"), []byte("01")))
		require.NoError(t, tr.Put([]byte("abc3"), []byte("02")))
		tr.Flush(0)
		// find items with extension's key prefix
		t.Run("from > start", func(t *testing.T) {
			res, err := tr.Find([]byte("ab"), []byte("d2"), 100)
			require.NoError(t, err)
			// nothing should be found, because from[0]=`d` > key[2]=`c`
			require.Equal(t, 0, len(res))
		})

		t.Run("from < start", func(t *testing.T) {
			res, err := tr.Find([]byte("ab"), []byte("b2"), 100)
			require.NoError(t, err)
			// all items should be included into the result, because from[0]=`b` < key[2]=`c`
			require.Equal(t, 2, len(res))
		})

		t.Run("from and start have common prefix", func(t *testing.T) {
			res, err := tr.Find([]byte("ab"), []byte("c"), 100)
			require.NoError(t, err)
			// all items should be included into the result, because from[0] == key[2]
			require.Equal(t, 2, len(res))
		})

		t.Run("from equals to item key", func(t *testing.T) {
			res, err := tr.Find([]byte("ab"), []byte("c1"), 100)
			require.NoError(t, err)
			require.Equal(t, 1, len(res))
		})
	})
}
