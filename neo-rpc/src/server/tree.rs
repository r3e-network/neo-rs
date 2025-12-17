use parking_lot::Mutex;
use std::sync::Arc;

use super::tree_node::TreeNode;

/// Simple tree structure mirroring the behaviour of C# `Tree<T>`.
#[derive(Debug, Default)]
pub struct Tree<T> {
    root: Option<Arc<Mutex<TreeNode<T>>>>,
}

impl<T> Tree<T> {
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
}
