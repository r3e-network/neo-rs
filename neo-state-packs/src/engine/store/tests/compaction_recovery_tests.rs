    #[test]
    fn compaction_plan_builds_without_the_writer_and_preserves_later_appends() {
        let root = tempdir().expect("temporary append store");
        let mut store = PackStore::create(root.path(), small_compaction_config(1024 * 1024))
            .expect("create store");
        let target = key(80);
        append_without_maintenance(&mut store, &[put(target, b"v0")]);
        append_without_maintenance(&mut store, &[put(target, b"v1")]);
        append_without_maintenance(&mut store, &[put(target, b"v2")]);
        let debt = store.compaction_debt();
        assert_eq!(debt.excess_runs, 1);
        assert!(!debt.backpressure_required);

        let plan = store
            .plan_compaction()
            .expect("plan compaction")
            .expect("overfull L0 has a plan");
        // The immutable plan no longer borrows the writer. A canonical append
        // can therefore land while the derived output is being built.
        append_without_maintenance(&mut store, &[put(target, b"v3")]);
        store.gc().expect("gc honors the plan's source lease");
        let prepared = plan.build().expect("build compacted output");
        let output = root
            .path()
            .join("runs/run-l1-00000000000000000000-00000000000000000002.idx");
        store
            .gc()
            .expect("gc must honor the in-flight output lease");
        assert!(output.exists(), "prepared output must survive runtime GC");
        store
            .adopt_compaction(prepared)
            .expect("adopt against the later generation");

        assert_eq!(
            store.get(&target).expect("read latest after adoption"),
            Some(b"v3".to_vec())
        );
        assert!(store.runs.iter().any(|live| live.level == 0));
        assert!(store.runs.iter().any(|live| live.level == 1));
        assert!(store.runs.iter().any(|live| {
            live.level == 0 && live.run.format_version == XOR_INDEX_RUN_FORMAT_VERSION
        }));
        assert!(store.runs.iter().any(|live| {
            live.level == 1 && live.run.format_version == PACK_INDEX_RUN_FORMAT_VERSION
        }));
        let scrub = store.scrub_index_runs().expect("scrub mixed v3/v4 runs");
        assert_eq!(scrub.runs, 2);
        assert_eq!(scrub.v3_runs, 1);
        assert_eq!(scrub.v4_runs, 1);
        drop(store);
        let reopened = PackStore::open(root.path(), store_config(1024 * 1024))
            .expect("reopen compacted store");
        assert_eq!(
            reopened.get(&target).expect("read latest after reopen"),
            Some(b"v3".to_vec())
        );
        assert_eq!(
            reopened
                .scrub_index_runs()
                .expect("scrub reopened mixed runs"),
            scrub
        );
    }

    #[test]
    fn index_scrub_detects_middle_record_corruption_in_a_non_tail_v4_run() {
        let root = tempdir().expect("temporary append store");
        let mut store = PackStore::create(root.path(), small_compaction_config(1024 * 1024))
            .expect("create store");
        store
            .append(&[put(key(1), b"one")])
            .expect("append frame 0");
        store
            .append(&[put(key(2), b"two")])
            .expect("append frame 1");
        store
            .append(&[put(key(3), b"three")])
            .expect("append and compact frame 2");
        append_without_maintenance(&mut store, &[put(key(4), b"tail")]);
        let compacted = store
            .runs
            .iter()
            .find(|live| live.level == 1)
            .expect("live v4 compacted run");
        assert_eq!(compacted.run.format_version, PACK_INDEX_RUN_FORMAT_VERSION);
        assert_eq!(compacted.run.record_count, 3);
        let corrupt_offset =
            compacted.run.records_offset + INDEX_RECORD_LEN as u64 + PACK_KEY_BYTES as u64 + 4;
        let path = root.path().join("runs").join(run_file_name(1, 0, 2));
        drop(store);

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .expect("open non-tail v4 run");
        let mut byte = [0u8; 1];
        file.read_exact_at(&mut byte, corrupt_offset)
            .expect("read middle record byte");
        file.write_all_at(&[byte[0] ^ 0x40], corrupt_offset)
            .expect("corrupt middle record byte");
        file.sync_all().expect("sync middle record corruption");
        drop(file);

        let error = PackStore::open(root.path(), store_config(1024 * 1024))
            .err()
            .expect("ordinary open must reject middle-record corruption");
        assert!(format!("{error:#}").contains("checksum mismatch"));
    }

    #[test]
    fn failed_manifest_adoption_keeps_the_previous_in_memory_generation() {
        let root = tempdir().expect("temporary append store");
        let mut store = PackStore::create(root.path(), small_compaction_config(1024 * 1024))
            .expect("create store");
        let target = key(70);
        append_without_maintenance(&mut store, &[put(target, b"v0")]);
        append_without_maintenance(&mut store, &[put(target, b"v1")]);
        append_without_maintenance(&mut store, &[put(target, b"v2")]);
        let generation = store.generation;
        let run_count = store.runs.len();
        let decoded = store.decoded_index_bytes;
        let plan = store
            .plan_compaction()
            .expect("plan compaction")
            .expect("overfull level has a plan");
        let prepared = plan.build().expect("build invisible compacted output");
        let next_generation = generation + 1;
        let manifest_temp = root
            .path()
            .join(format!("manifest-{next_generation:020}.tmp"));
        fs::write(&manifest_temp, b"block candidate manifest publication")
            .expect("create manifest collision");

        let error = store
            .adopt_compaction(prepared)
            .expect_err("manifest collision must reject adoption");
        assert!(error.to_string().contains("create manifest"));
        assert_eq!(store.generation, generation);
        assert_eq!(store.runs.len(), run_count);
        assert_eq!(store.decoded_index_bytes, decoded);
        assert_eq!(
            store.get(&target).expect("read previous live generation"),
            Some(b"v2".to_vec())
        );
    }

    #[test]
    fn runtime_gc_does_not_delete_an_active_compaction_temp_file() {
        let root = tempdir().expect("temporary append store");
        let mut store =
            PackStore::create(root.path(), store_config(1024 * 1024)).expect("create store");
        store.append(&[put(key(1), b"one")]).expect("append frame");
        let temp = root.path().join("runs/run-active-compaction.idx.tmp");
        fs::write(&temp, b"active").expect("create active temp file");
        store.gc().expect("runtime gc");
        assert!(temp.exists(), "runtime GC raced with an active build");
        drop(store);
        let reopened =
            PackStore::open(root.path(), store_config(1024 * 1024)).expect("reopen store");
        assert!(
            !temp.exists(),
            "startup recovery must remove stale temp files"
        );
        drop(reopened);
    }

    #[test]
    fn leveled_compaction_bounds_levels_beyond_l2() {
        let root = tempdir().expect("temporary append store");
        let mut store = PackStore::create(root.path(), small_compaction_config(1024 * 1024))
            .expect("create store");
        for frame in 0..27u8 {
            store
                .append(&[put(key(frame), &[frame])])
                .expect("append recursively compacted frame");
        }
        let debt = store.compaction_debt();
        assert_eq!(debt.excess_runs, 0, "all levels stay within bounds");
        assert!(!debt.backpressure_required);
        assert!(
            store.runs.iter().any(|live| live.level >= 3),
            "long-running stores must compact beyond the former L2 ceiling"
        );
        for level in 0..=store.runs.iter().map(|live| live.level).max().unwrap_or(0) {
            assert!(
                store.runs.iter().filter(|live| live.level == level).count() <= 2,
                "level {level} exceeded its configured run bound"
            );
        }
    }

    #[test]
    fn lease_prevents_reclamation_until_snapshot_release() {
        let root = tempdir().expect("temporary append store");
        let runs_dir = root.path().join("runs");
        let mut store = PackStore::create(root.path(), small_compaction_config(1024 * 1024))
            .expect("create store");
        let target = key(1);
        store.append(&[put(target, b"v1")]).expect("append frame 0");
        store.append(&[put(target, b"v2")]).expect("append frame 1");
        let pinned = store.snapshot().expect("pin generation 2");
        store
            .append(&[put(target, b"v3")])
            .expect("append frame 2 compacts");
        assert_eq!(store.runs.len(), 1);

        let first = store.gc().expect("gc with pinned lease");
        // run-2 is listed only by the superseded pre-compaction generation
        // and is reclaimed; the leased generation's runs must survive.
        assert_eq!(first.runs_deleted, 1, "only unprotected runs go");
        assert_eq!(first.manifests_deleted, 2, "only unprotected manifests go");
        for epoch in 0..2 {
            assert!(
                runs_dir.join(run_file_name(0, epoch, epoch)).exists(),
                "leased run {epoch} must be kept"
            );
        }
        assert!(!runs_dir.join(run_file_name(0, 2, 2)).exists());
        assert!(root.path().join(manifest::manifest_file_name(2)).exists());
        assert!(!root.path().join(manifest::manifest_file_name(1)).exists());
        assert_eq!(
            pinned.get(&target).expect("read through gc"),
            Some(b"v2".to_vec())
        );

        drop(pinned);
        let second = store.gc().expect("gc after lease release");
        assert_eq!(
            second.runs_deleted, 2,
            "leased runs reclaimed after release"
        );
        assert_eq!(second.manifests_deleted, 1, "released manifest reclaimed");
        for epoch in 0..3 {
            assert!(!runs_dir.join(run_file_name(0, epoch, epoch)).exists());
        }
        assert!(
            runs_dir
                .join("run-l1-00000000000000000000-00000000000000000002.idx")
                .exists(),
            "live compacted run stays"
        );
        assert_eq!(
            store.get(&target).expect("read after reclamation"),
            Some(b"v3".to_vec())
        );
        let stats = store.compaction_stats();
        assert_eq!(stats.gc_cycles, 2);
        assert_eq!(stats.gc_runs_deleted, 3);
    }

    #[test]
    fn crash_mid_compaction_keeps_previous_generation_live() {
        let root = tempdir().expect("temporary append store");
        let runs_dir = root.path().join("runs");
        let mut store =
            PackStore::create(root.path(), store_config(1024 * 1024)).expect("create store");
        let target = key(1);
        store
            .append(&[put(target, b"v1"), put(key(2), b"a")])
            .expect("append frame 0");
        store.append(&[put(target, b"v2")]).expect("append frame 1");
        store.append(&[put(target, b"v3")]).expect("append frame 2");

        // Publish the merge output run file but drop the store before the
        // manifest publication: exactly a crash between the two atomic steps.
        let pending = store
            .build_compacted_run(0)
            .expect("merge oldest runs")
            .expect("three runs are mergeable");
        let orphan = runs_dir.join(run_file_name(
            pending.level,
            pending.min_epoch,
            pending.max_epoch,
        ));
        assert!(orphan.exists());
        drop(store);

        let mut reopened = PackStore::open(root.path(), store_config(1024 * 1024))
            .expect("reopen after interrupted compaction");
        assert_eq!(
            reopened.open_validation().runs,
            3,
            "orphan run is invisible"
        );
        assert_eq!(
            reopened.get(&target).expect("read after crash"),
            Some(b"v3".to_vec())
        );
        assert_eq!(
            reopened.get(&key(2)).expect("read sibling after crash"),
            Some(b"a".to_vec())
        );
        assert!(orphan.exists(), "gc did not run yet");
        let stats = reopened
            .gc()
            .expect("reclaim interrupted compaction output");
        assert_eq!(stats.runs_deleted, 1);
        assert!(!orphan.exists());
        assert_eq!(
            reopened.get(&target).expect("read after reclamation"),
            Some(b"v3".to_vec())
        );

        // A crashed append leaves a stale temp file; open clears it so the
        // next publication does not trip over create-new.
        let stale = runs_dir.join("run-00000000000000000003.tmp");
        fs::write(&stale, b"torn").expect("plant stale temp file");
        drop(reopened);
        let mut cleared = PackStore::open(root.path(), store_config(1024 * 1024))
            .expect("reopen clears stale");
        assert!(!stale.exists());
        cleared
            .append(&[put(target, b"v4")])
            .expect("append after stale temp cleanup");
        assert_eq!(
            cleared.get(&target).expect("read appended value"),
            Some(b"v4".to_vec())
        );
    }

    #[test]
    fn reopen_after_compaction_matches_precompaction_byte_for_byte() {
        let root = tempdir().expect("temporary append store");
        let mut store = PackStore::create(root.path(), small_compaction_config(1024 * 1024))
            .expect("create store");
        let mut model: Vec<(bool, [u8; PACK_KEY_BYTES], Option<Vec<u8>>)> = Vec::new();
        for tag in 0..16u8 {
            model.push((false, key(tag), None));
        }
        // Twelve frames drive L0 merges, an L1 merge into L2, tombstones,
        // and rewrites of earlier keys at every level.
        for frame in 0..12u8 {
            let mut operations = Vec::new();
            for ordinal in 0..8u8 {
                let tag = frame.wrapping_mul(3).wrapping_add(ordinal) % 16;
                let value = format!("f{frame}-v{ordinal}");
                operations.push(put(key(tag), value.as_bytes()));
                model[usize::from(tag)] = (true, key(tag), Some(value.into_bytes()));
            }
            if frame % 4 == 3 {
                let tag = (frame + 5) % 16;
                operations.push(tombstone(key(tag)));
                model[usize::from(tag)] = (true, key(tag), None);
            }
            store.append(&operations).expect("append model frame");
            assert_full_scan_matches(&store, &model);
        }
        let all_keys: Vec<_> = model.iter().map(|(_, key, _)| *key).collect();
        let before = store
            .get_many_sorted(&all_keys)
            .expect("capture pre-reopen reads");
        assert!(
            store.runs.iter().any(|live| live.level == 2),
            "L2 must be exercised"
        );
        let stats = store.compaction_stats();
        assert!(
            stats.cycles >= 4,
            "several compaction cycles ran: {stats:?}"
        );
        drop(store);

        let reopened = PackStore::open(root.path(), store_config(1024 * 1024))
            .expect("reopen compacted store");
        let after = reopened
            .get_many_sorted(&all_keys)
            .expect("read reopened compacted store");
        assert_eq!(before, after, "reopen reads diverged after compaction");
        assert_full_scan_matches(&reopened, &model);
    }

    fn assert_full_scan_matches(
        store: &PackStore,
        model: &[(bool, [u8; PACK_KEY_BYTES], Option<Vec<u8>>)],
    ) {
        let touched: Vec<_> = model.iter().filter(|entry| entry.0).collect();
        let keys: Vec<_> = touched.iter().map(|(_, key, _)| *key).collect();
        let actual = store.get_many_sorted(&keys).expect("full sorted scan");
        let expected: Vec<_> = touched.iter().map(|(_, _, value)| value.clone()).collect();
        assert_eq!(actual, expected, "store diverged from the model");
    }

    #[test]
    fn external_horizon_rebuilds_missing_manifest_and_runs_from_frames() {
        let root = tempdir().expect("temporary append store");
        let runs_dir = root.path().join("runs");
        let mut store =
            PackStore::create(root.path(), store_config(1024 * 1024)).expect("create store");
        let target = key(1);
        store
            .append(&[put(target, b"v1"), put(key(2), b"a")])
            .expect("append frame 0");
        store.append(&[put(target, b"v2")]).expect("append frame 1");
        store.append(&[put(key(3), b"c")]).expect("append frame 2");
        let committed = store.last_frame_receipt().expect("committed receipt");
        let horizon = PackCommitHorizon {
            epoch: committed.epoch,
            segment_id: committed.segment_id,
            frame_end: committed.frame_end,
            payload_sha256: committed.payload_sha256,
        };
        drop(store);

        // A missing derived manifest is recoverable only with the explicit
        // canonical horizon. Raw frames alone are not a commit decision.
        for (_, path) in manifest::list_manifest_files(root.path()).expect("list manifests") {
            fs::remove_file(path).expect("delete manifest");
        }
        let reopened = PackStore::open_at_commit_horizon(
            root.path(),
            store_config(1024 * 1024),
            Some(horizon),
        )
        .expect("marker rebuilds the derived generation");
        assert_eq!(reopened.open_validation().frames, 3);
        assert_eq!(reopened.open_validation().runs, 3);
        assert_eq!(
            reopened.get(&target).expect("read reconstructed"),
            Some(b"v2".to_vec())
        );
        let republished = manifest::list_manifest_files(root.path()).expect("list republished");
        assert_eq!(
            republished.len(),
            1,
            "marker recovery republishes one generation"
        );
        assert_eq!(
            republished[0].0, 1,
            "generation restarts after a total manifest loss"
        );
        drop(reopened);

        // Lose the manifest again plus one run: the same marker deterministically
        // rebuilds every run from the committed frame prefix.
        for (_, path) in manifest::list_manifest_files(root.path()).expect("list manifests") {
            fs::remove_file(path).expect("delete manifest");
        }
        fs::remove_file(runs_dir.join(run_file_name(0, 1, 1))).expect("delete one run");
        let mut rebuilt = PackStore::open_at_commit_horizon(
            root.path(),
            store_config(1024 * 1024),
            Some(horizon),
        )
        .expect("marker rebuilds missing runs from frames");
        assert_eq!(rebuilt.open_validation().frames, 3);
        assert_eq!(rebuilt.open_validation().runs, 3);
        assert_eq!(
            rebuilt.get(&target).expect("read rebuilt"),
            Some(b"v2".to_vec())
        );
        assert_eq!(
            rebuilt.get(&key(3)).expect("read rebuilt sibling"),
            Some(b"c".to_vec())
        );
        // The store keeps appending at the right epoch after a rebuild.
        rebuilt
            .append(&[put(target, b"v3")])
            .expect("append after rebuild");
        assert_eq!(
            rebuilt.get(&target).expect("read post-rebuild append"),
            Some(b"v3".to_vec())
        );
    }

    #[test]
    fn prepared_append_is_invisible_in_process_and_without_an_external_horizon() {
        let empty_root = tempdir().expect("temporary empty store");
        let prepared_key = key(7);
        let mut empty = PackStore::create(empty_root.path(), store_config(1024 * 1024))
            .expect("create store");
        let prepared = empty
            .prepare_append(&[put(prepared_key, b"prepared-only")])
            .expect("prepare first frame");
        assert_eq!(prepared.receipt().epoch, 0);
        assert_eq!(prepared.stage_totals().frames, 1);
        assert_eq!(empty.get(&prepared_key).expect("read prepared key"), None);
        assert_eq!(empty.last_frame_receipt(), None);
        assert_eq!(empty.open_validation().frames, 0);
        drop(empty);

        let reopened = PackStore::open(empty_root.path(), store_config(1024 * 1024))
            .expect("plain reopen discards an unactivated first frame");
        assert_eq!(reopened.open_validation().frames, 0);
        assert_eq!(
            reopened.get(&prepared_key).expect("read after reopen"),
            None
        );
        assert_eq!(
            reopened.layout().expect("recovered empty layout").0,
            segment::SEGMENT_HEADER_LEN as u64,
        );

        let prefix_root = tempdir().expect("temporary prefixed store");
        let committed_key = key(1);
        let orphan_key = key(2);
        let mut prefixed = PackStore::create(prefix_root.path(), store_config(1024 * 1024))
            .expect("create prefixed store");
        prefixed
            .append(&[put(committed_key, b"committed")])
            .expect("append committed prefix");
        let committed = prefixed.last_frame_receipt().expect("committed receipt");
        prefixed
            .prepare_append(&[
                put(committed_key, b"unactivated-replacement"),
                put(orphan_key, b"orphan"),
            ])
            .expect("prepare suffix");
        assert_eq!(
            prefixed.get(&committed_key).expect("read visible prefix"),
            Some(b"committed".to_vec())
        );
        assert_eq!(prefixed.get(&orphan_key).expect("read orphan key"), None);
        assert_eq!(prefixed.last_frame_receipt(), Some(committed));
        drop(prefixed);

        let reopened = PackStore::open(prefix_root.path(), store_config(1024 * 1024))
            .expect("plain reopen keeps only manifested prefix");
        assert_eq!(reopened.open_validation().frames, 1);
        assert_eq!(
            reopened.get(&committed_key).expect("read committed key"),
            Some(b"committed".to_vec())
        );
        assert_eq!(reopened.get(&orphan_key).expect("read discarded key"), None);
    }

    #[test]
    fn sealed_append_pins_the_new_generation_while_old_snapshots_stay_old() {
        let root = tempdir().expect("temporary sealed store");
        let target = key(4);
        let added = key(5);
        let mut store =
            PackStore::create(root.path(), store_config(1024 * 1024)).expect("create store");
        store
            .append(&[put(target, b"old")])
            .expect("append committed prefix");
        let old_snapshot = store.snapshot().expect("pin old snapshot");

        let prepared = store
            .prepare_append(&[put(target, b"new"), put(added, b"added")])
            .expect("prepare next generation");
        let expected_horizon = prepared.commit_horizon();
        let sealed = store
            .seal_prepared(prepared)
            .expect("seal prepared generation");

        assert_eq!(sealed.commit_horizon(), expected_horizon);
        assert_eq!(
            old_snapshot.get(&target).expect("read old target"),
            Some(b"old".to_vec())
        );
        assert_eq!(old_snapshot.get(&added).expect("read old added key"), None);
        assert_eq!(
            sealed.snapshot().get(&target).expect("read sealed target"),
            Some(b"new".to_vec())
        );
        assert_eq!(
            sealed
                .snapshot()
                .get(&added)
                .expect("read sealed added key"),
            Some(b"added".to_vec())
        );
        assert!(sealed.snapshot().generation() > old_snapshot.generation());

        let activated_snapshot = sealed.into_snapshot();
        assert_eq!(
            activated_snapshot
                .get(&target)
                .expect("read consumed sealed snapshot"),
            Some(b"new".to_vec())
        );
    }

    #[test]
    fn prior_horizon_discards_a_sealed_but_uncommitted_suffix() {
        let root = tempdir().expect("temporary sealed recovery store");
        let target = key(6);
        let suffix_only = key(7);
        let mut store =
            PackStore::create(root.path(), store_config(1024 * 1024)).expect("create store");
        store
            .append(&[put(target, b"committed")])
            .expect("append committed prefix");
        let committed = store.last_frame_receipt().expect("committed receipt");
        let prior_horizon = PackCommitHorizon {
            epoch: committed.epoch,
            segment_id: committed.segment_id,
            frame_end: committed.frame_end,
            payload_sha256: committed.payload_sha256,
        };

        let prepared = store
            .prepare_append(&[
                put(target, b"sealed-uncommitted"),
                put(suffix_only, b"suffix-only"),
            ])
            .expect("prepare suffix");
        let sealed = store
            .seal_prepared(prepared)
            .expect("seal provisional suffix");
        assert_eq!(
            sealed.snapshot().get(&target).expect("read sealed value"),
            Some(b"sealed-uncommitted".to_vec())
        );
        drop(sealed);
        drop(store);

        let reopened = PackStore::open_at_commit_horizon(
            root.path(),
            store_config(1024 * 1024),
            Some(prior_horizon),
        )
        .expect("reopen at preceding canonical horizon");
        assert_eq!(reopened.open_validation().frames, 1);
        assert_eq!(reopened.last_frame_receipt(), Some(committed));
        assert_eq!(
            reopened.get(&target).expect("read committed target"),
            Some(b"committed".to_vec())
        );
        assert_eq!(
            reopened.get(&suffix_only).expect("read discarded suffix"),
            None
        );
    }

    #[test]
    fn activation_publishes_the_prepared_view_and_survives_reopen() {
        let root = tempdir().expect("temporary append store");
        let target = key(5);
        let mut store =
            PackStore::create(root.path(), store_config(1024 * 1024)).expect("create store");
        let prepared = store
            .prepare_append(&[put(target, b"activated")])
            .expect("prepare frame");
        assert_eq!(store.get(&target).expect("read before activation"), None);
        store
            .activate_prepared(prepared, prepared.commit_horizon())
            .expect("activate prepared frame");
        assert_eq!(store.last_frame_receipt(), Some(prepared.receipt()));
        assert_eq!(
            store.get(&target).expect("read activated value"),
            Some(b"activated".to_vec())
        );
        drop(store);

        let reopened = PackStore::open(root.path(), store_config(1024 * 1024))
            .expect("reopen activated store");
        assert_eq!(reopened.open_validation().frames, 1);
        assert_eq!(
            reopened.get(&target).expect("read reopened value"),
            Some(b"activated".to_vec())
        );
    }

    #[test]
    fn committed_marker_recovers_a_crash_before_in_process_activation() {
        let root = tempdir().expect("temporary append store");
        let target = key(6);
        let mut store =
            PackStore::create(root.path(), store_config(1024 * 1024)).expect("create store");
        let prepared = store
            .prepare_append(&[put(target, b"marker-committed")])
            .expect("prepare frame");
        let horizon = prepared.commit_horizon();
        drop(store);

        let reopened = PackStore::open_at_commit_horizon(
            root.path(),
            store_config(1024 * 1024),
            Some(horizon),
        )
        .expect("marker rebuilds missing activation index");
        assert_eq!(reopened.open_validation().frames, 1);
        assert_eq!(reopened.last_frame_receipt(), Some(prepared.receipt()));
        assert_eq!(
            reopened.get(&target).expect("read marker-recovered value"),
            Some(b"marker-committed".to_vec())
        );
    }

    #[test]
    fn activation_rejects_errors_duplicates_and_reordering_without_visibility() {
        let root = tempdir().expect("temporary append store");
        let first_key = key(10);
        let second_key = key(11);
        let mut store =
            PackStore::create(root.path(), store_config(1024 * 1024)).expect("create store");
        let first = store
            .prepare_append(&[put(first_key, b"first")])
            .expect("prepare first frame");
        assert!(
            store
                .prepare_append(&[put(second_key, b"blocked")])
                .is_err(),
            "a second prepare must not pass the pending frame"
        );
        assert!(store.gc().is_err(), "gc must not reclaim a pending run");

        let mut wrong_checksum = first.commit_horizon();
        wrong_checksum.payload_sha256[0] ^= 0x80;
        let checksum_error = store
            .activate_prepared(first, wrong_checksum)
            .expect_err("wrong marker checksum must fail");
        assert!(checksum_error.to_string().contains("checksum"));
        assert_eq!(store.get(&first_key).expect("read after bad marker"), None);

        let wrong_epoch = PackCommitHorizon {
            epoch: first.receipt().epoch + 1,
            segment_id: first.receipt().segment_id,
            frame_end: first.receipt().frame_end,
            payload_sha256: first.receipt().payload_sha256,
        };
        let epoch_error = store
            .activate_prepared(first, wrong_epoch)
            .expect_err("wrong marker epoch must fail");
        assert!(epoch_error.to_string().contains("epoch"));
        assert_eq!(store.get(&first_key).expect("read after bad epoch"), None);

        let forged = PreparedAppend {
            serial: first.serial + 1,
            ..first
        };
        let token_error = store
            .activate_prepared(forged, first.commit_horizon())
            .expect_err("forged token must fail");
        assert!(token_error.to_string().contains("token"));
        assert_eq!(store.get(&first_key).expect("read after bad token"), None);

        store
            .activate_prepared(first, first.commit_horizon())
            .expect("activate first frame");
        let duplicate_error = store
            .activate_prepared(first, first.commit_horizon())
            .expect_err("duplicate activation must fail");
        assert!(duplicate_error.to_string().contains("no prepared append"));
        assert_eq!(
            store.get(&first_key).expect("first remains visible"),
            Some(b"first".to_vec())
        );

        let second = store
            .prepare_append(&[put(second_key, b"second")])
            .expect("prepare second frame");
        let stale_error = store
            .activate_prepared(first, second.commit_horizon())
            .expect_err("stale token must not activate a later frame");
        assert!(stale_error.to_string().contains("token"));
        assert_eq!(store.get(&second_key).expect("read reordered frame"), None);
        store
            .activate_prepared(second, second.commit_horizon())
            .expect("activate second frame in order");
        assert_eq!(
            store.get(&second_key).expect("read second frame"),
            Some(b"second".to_vec())
        );
    }

    #[test]
    fn activation_revalidates_prepared_frame_and_run_before_publication() {
        let frame_root = tempdir().expect("temporary frame-corruption store");
        let frame_key = key(12);
        let mut frame_store =
            PackStore::create(frame_root.path(), store_config(1024 * 1024))
                .expect("create frame store");
        let frame_prepared = frame_store
            .prepare_append(&[put(frame_key, b"frame-target")])
            .expect("prepare frame target");
        let mut pack = OpenOptions::new()
            .read(true)
            .write(true)
            .open(
                frame_root
                    .path()
                    .join(PackSegmentId::INITIAL.file_name()),
            )
            .expect("open prepared pack");
        pack.seek(SeekFrom::Start(
            segment::SEGMENT_HEADER_LEN as u64 + FRAME_HEADER_LEN as u64 + 1,
        ))
            .expect("seek into prepared payload");
        pack.write_all(&[0x7f]).expect("corrupt prepared payload");
        pack.sync_all().expect("sync prepared payload corruption");
        drop(pack);
        let frame_error = frame_store
            .activate_prepared(frame_prepared, frame_prepared.commit_horizon())
            .expect_err("corrupt prepared frame must not activate");
        assert!(frame_error.to_string().contains("checksum mismatch"));
        assert_eq!(
            frame_store.get(&frame_key).expect("read corrupt frame"),
            None
        );
        assert!(
            manifest::list_manifest_files(frame_root.path())
                .expect("list frame manifests")
                .is_empty()
        );

        let run_root = tempdir().expect("temporary run-corruption store");
        let run_key = key(13);
        let mut run_store = PackStore::create(run_root.path(), store_config(1024 * 1024))
            .expect("create run store");
        let run_prepared = run_store
            .prepare_append(&[put(run_key, b"run-target")])
            .expect("prepare run target");
        let run_path = run_root.path().join("runs").join(run_file_name(0, 0, 0));
        let mut run = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&run_path)
            .expect("open prepared run");
        run.seek(SeekFrom::End(-1)).expect("seek to run tail");
        let mut byte = [0u8; 1];
        run.read_exact(&mut byte).expect("read run tail");
        run.seek(SeekFrom::End(-1)).expect("rewind to run tail");
        run.write_all(&[byte[0] ^ 0x80])
            .expect("corrupt prepared run");
        run.sync_all().expect("sync prepared run corruption");
        drop(run);
        let run_error = run_store
            .activate_prepared(run_prepared, run_prepared.commit_horizon())
            .expect_err("corrupt prepared run must not activate");
        assert!(
            run_error
                .to_string()
                .contains("invalid index tombstone flag")
        );
        assert_eq!(run_store.get(&run_key).expect("read corrupt run"), None);
        assert!(
            manifest::list_manifest_files(run_root.path())
                .expect("list run manifests")
                .is_empty()
        );
    }

    #[test]
    fn external_commit_horizon_discards_complete_orphan_suffix() {
        let root = tempdir().expect("temporary append store");
        let target = key(1);
        let orphan_only = key(9);
        let mut store =
            PackStore::create(root.path(), store_config(1024 * 1024)).expect("create store");
        store
            .append(&[put(target, b"committed-zero")])
            .expect("append frame zero");
        store
            .append(&[put(target, b"committed-one")])
            .expect("append frame one");
        let committed = store.last_frame_receipt().expect("committed frame receipt");
        store
            .append(&[
                put(target, b"orphan-value"),
                put(orphan_only, b"orphan-only"),
            ])
            .expect("append complete orphan frame");
        drop(store);

        let mut reopened = PackStore::open_at_commit_horizon(
            root.path(),
            store_config(1024 * 1024),
            Some(PackCommitHorizon {
                epoch: committed.epoch,
                segment_id: committed.segment_id,
                frame_end: committed.frame_end,
                payload_sha256: committed.payload_sha256,
            }),
        )
        .expect("recover to external commit marker");
        assert_eq!(reopened.open_validation().frames, 2);
        assert_eq!(reopened.last_frame_receipt(), Some(committed));
        assert_eq!(
            reopened.get(&target).expect("read committed value"),
            Some(b"committed-one".to_vec())
        );
        assert_eq!(
            reopened.get(&orphan_only).expect("read discarded key"),
            None
        );

        reopened
            .append(&[put(target, b"replacement-two")])
            .expect("append replacement frame");
        assert_eq!(
            reopened
                .last_frame_receipt()
                .expect("replacement receipt")
                .epoch,
            2
        );
        assert_eq!(
            reopened.get(&target).expect("read replacement value"),
            Some(b"replacement-two".to_vec())
        );
    }

    #[test]
    fn external_commit_horizon_rejects_missing_or_checksum_mismatched_frame() {
        let root = tempdir().expect("temporary append store");
        let mut store =
            PackStore::create(root.path(), store_config(1024 * 1024)).expect("create store");
        store
            .append(&[put(key(1), b"committed")])
            .expect("append committed frame");
        let receipt = store.last_frame_receipt().expect("frame receipt");
        drop(store);

        let mut wrong_checksum = receipt.payload_sha256;
        wrong_checksum[0] ^= 0x80;
        let checksum_error = PackStore::open_at_commit_horizon(
            root.path(),
            store_config(1024 * 1024),
            Some(PackCommitHorizon {
                epoch: receipt.epoch,
                segment_id: receipt.segment_id,
                frame_end: receipt.frame_end,
                payload_sha256: wrong_checksum,
            }),
        )
        .err()
        .expect("checksum mismatch must fail");
        assert!(checksum_error.to_string().contains("checksum"));

        let missing_error = PackStore::open_at_commit_horizon(
            root.path(),
            store_config(1024 * 1024),
            Some(PackCommitHorizon {
                epoch: receipt.epoch + 1,
                segment_id: receipt.segment_id,
                frame_end: receipt.frame_end,
                payload_sha256: receipt.payload_sha256,
            }),
        )
        .err()
        .expect("missing committed frame must fail");
        assert!(missing_error.to_string().contains("only 1 complete frames"));

        let reopened = PackStore::open_at_commit_horizon(
            root.path(),
            store_config(1024 * 1024),
            Some(PackCommitHorizon {
                epoch: receipt.epoch,
                segment_id: receipt.segment_id,
                frame_end: receipt.frame_end,
                payload_sha256: receipt.payload_sha256,
            }),
        )
        .expect("valid marker remains recoverable");
        assert_eq!(reopened.open_validation().frames, 1);
    }
