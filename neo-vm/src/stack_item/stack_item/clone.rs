use super::ordering::{combine_hash, hash_bytes};
use super::*;

impl StackItem {
    /// Creates a deep clone of the stack item.
    ///
    /// The tree is copied on an explicit frame stack and memoised by compound
    /// identity, so a chain nested as deep as `MaxStackSize` is copied without
    /// adding one native frame per level. Cycles are preserved: a compound
    /// reached a second time resolves to the copy that is still being filled.
    #[must_use]
    pub fn deep_clone(&self) -> Self {
        /// The compound that a frame is filling.
        enum Sink {
            /// An `Array` under construction.
            Array(ArrayItem),
            /// A `Struct` under construction.
            Struct(StructItem),
            /// A `Map` under construction; its sub items arrive as key, value,
            /// key, value, ... because the recursive form copied them in that
            /// order.
            Map {
                map: MapItem,
                pending_key: Option<StackItem>,
            },
        }

        /// One compound being copied and the source items it still needs.
        struct Frame {
            sink: Sink,
            target: StackItem,
            source: std::vec::IntoIter<StackItem>,
        }

        /// Copies an item that has no children of its own.
        fn clone_leaf(item: &StackItem) -> Option<StackItem> {
            match item {
                StackItem::Null => Some(StackItem::Null),
                StackItem::Boolean(value) => Some(StackItem::Boolean(*value)),
                StackItem::Integer(value) => Some(StackItem::Integer(value.clone())),
                StackItem::ByteString(value) => Some(StackItem::ByteString(value.clone())),
                StackItem::Buffer(buffer) => {
                    Some(StackItem::Buffer(BufferItem::new(buffer.data())))
                }
                StackItem::Pointer(pointer) => Some(StackItem::Pointer(pointer.clone())),
                StackItem::InteropInterface(interface) => {
                    Some(StackItem::InteropInterface(interface.clone()))
                }
                _ => None,
            }
        }

        /// Opens the copy of a compound and records it in `refs` before any
        /// child is visited.
        fn open(
            item: &StackItem,
            refs: &mut std::collections::HashMap<CompoundIdentity, StackItem>,
        ) -> Frame {
            match item {
                StackItem::Array(array) => {
                    let copy = ArrayItem::new(Vec::new());
                    let target = StackItem::Array(copy.clone());
                    refs.insert(
                        compound_identity(item).expect("compound item"),
                        target.clone(),
                    );
                    Frame {
                        sink: Sink::Array(copy),
                        target,
                        source: array.items().into_iter(),
                    }
                }
                StackItem::Struct(structure) => {
                    let copy = StructItem::new(Vec::new());
                    let target = StackItem::Struct(copy.clone());
                    refs.insert(
                        compound_identity(item).expect("compound item"),
                        target.clone(),
                    );
                    Frame {
                        sink: Sink::Struct(copy),
                        target,
                        source: structure.items().into_iter(),
                    }
                }
                StackItem::Map(map) => {
                    let copy = MapItem::new(VmOrderedDictionary::new());
                    let target = StackItem::Map(copy.clone());
                    refs.insert(
                        compound_identity(item).expect("compound item"),
                        target.clone(),
                    );
                    let mut source = Vec::with_capacity(map.len() * 2);
                    for (key, value) in map.iter() {
                        source.push(key);
                        source.push(value);
                    }
                    Frame {
                        sink: Sink::Map {
                            map: copy,
                            pending_key: None,
                        },
                        target,
                        source: source.into_iter(),
                    }
                }
                _ => unreachable!("only compound items are opened"),
            }
        }

        /// Stores a finished child copy in the compound being filled. The
        /// recursive form ignored the `push`/`set` results here as well.
        fn accept(sink: &mut Sink, cloned: StackItem) {
            match sink {
                Sink::Array(array) => {
                    let _ = array.push(cloned);
                }
                Sink::Struct(structure) => {
                    let _ = structure.push(cloned);
                }
                Sink::Map { map, pending_key } => match pending_key.take() {
                    None => *pending_key = Some(cloned),
                    Some(key) => {
                        let _ = map.set(key, cloned);
                    }
                },
            }
        }

        let mut refs: std::collections::HashMap<CompoundIdentity, StackItem> =
            std::collections::HashMap::new();
        let mut frames: Vec<Frame> = Vec::new();
        let mut pending = Some(self.clone());
        let mut cloned = StackItem::Null;

        loop {
            if let Some(item) = pending.take() {
                if let Some(known) =
                    compound_identity(&item).and_then(|identity| refs.get(&identity))
                {
                    cloned = known.clone();
                } else if let Some(leaf) = clone_leaf(&item) {
                    cloned = leaf;
                } else {
                    let mut frame = open(&item, &mut refs);
                    let first = frame.source.next();
                    frames.push(frame);
                    match first {
                        Some(child) => pending = Some(child),
                        None => {
                            let frame = frames.pop().expect("open frame");
                            cloned = frame.target;
                        }
                    }
                    continue;
                }
            }

            if frames.is_empty() {
                return cloned;
            }
            let value = std::mem::replace(&mut cloned, StackItem::Null);
            accept(&mut frames.last_mut().expect("open frame").sink, value);

            let child = frames.last_mut().and_then(|frame| frame.source.next());
            match child {
                Some(child) => pending = Some(child),
                None => {
                    let frame = frames.pop().expect("open frame");
                    cloned = frame.target;
                }
            }
        }
    }

    /// Creates a deep copy respecting execution limits (mirrors C# behaviour).
    pub fn deep_copy(&self, limits: &ExecutionEngineLimits) -> VmResult<Self> {
        match self {
            Self::Struct(structure) => {
                let cloned = structure.clone_with_limits(limits)?;
                Ok(Self::Struct(cloned))
            }
            Self::Array(array) => {
                let copy = array.deep_copy()?;
                Ok(Self::Array(copy))
            }
            Self::Map(map) => {
                let copy = map.deep_copy()?;
                Ok(Self::Map(copy))
            }
            _ => Ok(self.deep_clone()),
        }
    }

    /// Clears all references to other stack items.
    pub fn clear_references(&mut self) {
        match self {
            Self::Array(array) => {
                let _ = array.clear();
            }
            Self::Struct(structure) => {
                let _ = structure.clear();
            }
            Self::Map(map) => {
                let _ = map.clear();
            }
            _ => {}
        }
    }
    /// Computes a deterministic hash code compatible with the C# implementation.
    ///
    /// Child hashes are collected on a value stack and combined in source order
    /// when their parent finishes, so a deeply nested item does not consume
    /// native stack. A compound that (through a cycle) contains itself
    /// contributes `0` instead of being folded forever.
    #[must_use]
    pub fn hash_code(&self) -> i32 {
        enum Task {
            /// Hash this item.
            Item(StackItem),
            /// Fold the `children` hashes that sit on the top of `hashes`.
            Finish {
                identity: CompoundIdentity,
                children: usize,
            },
        }

        fn leaf_hash(item: &StackItem) -> Option<i32> {
            match item {
                StackItem::Null => Some(0),
                StackItem::Boolean(value) => Some(i32::from(*value)),
                StackItem::Integer(value) => Some(hash_bytes(&value.to_signed_bytes_le())),
                StackItem::ByteString(bytes) => Some(hash_bytes(bytes)),
                StackItem::Buffer(buffer) => Some(buffer.with_data(hash_bytes)),
                StackItem::Pointer(pointer) => {
                    let script_ptr = pointer.script() as *const Script as usize as u64;
                    let mut hash = 17;
                    hash = combine_hash(hash, (script_ptr & 0xFFFF_FFFF) as i32);
                    hash = combine_hash(hash, ((script_ptr >> 32) & 0xFFFF_FFFF) as i32);
                    hash = combine_hash(hash, pointer.position() as i32);
                    Some(hash)
                }
                StackItem::InteropInterface(interface) => {
                    let addr = Arc::as_ptr(interface).cast::<()>() as usize as u64;
                    let mut hash = 17;
                    hash = combine_hash(hash, (addr & 0xFFFF_FFFF) as i32);
                    hash = combine_hash(hash, ((addr >> 32) & 0xFFFF_FFFF) as i32);
                    Some(hash)
                }
                _ => None,
            }
        }

        let mut tasks = vec![Task::Item(self.clone())];
        let mut hashes: Vec<i32> = Vec::new();
        let mut visited = std::collections::HashSet::new();

        while let Some(task) = tasks.pop() {
            match task {
                Task::Item(item) => {
                    if let Some(hash) = leaf_hash(&item) {
                        hashes.push(hash);
                        continue;
                    }
                    let Some(identity) = compound_identity(&item) else {
                        continue;
                    };
                    if !visited.insert(identity) {
                        hashes.push(0);
                        continue;
                    }
                    let children: Vec<StackItem> = match &item {
                        StackItem::Array(array) => array.items(),
                        StackItem::Struct(structure) => structure.items(),
                        StackItem::Map(map) => {
                            let entries = map.items();
                            entries
                                .iter()
                                .flat_map(|(key, value)| [key.clone(), value.clone()])
                                .collect()
                        }
                        _ => Vec::new(),
                    };
                    tasks.push(Task::Finish {
                        identity,
                        children: children.len(),
                    });
                    for child in children.into_iter().rev() {
                        tasks.push(Task::Item(child));
                    }
                }
                Task::Finish { identity, children } => {
                    visited.remove(&identity);
                    let start = hashes.len() - children;
                    let mut hash = combine_hash(17, children as i32);
                    for child in &hashes[start..] {
                        hash = combine_hash(hash, *child);
                    }
                    hashes.truncate(start);
                    hashes.push(hash);
                }
            }
        }

        hashes.pop().unwrap_or(0)
    }
}
