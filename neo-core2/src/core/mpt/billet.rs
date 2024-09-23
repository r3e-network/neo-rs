use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::error::Error;
use std::fmt;

use crate::core::storage::{KeyPrefix, MemCachedStore};
use crate::core::util::Uint256;
use crate::core::io;
use crate::core::mpt::{Node, HashNode, LeafNode, BranchNode, ExtensionNode, EmptyNode, TrieMode, NodeObject, flushedNode};

#[derive(Debug, Clone)]
pub struct RestoreFailedError;

impl fmt::Display for RestoreFailedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "failed to restore MPT node")
    }
}

impl Error for RestoreFailedError {}

#[derive(Debug, Clone)]
pub struct StopError;

impl fmt::Display for StopError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "stop condition is met")
    }
}

impl Error for StopError {}

pub struct Billet {
    temp_storage_prefix: KeyPrefix,
    store: Arc<Mutex<MemCachedStore>>,
    root: Node,
    mode: TrieMode,
}

impl Billet {
    pub fn new(root_hash: Uint256, mode: TrieMode, prefix: KeyPrefix, store: Arc<Mutex<MemCachedStore>>) -> Self {
        Billet {
            temp_storage_prefix: prefix,
            store,
            root: Node::HashNode(HashNode::new(root_hash)),
            mode,
        }
    }

    pub fn restore_hash_node(&mut self, path: &[u8], node: Node) -> Result<(), Box<dyn Error>> {
        if let Node::HashNode(_) = node {
            return Err(Box::new(RestoreFailedError));
        }
        if let Node::EmptyNode = node {
            return Err(Box::new(RestoreFailedError));
        }
        let r = self.put_into_node(self.root.clone(), path, node.clone())?;
        self.root = r;

        if let Node::LeafNode(leaf) = node {
            if self.temp_storage_prefix == 0 {
                panic!("invalid storage prefix");
            }
            let mut k = vec![self.temp_storage_prefix as u8];
            k.extend_from_slice(&from_nibbles(path));
            self.store.lock().unwrap().put(&k, &leaf.value);
        }
        Ok(())
    }

    fn put_into_node(&mut self, curr: Node, path: &[u8], val: Node) -> Result<Node, Box<dyn Error>> {
        match curr {
            Node::LeafNode(leaf) => self.put_into_leaf(leaf, path, val),
            Node::BranchNode(branch) => self.put_into_branch(branch, path, val),
            Node::ExtensionNode(ext) => self.put_into_extension(ext, path, val),
            Node::HashNode(hash) => self.put_into_hash(hash, path, val),
            Node::EmptyNode => Err(Box::new(RestoreFailedError)),
        }
    }

    fn put_into_leaf(&mut self, curr: LeafNode, path: &[u8], val: Node) -> Result<Node, Box<dyn Error>> {
        if !path.is_empty() {
            return Err(Box::new(RestoreFailedError));
        }
        if curr.hash() != val.hash() {
            return Err(Box::new(RestoreFailedError));
        }
        panic!("bug: can't restore LeafNode");
    }

    fn put_into_branch(&mut self, mut curr: BranchNode, path: &[u8], val: Node) -> Result<Node, Box<dyn Error>> {
        if path.is_empty() && curr.hash() == val.hash() {
            panic!("bug: can't perform restoring of BranchNode twice");
        }
        let (i, path) = split_path(path);
        let r = self.put_into_node(curr.children[i].clone(), path, val)?;
        curr.children[i] = r;
        Ok(self.try_collapse_branch(curr))
    }

    fn put_into_extension(&mut self, mut curr: ExtensionNode, path: &[u8], val: Node) -> Result<Node, Box<dyn Error>> {
        if path.is_empty() {
            if curr.hash() != val.hash() {
                return Err(Box::new(RestoreFailedError));
            }
            panic!("bug: can't perform restoring of ExtensionNode twice");
        }
        if !path.starts_with(&curr.key) {
            return Err(Box::new(RestoreFailedError));
        }

        let r = self.put_into_node(curr.next.clone(), &path[curr.key.len()..], val)?;
        curr.next = r;
        Ok(self.try_collapse_extension(curr))
    }

    fn put_into_hash(&mut self, curr: HashNode, path: &[u8], val: Node) -> Result<Node, Box<dyn Error>> {
        if !path.is_empty() {
            return Err(Box::new(RestoreFailedError));
        }

        if val.hash() != curr.hash() {
            return Err(Box::new(RestoreFailedError));
        }

        if curr.collapsed {
            panic!("bug: can't perform restoring of collapsed node");
        }

        self.increment_ref_and_store(val.hash(), &val.bytes());

        if let Node::LeafNode(leaf) = val {
            Ok(self.try_collapse_leaf(leaf))
        } else {
            Ok(val)
        }
    }

    fn increment_ref_and_store(&mut self, h: Uint256, bs: &[u8]) {
        let key = make_storage_key(h);
        if self.mode.rc() {
            let mut cnt = 0;
            let mut data = vec![];
            if let Ok(existing_data) = self.store.lock().unwrap().get(&key) {
                cnt = i32::from_le_bytes(existing_data[existing_data.len() - 4..].try_into().unwrap());
            }
            cnt += 1;
            if data.is_empty() {
                data.extend_from_slice(bs);
                data.extend_from_slice(&1i32.to_le_bytes());
            }
            data[data.len() - 4..].copy_from_slice(&cnt.to_le_bytes());
            self.store.lock().unwrap().put(&key, &data);
        } else {
            self.store.lock().unwrap().put(&key, bs);
        }
    }

    pub fn traverse<F>(&mut self, process: F, ignore_storage_err: bool) -> Result<(), Box<dyn Error>>
    where
        F: Fn(&[u8], &Node, &[u8]) -> bool,
    {
        let r = self.traverse_inner(self.root.clone(), &[], &[], &process, ignore_storage_err, false)?;
        self.root = r;
        Ok(())
    }

    fn traverse_inner<F>(
        &mut self,
        curr: Node,
        path: &[u8],
        from: &[u8],
        process: &F,
        ignore_storage_err: bool,
        backwards: bool,
    ) -> Result<Node, Box<dyn Error>>
    where
        F: Fn(&[u8], &Node, &[u8]) -> bool,
    {
        if let Node::EmptyNode = curr {
            return Ok(curr);
        }
        if let Node::HashNode(hn) = curr {
            let r = self.get_from_store(hn.hash())?;
            return self.traverse_inner(r, path, from, process, ignore_storage_err, backwards);
        }
        if from.is_empty() {
            let bytes = curr.bytes().to_vec();
            if process(&from_nibbles(path), &curr, &bytes) {
                return Err(Box::new(StopError));
            }
        }
        match curr {
            Node::LeafNode(leaf) => Ok(self.try_collapse_leaf(leaf)),
            Node::BranchNode(mut branch) => {
                let (start_index, end_index, cmp, step) = if backwards {
                    (children_count - 1, 0, |i| i >= 0, -1)
                } else {
                    (0, children_count, |i| i < children_count, 1)
                };
                let mut from = from;
                for i in (start_index..end_index).step_by(step) {
                    let new_path = if i == children_count - 1 {
                        path.to_vec()
                    } else {
                        let mut new_path = path.to_vec();
                        new_path.push(i as u8);
                        new_path
                    };
                    if i != start_index {
                        from = &[];
                    }
                    let r = self.traverse_inner(branch.children[i].clone(), &new_path, from, process, ignore_storage_err, backwards)?;
                    branch.children[i] = r;
                }
                Ok(self.try_collapse_branch(branch))
            }
            Node::ExtensionNode(mut ext) => {
                if !from.is_empty() && from.starts_with(&ext.key) {
                    from = &from[ext.key.len()..];
                } else if from.is_empty() || ext.key > from {
                    from = &[];
                } else {
                    return Ok(self.try_collapse_extension(ext));
                }
                let r = self.traverse_inner(ext.next.clone(), &[path, &ext.key].concat(), from, process, ignore_storage_err, backwards)?;
                ext.next = r;
                Ok(self.try_collapse_extension(ext))
            }
            _ => Err(Box::new(RestoreFailedError)),
        }
    }

    fn try_collapse_leaf(&self, curr: LeafNode) -> Node {
        let mut res = HashNode::new(curr.hash());
        res.collapsed = true;
        Node::HashNode(res)
    }

    fn try_collapse_extension(&self, curr: ExtensionNode) -> Node {
        if let Node::HashNode(hash) = &curr.next {
            if hash.collapsed {
                let mut res = HashNode::new(curr.hash());
                res.collapsed = true;
                return Node::HashNode(res);
            }
        }
        Node::ExtensionNode(curr)
    }

    fn try_collapse_branch(&self, curr: BranchNode) -> Node {
        let can_collapse = curr.children.iter().all(|child| {
            matches!(child, Node::EmptyNode) || matches!(child, Node::HashNode(hash) if hash.collapsed)
        });
        if can_collapse {
            let mut res = HashNode::new(curr.hash());
            res.collapsed = true;
            Node::HashNode(res)
        } else {
            Node::BranchNode(curr)
        }
    }

    pub fn get_from_store(&self, h: Uint256) -> Result<Node, Box<dyn Error>> {
        let data = self.store.lock().unwrap().get(&make_storage_key(h))?;
        let mut r = io::BinReader::new(&data);
        let mut n = NodeObject::default();
        n.decode_binary(&mut r)?;
        if self.mode.rc() {
            n.node.set_cache(&data[..data.len() - 5], h);
        }
        Ok(n.node)
    }
}
