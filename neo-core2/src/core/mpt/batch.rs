use std::cmp::Ordering;
use std::collections::HashMap;

#[derive(Default)]
pub struct Batch {
    kv: Vec<KeyValue>,
}

#[derive(Clone)]
struct KeyValue {
    key: Vec<u8>,
    value: Vec<u8>,
}

impl Batch {
    // MapToMPTBatch makes a Batch from an unordered set of storage changes.
    pub fn map_to_mpt_batch(m: HashMap<String, Vec<u8>>) -> Batch {
        let mut b = Batch::default();
        b.kv = m.into_iter()
            .map(|(k, v)| KeyValue { key: str_to_nibbles(&k), value: v })
            .collect();
        b.kv.sort_by(|a, b| a.key.cmp(&b.key));
        b
    }
}

impl Trie {
    // PutBatch puts a batch to a trie.
    pub fn put_batch(&mut self, b: Batch) -> Result<usize, String> {
        if b.kv.is_empty() {
            return Ok(0);
        }
        let (r, n, err) = self.put_batch_internal(b.kv)?;
        self.root = r;
        Ok(n)
    }

    fn put_batch_internal(&mut self, kv: Vec<KeyValue>) -> Result<(Node, usize, String), String> {
        self.put_batch_into_node(self.root.clone(), kv)
    }

    fn put_batch_into_node(&mut self, curr: Node, kv: Vec<KeyValue>) -> Result<(Node, usize, String), String> {
        match curr {
            Node::LeafNode(n) => self.put_batch_into_leaf(n, kv),
            Node::BranchNode(n) => self.put_batch_into_branch(n, kv),
            Node::ExtensionNode(n) => self.put_batch_into_extension(n, kv),
            Node::HashNode(n) => self.put_batch_into_hash(n, kv),
            Node::EmptyNode => self.put_batch_into_empty(kv),
            _ => panic!("invalid MPT node type"),
        }
    }

    fn put_batch_into_leaf(&mut self, curr: LeafNode, kv: Vec<KeyValue>) -> Result<(Node, usize, String), String> {
        self.remove_ref(&curr.hash(), &curr.bytes());
        self.new_sub_trie_many(None, kv, Some(curr.value))
    }

    fn put_batch_into_branch(&mut self, curr: BranchNode, kv: Vec<KeyValue>) -> Result<(Node, usize, String), String> {
        self.add_to_branch(curr, kv, true)
    }

    fn merge_extension(&mut self, prefix: Vec<u8>, sub: Node) -> Result<Node, String> {
        match sub {
            Node::ExtensionNode(mut sn) => {
                self.remove_ref(&sn.hash(), &sn.bytes);
                sn.key.extend(prefix);
                sn.invalidate_cache();
                self.add_ref(&sn.hash(), &sn.bytes);
                Ok(Node::ExtensionNode(sn))
            }
            Node::EmptyNode => Ok(Node::EmptyNode),
            Node::HashNode(sn) => {
                let n = self.get_from_store(&sn.hash)?;
                self.merge_extension(prefix, n)
            }
            _ => {
                if !prefix.is_empty() {
                    let e = ExtensionNode::new(prefix, sub);
                    self.add_ref(&e.hash(), &e.bytes);
                    Ok(Node::ExtensionNode(e))
                } else {
                    Ok(sub)
                }
            }
        }
    }

    fn put_batch_into_extension(&mut self, curr: ExtensionNode, kv: Vec<KeyValue>) -> Result<(Node, usize, String), String> {
        self.remove_ref(&curr.hash(), &curr.bytes);

        let common = lcp_many(&kv);
        let pref = lcp(&common, &curr.key);
        if pref.len() == curr.key.len() {
            strip_prefix(curr.key.len(), &mut kv);
            let (sub, n, err) = self.put_batch_into_node(curr.next, kv)?;
            let sub = self.merge_extension(pref, sub)?;
            return Ok((sub, n, err));
        }

        if !pref.is_empty() {
            strip_prefix(pref.len(), &mut kv);
            let (sub, n, err) = self.put_batch_into_extension_no_prefix(curr.key[pref.len()..].to_vec(), curr.next, kv)?;
            let sub = self.merge_extension(pref, sub)?;
            return Ok((sub, n, err));
        }
        self.put_batch_into_extension_no_prefix(curr.key, curr.next, kv)
    }

    fn put_batch_into_extension_no_prefix(&mut self, key: Vec<u8>, next: Node, kv: Vec<KeyValue>) -> Result<(Node, usize, String), String> {
        let mut b = BranchNode::new();
        if key.len() > 1 {
            b.children[key[0] as usize] = self.new_sub_trie(Some(key[1..].to_vec()), next, false);
        } else {
            b.children[key[0] as usize] = next;
        }
        self.add_to_branch(b, kv, false)
    }

    fn add_to_branch(&mut self, mut b: BranchNode, kv: Vec<KeyValue>, in_trie: bool) -> Result<(Node, usize, String), String> {
        if in_trie {
            self.remove_ref(&b.hash(), &b.bytes);
        }

        let (n, err) = self.iterate_batch(kv, |c, kv| {
            let (child, n, err) = self.put_batch_into_node(b.children[c as usize].clone(), kv)?;
            b.children[c as usize] = child;
            Ok((n, err))
        })?;
        if in_trie && n != 0 {
            b.invalidate_cache();
        }

        let (nd, b_err) = self.strip_branch(b)?;
        if err.is_none() {
            return Ok((nd, n, b_err));
        }
        Ok((nd, n, err))
    }

    fn strip_branch(&mut self, b: BranchNode) -> Result<(Node, String), String> {
        let mut n = 0;
        let mut last_index = 0;
        for (i, child) in b.children.iter().enumerate() {
            if !child.is_empty() {
                n += 1;
                last_index = i;
            }
        }
        match n {
            0 => Ok((Node::EmptyNode, String::new())),
            1 => {
                if last_index != LAST_CHILD {
                    return self.merge_extension(vec![last_index as u8], b.children[last_index].clone());
                }
                Ok((b.children[last_index].clone(), String::new()))
            }
            _ => {
                self.add_ref(&b.hash(), &b.bytes);
                Ok((Node::BranchNode(b), String::new()))
            }
        }
    }

    fn iterate_batch<F>(&mut self, mut kv: Vec<KeyValue>, mut f: F) -> Result<(usize, String), String>
    where
        F: FnMut(u8, Vec<KeyValue>) -> Result<(usize, String), String>,
    {
        let mut n = 0;
        while !kv.is_empty() {
            let (c, i) = get_last_index(&kv);
            if c != LAST_CHILD {
                strip_prefix(1, &mut kv[..i]);
            }
            let (sub, err) = f(c, kv[..i].to_vec())?;
            n += sub;
            if err.is_some() {
                return Ok((n, err));
            }
            kv = kv[i..].to_vec();
        }
        Ok((n, String::new()))
    }

    fn put_batch_into_empty(&mut self, kv: Vec<KeyValue>) -> Result<(Node, usize, String), String> {
        let common = lcp_many(&kv);
        strip_prefix(common.len(), &mut kv);
        self.new_sub_trie_many(Some(common), kv, None)
    }

    fn put_batch_into_hash(&mut self, curr: HashNode, kv: Vec<KeyValue>) -> Result<(Node, usize, String), String> {
        let result = self.get_from_store(&curr.hash)?;
        self.put_batch_into_node(result, kv)
    }

    fn new_sub_trie_many(&mut self, prefix: Option<Vec<u8>>, mut kv: Vec<KeyValue>, value: Option<Vec<u8>>) -> Result<(Node, usize, String), String> {
        if kv[0].key.is_empty() {
            if kv[0].value.is_none() {
                if kv.len() == 1 {
                    return Ok((Node::EmptyNode, 1, String::new()));
                }
                let (node, n, err) = self.new_sub_trie_many(prefix, kv[1..].to_vec(), None)?;
                return Ok((node, n + 1, err));
            }
            if kv.len() == 1 {
                return Ok((self.new_sub_trie(prefix, Node::LeafNode(LeafNode::new(kv[0].value.clone())), true), 1, String::new()));
            }
            value = kv[0].value.clone();
        }

        let mut b = BranchNode::new();
        if let Some(value) = value {
            let leaf = LeafNode::new(value);
            self.add_ref(&leaf.hash(), &leaf.bytes);
            b.children[LAST_CHILD] = Node::LeafNode(leaf);
        }
        let (nd, n, err) = self.add_to_branch(b, kv, false)?;
        let nd = self.merge_extension(prefix.unwrap_or_default(), nd)?;
        Ok((nd, n, err))
    }
}

fn strip_prefix(n: usize, kv: &mut [KeyValue]) {
    for kv in kv.iter_mut() {
        kv.key = kv.key[n..].to_vec();
    }
}

fn get_last_index(kv: &[KeyValue]) -> (u8, usize) {
    if kv[0].key.is_empty() {
        return (LAST_CHILD, 1);
    }
    let c = kv[0].key[0];
    for (i, kv) in kv.iter().enumerate().skip(1) {
        if kv.key[0] != c {
            return (c, i);
        }
    }
    (c, kv.len())
}
