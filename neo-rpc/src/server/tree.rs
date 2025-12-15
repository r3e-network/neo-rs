use parking_lot::Mutex;
use std::sync::{Arc, Weak};

use super::tree_node::TreeNode;

/// Simple tree structure mirroring the behaviour of C# `Tree<T>`.
#[derive(Debug, Default)]
pub struct Tree<T> {
    root: Option<Arc<Mutex<TreeNode<T>>>>,
}

impl<T> Tree<T> {
    pub fn new() -> Self {
        Self { root: None }
    }

    /// Adds (or replaces) the root node of the tree.
    pub fn add_root(&mut self, item: T) -> Arc<Mutex<TreeNode<T>>> {
        // Avoid panicking in long-running node processes; if a prior root exists,
        // treat this as a new invocation and reset the tree.
        self.root = None;
        let node = Arc::new(Mutex::new(TreeNode::new(item, None)));
        self.root = Some(node.clone());
        node
    }

    /// Returns the root node if present.
    pub fn root(&self) -> Option<Arc<Mutex<TreeNode<T>>>> {
        self.root.clone()
    }

    /// Iterates over all items in the tree (pre-order).
    pub fn iter(&self) -> TreeIter<T> {
        TreeIter {
            stack: self
                .root
                .as_ref()
                .map(|root| vec![Arc::downgrade(root)])
                .unwrap_or_default(),
        }
    }
}

pub struct TreeIter<T> {
    stack: Vec<Weak<Mutex<TreeNode<T>>>>,
}

impl<T> Iterator for TreeIter<T>
where
    T: Clone,
{
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(node_weak) = self.stack.pop() {
            if let Some(node_arc) = node_weak.upgrade() {
                let (item, children) = {
                    let node = node_arc.lock();
                    (node.item().clone(), node.children().to_vec())
                };
                for child in children.iter().rev() {
                    self.stack.push(Arc::downgrade(child));
                }
                return Some(item);
            }
        }
        None
    }
}
