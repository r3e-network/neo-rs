#[cfg(test)]
mod tests {
    use super::*;
    use neo_crypto::Sha256Hasher;
    use neo_state_packs::{
        CHECKPOINT_NAMESPACE_DIGEST_DOMAIN, PackFrameReceipt, PackOpKind, PackOperation,
    };
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
        let mut auxiliary_key = [0u8; PACK_KEY_BYTES];
        auxiliary_key[0] = STATE_NODE_PREFIX;
        auxiliary_key[1..].fill(0x44);
        let auxiliary_value = b"checkpoint-auxiliary-node".to_vec();

        let mut pack = PackStore::create(&pack_path, 1024 * 1024).expect("create pack");
        pack.append(&[PackOperation {
            key: auxiliary_key,
            kind: PackOpKind::Put(auxiliary_value.clone()),
        }])
        .expect("append auxiliary node");
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
        for (key, value) in [
            (auxiliary_key, auxiliary_value.as_slice()),
            (root_key, root_value.as_slice()),
        ] {
            hasher.update(&(key.len() as u32).to_le_bytes());
            hasher.update(&key);
            hasher.update(&(value.len() as u64).to_le_bytes());
            hasher.update(value);
        }
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
            "rows": 2,
            "value_bytes": root_value.len() + auxiliary_value.len(),
            "frames": 2,
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
            .put(auxiliary_key.to_vec(), auxiliary_value)
            .expect("write source auxiliary node");
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

    fn append_unmaintained_frames(pack_path: &Path, count: u8) -> PackFrameReceipt {
        let mut pack = PackStore::open(pack_path, 1024 * 1024).expect("open fixture pack");
        let mut receipt = pack.last_frame_receipt().expect("fixture pack receipt");
        for tag in 1..=count {
            let mut key = [0u8; PACK_KEY_BYTES];
            key[0] = STATE_NODE_PREFIX;
            key[1..].fill(tag);
            let prepared = pack
                .prepare_append(&[PackOperation {
                    key,
                    kind: PackOpKind::Put(format!("frame-{tag}").into_bytes()),
                }])
                .expect("prepare unmaintained frame");
            receipt = prepared.receipt();
            let sealed = pack
                .seal_prepared(prepared)
                .expect("seal unmaintained frame");
            drop(sealed.into_snapshot());
        }
        drop(pack);
        receipt
    }

    fn index_file_count(pack_path: &Path) -> usize {
        fs::read_dir(pack_path.join("runs"))
            .expect("read fixture runs")
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "idx"))
            .count()
    }

    fn live_run_count(pack_path: &Path) -> u64 {
        let pack = PackStore::open(pack_path, 1024 * 1024).expect("open fixture pack");
        let runs = pack.open_validation().runs;
        drop(pack);
        runs
    }

    fn pack_generation(pack_path: &Path) -> u64 {
        let pack = PackStore::open(pack_path, 1024 * 1024).expect("open fixture pack");
        let generation = pack
            .materialized_view_evidence(16)
            .expect("read fixture evidence")
            .generation;
        drop(pack);
        generation
    }

    fn corrupt_oldest_run_value_offset(pack_path: &Path) {
        let mut runs = fs::read_dir(pack_path.join("runs"))
            .expect("read fixture runs")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "idx"))
            .collect::<Vec<_>>();
        runs.sort_unstable();
        let oldest = runs.first().expect("fixture has an old run");
        let mut bytes = fs::read(oldest).expect("read old fixture run");
        const INDEX_RECORD_BYTES: usize = PACK_KEY_BYTES + 4 + 8 + 4 + 1;
        let value_offset = bytes.len() - INDEX_RECORD_BYTES + PACK_KEY_BYTES + 4;
        bytes[value_offset] ^= 0x01;
        fs::write(oldest, bytes).expect("corrupt old fixture run");
    }

    #[test]
    fn authority_verifier_accepts_checkpoint_then_marker_and_rejects_tip_drift() {
        let mut fixture = fixture();
        let error = verify_authority(
            &fixture.state,
            &fixture.pack_path,
            0x334F_454E,
            1,
            PackStoreOptions::default(),
            0,
            16,
            false,
            false,
            false,
            false,
            0,
            false,
        )
        .expect_err("checkpoint verification must scan the independent namespace");
        assert!(error.to_string().contains("requires --full-scan"));

        let error = verify_authority(
            &fixture.state,
            &fixture.pack_path,
            0x334F_454E,
            1,
            PackStoreOptions::default(),
            0,
            16,
            true,
            false,
            false,
            false,
            0,
            false,
        )
        .expect_err("checkpoint verification must scrub the complete pack");
        assert!(error.to_string().contains("requires --scrub"));

        let error = verify_authority(
            &fixture.state,
            &fixture.pack_path,
            0x334F_454E,
            1,
            PackStoreOptions::default(),
            0,
            16,
            true,
            false,
            true,
            false,
            0,
            false,
        )
        .expect_err("checkpoint verification must scrub every derived index run");
        assert!(error.to_string().contains("requires --scrub-indexes"));

        assert_eq!(
            verify_authority(
                &fixture.state,
                &fixture.pack_path,
                0x334F_454E,
                1,
                PackStoreOptions {
                    random_point_mmap: true,
                },
                16,
                16,
                true,
                false,
                true,
                true,
                16,
                false,
            )
            .expect("verify checkpoint base"),
            AuthorityState::Checkpoint
        );

        assert_eq!(index_file_count(&fixture.pack_path), 2);

        // Build real compaction debt without invoking the compatibility
        // append API. Each prepared/sealed frame publishes one immutable L0
        // run, so maintenance must materialize a new merged run.
        fixture.receipt = append_unmaintained_frames(&fixture.pack_path, 9);
        assert_eq!(index_file_count(&fixture.pack_path), 11);
        let generation_before = pack_generation(&fixture.pack_path);

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
                PackStoreOptions {
                    random_point_mmap: true,
                },
                16,
                16,
                false,
                true,
                false,
                true,
                100_000,
                false,
            )
            .expect("maintain marker-bound authority"),
            AuthorityState::Marker
        );
        assert_eq!(index_file_count(&fixture.pack_path), 12);
        assert_eq!(live_run_count(&fixture.pack_path), 1);
        assert_eq!(pack_generation(&fixture.pack_path), generation_before + 1);

        assert_eq!(
            verify_authority(
                &fixture.state,
                &fixture.pack_path,
                0x334F_454E,
                1,
                PackStoreOptions {
                    random_point_mmap: true,
                },
                16,
                16,
                false,
                false,
                false,
                true,
                100_000,
                true,
            )
            .expect("GC marker-bound authority"),
            AuthorityState::Marker
        );
        assert_eq!(index_file_count(&fixture.pack_path), 1);
        assert_eq!(live_run_count(&fixture.pack_path), 1);

        put_state_tip(&mut fixture.state, 9, fixture.root);
        let error = verify_authority(
            &fixture.state,
            &fixture.pack_path,
            0x334F_454E,
            1,
            PackStoreOptions::default(),
            16,
            16,
            false,
            false,
            false,
            false,
            16,
            false,
        )
        .expect_err("marker and StateService tip drift must fail closed");
        assert!(
            error
                .to_string()
                .contains("differs from StateService metadata")
        );
    }

    #[test]
    fn checkpoint_rejects_corrupt_non_tail_index_before_lookup() {
        let fixture = fixture();
        corrupt_oldest_run_value_offset(&fixture.pack_path);
        let error = verify_authority(
            &fixture.state,
            &fixture.pack_path,
            0x334F_454E,
            1,
            PackStoreOptions::default(),
            0,
            16,
            true,
            false,
            true,
            true,
            0,
            false,
        )
        .expect_err("non-tail index corruption must fail before any sampled lookup");
        assert!(format!("{error:#}").contains("checksum mismatch during scrub"));
    }

    #[test]
    fn checkpoint_rejects_oversized_source_values_during_borrowed_scan() {
        let mut fixture = fixture();
        let mut oversized_key = [0x66; PACK_KEY_BYTES];
        oversized_key[0] = STATE_NODE_PREFIX;
        fixture
            .state
            .put(
                oversized_key.to_vec(),
                vec![0u8; AUTHORITY_LOOKUP_MAX_VALUE_BYTES + 1],
            )
            .expect("write oversized source node");
        let checkpoint = read_checkpoint(&fixture.pack_path).expect("read fixture checkpoint");
        let pack = PackStore::open(&fixture.pack_path, 1024 * 1024).expect("open fixture pack");

        let error = compare_checkpoint_nodes(
            &fixture.state,
            &pack,
            &checkpoint,
            fixture.identity,
            1,
            16,
            false,
        )
        .expect_err("oversized source value must fail during the borrowed scan");
        assert!(format!("{error:#}").contains("exceeding the verifier limit"));
    }

    #[test]
    fn authority_mutation_flags_require_bounded_evidence_and_index_scrubs() {
        assert!(validate_authority_mutation_flags(true, true, true, 100_000).is_err());
        assert!(validate_authority_mutation_flags(true, false, true, 0).is_err());
        assert!(validate_authority_mutation_flags(true, false, true, 99_999).is_err());
        assert!(validate_authority_mutation_flags(false, true, false, 100_000).is_err());
        assert!(validate_authority_mutation_flags(true, false, false, 100_000).is_err());
        assert!(validate_authority_mutation_flags(true, false, true, 100_000).is_ok());
        assert!(validate_authority_mutation_flags(false, true, true, 100_000).is_ok());
    }
}
