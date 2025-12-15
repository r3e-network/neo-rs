use parking_lot::Mutex;
use std::sync::{Arc, Weak};

/// Tree node structure mirroring the C# `TreeNode<T>` behaviour.
#[derive(Debug)]
pub struct TreeNode<T> {
    item: T,
    parent: Option<Weak<Mutex<TreeNode<T>>>>,
    children: Vec<Arc<Mutex<TreeNode<T>>>>,
}

impl<T> TreeNode<T> {
    pub fn new(item: T, parent: Option<Weak<Mutex<TreeNode<T>>>>) -> Self {
        Self {
            item,
            parent,
            children: Vec::new(),
        }
    }

    pub fn add_child(parent: &Arc<Mutex<TreeNode<T>>>, item: T) -> Arc<Mutex<TreeNode<T>>> {
        let child = Arc::new(Mutex::new(TreeNode::new(
            item,
            Some(Arc::downgrade(parent)),
        )));
        parent.lock().children.push(child.clone());
        child
    }

    pub fn item(&self) -> &T {
        &self.item
    }

    pub fn children(&self) -> &[Arc<Mutex<TreeNode<T>>>] {
        &self.children
    }

    pub fn parent(&self) -> Option<Weak<Mutex<TreeNode<T>>>> {
        self.parent.clone()
    }
}
