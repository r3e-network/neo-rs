#[cfg(test)]
mod tests {
    use super::*;
    use neo_state_packs::{PackFrameReceipt, PackOpKind, PackOperation};
    use neo_storage::persistence::providers::MemoryStore;
    use neo_storage::persistence::{StoreMaintenanceBatch, WriteStore};
    use serde_json::json;
    use tempfile::tempdir;

    struct Fixture {
        _temporary: tempfile::TempDir,
        pack_path: PathBuf,
        state: RuntimeStore,
        receipt: PackFrameReceipt,
        identity: [u8; 32],
        root: [u8; 32],
    }

    fn fixture() -> Fixture {
        let temporary = tempdir().expect("temporary verifier fixture");
        let pack_path = temporary.path().join("packs");
        let root = [0x55; 32];
        let mut root_key = [0u8; PACK_KEY_BYTES];
        root_key[0] = STATE_NODE_PREFIX;
        root_key[1..].copy_from_slice(&root);
        let root_value = b"complete-root-node".to_vec();

        let mut pack = PackStore::create(&pack_path, 1024 * 1024).expect("create pack");
        pack.append(&[PackOperation {
            key: root_key,
            kind: PackOpKind::Put(root_value.clone()),
        }])
        .expect("append root node");
        let receipt = pack.last_frame_receipt().expect("pack receipt");
        let scrub = pack.scrub_committed_frames().expect("scrub fixture pack");
        drop(pack);

        let mut hasher = Sha256Hasher::new();
        hasher.update(CHECKPOINT_NAMESPACE_DIGEST_DOMAIN);
        hasher.update(&(root_key.len() as u32).to_le_bytes());
        hasher.update(&root_key);
        hasher.update(&(root_value.len() as u64).to_le_bytes());
        hasher.update(&root_value);
        let identity = hasher.finalize();
        let checkpoint = json!({
            "schema_version": CHECKPOINT_SCHEMA_VERSION,
            "authoritative_ready": true,
            "complete": true,
            "source_backend": "mdbx",
            "source_namespace": STATE_SERVICE_NAMESPACE,
            "network_magic": "0x334F454E",
            "source_height": 7,
            "source_root_internal_bytes": format!("0x{}", hex::encode(root)),
            "source_namespace_sha256": format!("0x{}", hex::encode(identity)),
            "rows": 1,
            "value_bytes": root_value.len(),
            "frames": 1,
            "pack_frame_format_version": PACK_FRAME_FORMAT_VERSION,
            "pack_index_format_version": PACK_INDEX_FORMAT_VERSION,
            "pack_manifest_format_version": PACK_MANIFEST_FORMAT_VERSION,
            "tip_epoch": receipt.epoch,
            "tip_frame_end": receipt.frame_end,
            "tip_payload_sha256": format!("0x{}", hex::encode(receipt.payload_sha256)),
            "scrubbed_frames": scrub.frames,
            "scrubbed_rows": scrub.rows,
            "scrubbed_puts": scrub.puts,
            "scrubbed_tombstones": scrub.tombstones,
            "scrubbed_value_bytes": scrub.value_bytes,
        });
        fs::write(
            pack_path.join("checkpoint.json"),
            serde_json::to_vec_pretty(&checkpoint).expect("encode checkpoint"),
        )
        .expect("write checkpoint");

        let mut state = RuntimeStore::Memory(MemoryStore::new());
        put_state_tip(&mut state, 7, root);
        state
            .put(root_key.to_vec(), root_value)
            .expect("write source node");
        Fixture {
            _temporary: temporary,
            pack_path,
            state,
            receipt,
            identity,
            root,
        }
    }

    fn put_state_tip(store: &mut RuntimeStore, height: u32, root: [u8; 32]) {
        store
            .put(
                CURRENT_LOCAL_ROOT_INDEX.to_vec(),
                height.to_le_bytes().to_vec(),
            )
            .expect("write current StateService index");
        let mut key = vec![STATE_ROOT_PREFIX];
        key.extend_from_slice(&height.to_be_bytes());
        let mut value = vec![0];
        value.extend_from_slice(&height.to_le_bytes());
        value.extend_from_slice(&root);
        store.put(key, value).expect("write StateService root");
    }

    #[test]
    fn authority_verifier_accepts_checkpoint_then_marker_and_rejects_tip_drift() {
        let mut fixture = fixture();
        assert_eq!(
            verify_authority(
                &fixture.state,
                &fixture.pack_path,
                0x334F_454E,
                1,
                16,
                16,
                true,
                false,
                true,
                true,
            )
            .expect("verify checkpoint base"),
            AuthorityState::Checkpoint
        );

        put_state_tip(&mut fixture.state, 8, fixture.root);
        let marker = AuthoritativeHighWaterRecord::new(
            0x334F_454E,
            fixture.identity,
            fixture.receipt,
            8,
            fixture.root,
        );
        let mut maintenance = StoreMaintenanceBatch::new();
        maintenance.put_metadata(
            AUTHORITATIVE_HIGH_WATER_KEY.to_vec(),
            marker.encode().to_vec(),
        );
        fixture
            .state
            .commit_maintenance(&maintenance)
            .expect("publish authority marker");
        assert_eq!(
            verify_authority(
                &fixture.state,
                &fixture.pack_path,
                0x334F_454E,
                1,
                16,
                16,
                false,
                false,
                true,
                false,
            )
            .expect("verify marker-bound authority"),
            AuthorityState::Marker
        );

        put_state_tip(&mut fixture.state, 9, fixture.root);
        let error = verify_authority(
            &fixture.state,
            &fixture.pack_path,
            0x334F_454E,
            1,
            16,
            16,
            false,
            false,
            false,
            false,
        )
        .expect_err("marker and StateService tip drift must fail closed");
        assert!(
            error
                .to_string()
                .contains("differs from StateService metadata")
        );
    }
}
