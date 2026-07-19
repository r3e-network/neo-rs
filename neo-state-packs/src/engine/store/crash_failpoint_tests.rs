    const CRASH_ROOT_ENV: &str = "NEO_STATE_PACKS_CRASH_TEST_ROOT";

    #[derive(Clone, Copy)]
    struct CrashBoundary {
        name: &'static str,
        publishes_manifest: bool,
        leaves_complete_orphan: bool,
    }

    const CRASH_BOUNDARIES: [CrashBoundary; 8] = [
        CrashBoundary {
            name: "compaction.run.before-sync",
            publishes_manifest: false,
            leaves_complete_orphan: false,
        },
        CrashBoundary {
            name: "compaction.run.after-sync",
            publishes_manifest: false,
            leaves_complete_orphan: false,
        },
        CrashBoundary {
            name: "compaction.run.after-rename",
            publishes_manifest: false,
            leaves_complete_orphan: true,
        },
        CrashBoundary {
            name: "compaction.run.after-directory-sync",
            publishes_manifest: false,
            leaves_complete_orphan: true,
        },
        CrashBoundary {
            name: "compaction.manifest.before-sync",
            publishes_manifest: false,
            leaves_complete_orphan: true,
        },
        CrashBoundary {
            name: "compaction.manifest.after-sync",
            publishes_manifest: false,
            leaves_complete_orphan: true,
        },
        CrashBoundary {
            name: "compaction.manifest.after-rename",
            publishes_manifest: true,
            leaves_complete_orphan: false,
        },
        CrashBoundary {
            name: "compaction.before-install",
            publishes_manifest: true,
            leaves_complete_orphan: false,
        },
    ];

    struct CrashBaseline {
        receipt: PackFrameReceipt,
        evidence: PackMaterializedViewEvidence,
        frame_len: u64,
        frame_sha256: [u8; 32],
    }

    fn create_crash_fixture(root: &Path) -> CrashBaseline {
        let mut store = PackStore::create(root, 16 * 1024 * 1024).expect("create crash fixture");
        let updated = key(100);
        let deleted = key(101);
        append_without_maintenance(&mut store, &[put(updated, b"v0")]);
        append_without_maintenance(&mut store, &[put(updated, b"v1")]);
        append_without_maintenance(&mut store, &[put(deleted, b"gone")]);
        append_without_maintenance(&mut store, &[tombstone(deleted)]);
        for tag in 102..107 {
            append_without_maintenance(&mut store, &[put(key(tag), &[tag])]);
        }
        let receipt = store.last_frame_receipt().expect("crash fixture receipt");
        let evidence = store
            .materialized_view_evidence(64)
            .expect("crash fixture evidence");
        assert_eq!(evidence.generation, 9);
        assert_eq!(evidence.live_runs, 9);
        let frame_bytes = fs::read(root.join("frames.pack")).expect("read fixture frames");
        let baseline = CrashBaseline {
            receipt,
            evidence,
            frame_len: frame_bytes.len() as u64,
            frame_sha256: digest(&frame_bytes),
        };
        drop(store);
        baseline
    }

    #[test]
    #[ignore = "subprocess worker selected by compaction_crash_boundaries_recover_exactly"]
    fn compaction_crash_worker() {
        let root = std::env::var_os(CRASH_ROOT_ENV).expect("crash fixture root is set");
        let root = PathBuf::from(root);
        let mut store =
            PackStore::open(&root, 16 * 1024 * 1024).expect("open crash fixture");
        let plan = store
            .plan_compaction()
            .expect("plan compaction")
            .expect("nine L0 runs require compaction");
        let prepared = plan.build().expect("build compacted run");
        store
            .adopt_compaction(prepared)
            .expect("adopt compacted run");
        panic!("configured crash failpoint was not reached");
    }

    #[test]
    fn compaction_crash_boundaries_recover_exactly() {
        for boundary in CRASH_BOUNDARIES {
            let root = tempdir().expect("temporary crash fixture");
            let baseline = create_crash_fixture(root.path());
            let output = std::process::Command::new(
                std::env::current_exe().expect("resolve current test executable"),
            )
            .arg("--ignored")
            .arg("--exact")
            .arg("engine::store::tests::compaction_crash_worker")
            .arg("--test-threads=1")
            .env(CRASH_ROOT_ENV, root.path())
            .env(crate::engine::failpoint::ENVIRONMENT_VARIABLE, boundary.name)
            .output()
            .expect("run crash worker");
            assert_eq!(
                output.status.code(),
                Some(crate::engine::failpoint::EXIT_CODE),
                "failpoint {} was not reached\nstdout:\n{}\nstderr:\n{}",
                boundary.name,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );

            let frame_bytes = fs::read(root.path().join("frames.pack")).expect("read frames");
            assert_eq!(frame_bytes.len() as u64, baseline.frame_len);
            assert_eq!(digest(&frame_bytes), baseline.frame_sha256);
            let mut recovered = PackStore::open(root.path(), 16 * 1024 * 1024)
                .expect("reopen after injected crash");
            assert_eq!(recovered.last_frame_receipt(), Some(baseline.receipt));
            let recovered_evidence = recovered
                .materialized_view_evidence(64)
                .expect("recovered evidence");
            assert!(baseline.evidence.state_matches(&recovered_evidence));
            assert_eq!(
                recovered_evidence.generation,
                baseline.evidence.generation + u64::from(boundary.publishes_manifest),
                "unexpected recovered generation at {}",
                boundary.name,
            );
            assert_eq!(
                recovered_evidence.live_runs,
                if boundary.publishes_manifest { 1 } else { 9 },
                "unexpected recovered run count at {}",
                boundary.name,
            );
            assert_eq!(
                recovered.get(&key(100)).expect("read updated key"),
                Some(b"v1".to_vec())
            );
            assert_eq!(recovered.get(&key(101)).expect("read tombstone"), None);
            recovered
                .scrub_committed_frames()
                .expect("scrub recovered frames");
            recovered
                .scrub_index_runs()
                .expect("scrub recovered indexes");
            assert_no_temporary_pack_files(root.path());
            assert_eq!(
                count_index_files(root.path()),
                9 + usize::from(boundary.publishes_manifest || boundary.leaves_complete_orphan),
                "unexpected physical run count before retry at {}",
                boundary.name,
            );

            recovered
                .maintain()
                .unwrap_or_else(|error| panic!("retry maintenance at {}: {error:#}", boundary.name));
            let retried_evidence = recovered
                .materialized_view_evidence(64)
                .expect("evidence after immediate maintenance retry");
            assert!(baseline.evidence.state_matches(&retried_evidence));
            assert_eq!(
                retried_evidence.generation,
                baseline.evidence.generation + 1,
                "retry did not publish exactly one compacted generation at {}",
                boundary.name,
            );
            assert_eq!(
                retried_evidence.live_runs, 1,
                "retry did not converge to one compacted run at {}",
                boundary.name,
            );
            recovered
                .scrub_index_runs()
                .expect("scrub indexes after maintenance retry");

            let gc = recovered.gc().expect("reclaim crash artifacts");
            assert_eq!(
                gc.runs_deleted, 9,
                "retry did not leave exactly the superseded source runs at {}",
                boundary.name,
            );
            drop(recovered);

            let reopened = PackStore::open(root.path(), 16 * 1024 * 1024)
                .expect("second reopen after GC");
            let final_evidence = reopened
                .materialized_view_evidence(64)
                .expect("final evidence");
            assert!(baseline.evidence.state_matches(&final_evidence));
            assert_no_temporary_pack_files(root.path());
        }
    }

    fn assert_no_temporary_pack_files(root: &Path) {
        for directory in [root.to_path_buf(), root.join("runs")] {
            for entry in fs::read_dir(&directory).expect("read pack directory") {
                let path = entry.expect("read pack entry").path();
                assert_ne!(
                    path.extension().and_then(|extension| extension.to_str()),
                    Some("tmp"),
                    "temporary crash artifact survived reopen: {}",
                    path.display()
                );
            }
        }
    }

    fn count_index_files(root: &Path) -> usize {
        fs::read_dir(root.join("runs"))
            .expect("read pack runs")
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|extension| extension == "idx"))
            .count()
    }
