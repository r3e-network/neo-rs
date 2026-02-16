use super::*;

impl StateStore {
    /// Gets a proof for a storage key at a given root.
    pub fn get_proof(&self, root: UInt256, key: &StorageKey) -> Option<Vec<Vec<u8>>> {
        let snapshot = StateStoreSnapshot::new(Arc::clone(&self.store));
        let mut trie = Trie::new(Arc::new(snapshot), Some(root), self.settings.full_state);
        let key_bytes = key.to_array();
        trie.try_get_proof(&key_bytes)
            .ok()
            .flatten()
            .map(|set| set.into_iter().collect())
    }

    /// Builds a trie anchored at the supplied root hash for querying state.
    pub fn trie_for_root(&self, root: UInt256) -> Trie<StateStoreSnapshot> {
        let snapshot = StateStoreSnapshot::new(Arc::clone(&self.store));
        Trie::new(Arc::new(snapshot), Some(root), self.settings.full_state)
    }

    /// Verifies a proof.
    pub fn verify_proof(root: UInt256, key: &[u8], proof: &[Vec<u8>]) -> Option<Vec<u8>> {
        let proof_set: std::collections::HashSet<Vec<u8>> = proof.iter().cloned().collect();
        Trie::<StateStoreSnapshot>::verify_proof(root, key, &proof_set).ok()
    }

    /// Serializes a proof payload (key + nodes) for transport over RPC.
    pub fn encode_proof_payload(key: &[u8], nodes: &[Vec<u8>]) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        if let Err(err) = writer.write_var_bytes(key) {
            tracing::error!("failed to serialize proof key: {err}");
            return Vec::new();
        }
        if let Err(err) = writer.write_var_int(nodes.len() as u64) {
            tracing::error!("failed to serialize proof length: {err}");
            return Vec::new();
        }
        for node in nodes {
            if let Err(err) = writer.write_var_bytes(node) {
                tracing::error!("failed to serialize proof node: {err}");
                return Vec::new();
            }
        }
        writer.into_bytes()
    }

    /// Deserializes a proof payload produced by `encode_proof_payload`.
    pub fn decode_proof_payload(bytes: &[u8]) -> Option<(Vec<u8>, Vec<Vec<u8>>)> {
        let mut reader = MemoryReader::new(bytes);
        // Bound proof element sizes to prevent OOM from malicious payloads.
        const MAX_PROOF_BYTES: usize = 0x100000; // 1 MB per element
        const MAX_PROOF_NODES: u64 = 0x10000; // 65536 nodes max

        let key = reader.read_var_bytes(MAX_PROOF_BYTES).ok()?;
        let count = reader.read_var_int(MAX_PROOF_NODES).ok()? as usize;
        let mut nodes = Vec::with_capacity(count);
        for _ in 0..count {
            nodes.push(reader.read_var_bytes(MAX_PROOF_BYTES).ok()?);
        }
        Some((key, nodes))
    }
}
