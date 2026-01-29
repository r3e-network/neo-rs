use parking_lot::Mutex;
use std::sync::{Arc, Weak};

/// Tree node structure mirroring the C# `TreeNode<T>` behaviour.
#[derive(Debug)]
pub struct TreeNode<T> {
    item: T,
    parent: Option<Weak<Mutex<Self>>>,
    children: Vec<Arc<Mutex<Self>>>,
}

impl<T> TreeNode<T> {
    pub const fn new(item: T, parent: Option<Weak<Mutex<Self>>>) -> Self {
        Self {
            item,
            parent,
            children: Vec::new(),
        }
    }

    pub fn add_child(parent: &Arc<Mutex<Self>>, item: T) -> Arc<Mutex<Self>> {
        let child = Arc::new(Mutex::new(Self::new(
            item,
            Some(Arc::downgrade(parent)),
        )));
        parent.lock().children.push(child.clone());
        child
    }

    pub const fn item(&self) -> &T {
        &self.item
    }

    pub fn children(&self) -> &[Arc<Mutex<Self>>] {
        &self.children
    }

    pub fn parent(&self) -> Option<Weak<Mutex<Self>>> {
        self.parent.clone()
    }
}
