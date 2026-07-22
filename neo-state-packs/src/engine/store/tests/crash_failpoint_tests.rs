    const CRASH_ROOT_ENV: &str = "NEO_STATE_PACKS_CRASH_TEST_ROOT";
    const APPEND_CRASH_ROOT_ENV: &str = "NEO_STATE_PACKS_APPEND_CRASH_TEST_ROOT";
    const RECOVERY_REBUILD_CRASH_ROOT_ENV: &str = "NEO_STATE_PACKS_RECOVERY_REBUILD_CRASH_TEST_ROOT";
    const RECOVERY_REBUILD_EXTERNAL_CRASH_ROOT_ENV: &str =
        "NEO_STATE_PACKS_RECOVERY_REBUILD_EXTERNAL_CRASH_TEST_ROOT";
    const RECOVERY_REBUILD_HORIZON_EPOCH_ENV: &str =
        "NEO_STATE_PACKS_RECOVERY_REBUILD_HORIZON_EPOCH";
    const RECOVERY_REBUILD_HORIZON_SEGMENT_ENV: &str =
        "NEO_STATE_PACKS_RECOVERY_REBUILD_HORIZON_SEGMENT";
    const RECOVERY_REBUILD_HORIZON_END_ENV: &str =
        "NEO_STATE_PACKS_RECOVERY_REBUILD_HORIZON_END";
    const RECOVERY_REBUILD_HORIZON_BLOCK_START_ENV: &str =
        "NEO_STATE_PACKS_RECOVERY_REBUILD_HORIZON_BLOCK_START";
    const RECOVERY_REBUILD_HORIZON_BLOCK_END_ENV: &str =
        "NEO_STATE_PACKS_RECOVERY_REBUILD_HORIZON_BLOCK_END";
    const RECOVERY_REBUILD_HORIZON_PREVIOUS_ROOT_ENV: &str =
        "NEO_STATE_PACKS_RECOVERY_REBUILD_HORIZON_PREVIOUS_ROOT";
    const RECOVERY_REBUILD_HORIZON_RESULTING_ROOT_ENV: &str =
        "NEO_STATE_PACKS_RECOVERY_REBUILD_HORIZON_RESULTING_ROOT";
    const RECOVERY_REBUILD_HORIZON_FRAME_DIGEST_ENV: &str =
        "NEO_STATE_PACKS_RECOVERY_REBUILD_HORIZON_FRAME_DIGEST";
    const RECOVERY_REBUILD_ROOT_SENTINEL: &str = "operator-recovery-root.keep";
    const RECOVERY_REBUILD_RUNS_SENTINEL: &str = "operator-recovery-runs.keep";
    const RECOVERY_REBUILD_ROOT_SENTINEL_BYTES: &[u8] = b"preserve unknown root artifact";
    const RECOVERY_REBUILD_RUNS_SENTINEL_BYTES: &[u8] = b"preserve unknown runs artifact";

    #[derive(Clone, Copy)]
    struct RecoveryRebuildCrashBoundary {
        name: &'static str,
        leaves_staging: bool,
        leaves_manifest_temp: bool,
        publishes_manifest: bool,
    }

    const RECOVERY_REBUILD_CRASH_BOUNDARIES: [RecoveryRebuildCrashBoundary; 6] = [
        RecoveryRebuildCrashBoundary {
            name: "recovery.rebuild.after-staging-sync",
            leaves_staging: true,
            leaves_manifest_temp: false,
            publishes_manifest: false,
        },
        RecoveryRebuildCrashBoundary {
            name: "recovery.rebuild.after-run-promotion",
            leaves_staging: true,
            leaves_manifest_temp: false,
            publishes_manifest: false,
        },
        RecoveryRebuildCrashBoundary {
            name: "recovery.rebuild.after-run-directory-sync",
            leaves_staging: true,
            leaves_manifest_temp: false,
            publishes_manifest: false,
        },
        RecoveryRebuildCrashBoundary {
            name: "compaction.manifest.before-sync",
            leaves_staging: false,
            leaves_manifest_temp: true,
            publishes_manifest: false,
        },
        RecoveryRebuildCrashBoundary {
            name: "compaction.manifest.after-sync",
            leaves_staging: false,
            leaves_manifest_temp: true,
            publishes_manifest: false,
        },
        RecoveryRebuildCrashBoundary {
            name: "compaction.manifest.after-rename",
            leaves_staging: false,
            leaves_manifest_temp: false,
            publishes_manifest: true,
        },
    ];

    const EXTERNAL_RECOVERY_REBUILD_CRASH_BOUNDARIES: [RecoveryRebuildCrashBoundary; 3] = [
        RECOVERY_REBUILD_CRASH_BOUNDARIES[0],
        RECOVERY_REBUILD_CRASH_BOUNDARIES[2],
        RECOVERY_REBUILD_CRASH_BOUNDARIES[5],
    ];

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum CrashRunArtifact {
        None,
        Temporary,
        Canonical,
    }

    #[derive(Clone, Copy)]
    struct AppendCrashBoundary {
        name: &'static str,
        pending_segment: bool,
        canonical_segment: bool,
        segment_has_frame: bool,
        run_artifact: CrashRunArtifact,
        publishes_manifest: bool,
    }

    const APPEND_CRASH_BOUNDARIES: [AppendCrashBoundary; 12] = [
        AppendCrashBoundary {
            name: "segment.header.before-sync",
            pending_segment: true,
            canonical_segment: false,
            segment_has_frame: false,
            run_artifact: CrashRunArtifact::None,
            publishes_manifest: false,
        },
        AppendCrashBoundary {
            name: "segment.header.after-sync",
            pending_segment: true,
            canonical_segment: false,
            segment_has_frame: false,
            run_artifact: CrashRunArtifact::None,
            publishes_manifest: false,
        },
        AppendCrashBoundary {
            name: "segment.header.after-rename",
            pending_segment: false,
            canonical_segment: true,
            segment_has_frame: false,
            run_artifact: CrashRunArtifact::None,
            publishes_manifest: false,
        },
        AppendCrashBoundary {
            name: "segment.header.after-directory-sync",
            pending_segment: false,
            canonical_segment: true,
            segment_has_frame: false,
            run_artifact: CrashRunArtifact::None,
            publishes_manifest: false,
        },
        AppendCrashBoundary {
            name: "append.frame.after-write",
            pending_segment: false,
            canonical_segment: true,
            segment_has_frame: true,
            run_artifact: CrashRunArtifact::None,
            publishes_manifest: false,
        },
        AppendCrashBoundary {
            name: "append.frame.after-sync",
            pending_segment: false,
            canonical_segment: true,
            segment_has_frame: true,
            run_artifact: CrashRunArtifact::None,
            publishes_manifest: false,
        },
        AppendCrashBoundary {
            name: "append.run.before-sync",
            pending_segment: false,
            canonical_segment: true,
            segment_has_frame: true,
            run_artifact: CrashRunArtifact::Temporary,
            publishes_manifest: false,
        },
        AppendCrashBoundary {
            name: "append.run.after-sync",
            pending_segment: false,
            canonical_segment: true,
            segment_has_frame: true,
            run_artifact: CrashRunArtifact::Temporary,
            publishes_manifest: false,
        },
        AppendCrashBoundary {
            name: "append.run.after-rename",
            pending_segment: false,
            canonical_segment: true,
            segment_has_frame: true,
            run_artifact: CrashRunArtifact::Canonical,
            publishes_manifest: false,
        },
        AppendCrashBoundary {
            name: "append.run.after-directory-sync",
            pending_segment: false,
            canonical_segment: true,
            segment_has_frame: true,
            run_artifact: CrashRunArtifact::Canonical,
            publishes_manifest: false,
        },
        AppendCrashBoundary {
            name: "append.manifest.before-publication",
            pending_segment: false,
            canonical_segment: true,
            segment_has_frame: true,
            run_artifact: CrashRunArtifact::Canonical,
            publishes_manifest: false,
        },
        AppendCrashBoundary {
            name: "append.manifest.after-publication",
            pending_segment: false,
            canonical_segment: true,
            segment_has_frame: true,
            run_artifact: CrashRunArtifact::Canonical,
            publishes_manifest: true,
        },
    ];

    #[derive(Clone, Copy)]
    struct CrashBoundary {
        name: &'static str,
        publishes_manifest: bool,
    }

    const CRASH_BOUNDARIES: [CrashBoundary; 8] = [
        CrashBoundary {
            name: "compaction.run.before-sync",
            publishes_manifest: false,
        },
        CrashBoundary {
            name: "compaction.run.after-sync",
            publishes_manifest: false,
        },
        CrashBoundary {
            name: "compaction.run.after-rename",
            publishes_manifest: false,
        },
        CrashBoundary {
            name: "compaction.run.after-directory-sync",
            publishes_manifest: false,
        },
        CrashBoundary {
            name: "compaction.manifest.before-sync",
            publishes_manifest: false,
        },
        CrashBoundary {
            name: "compaction.manifest.after-sync",
            publishes_manifest: false,
        },
        CrashBoundary {
            name: "compaction.manifest.after-rename",
            publishes_manifest: true,
        },
        CrashBoundary {
            name: "compaction.before-install",
            publishes_manifest: true,
        },
    ];

    struct CrashBaseline {
        receipt: PackFrameReceipt,
        evidence: PackMaterializedViewEvidence,
        frame_len: u64,
        frame_sha256: [u8; 32],
    }

    struct AppendCrashBaseline {
        receipt: PackFrameReceipt,
        horizon: PackCommitHorizon,
        evidence: PackMaterializedViewEvidence,
        segment_sha256: [u8; 32],
        generation: u64,
    }

    struct RecoveryRebuildCrashBaseline {
        receipt: PackFrameReceipt,
        horizon: PackCommitHorizon,
        evidence: PackMaterializedViewEvidence,
        segment_sha256: [u8; 32],
        segment_bytes: u64,
        generation: u64,
        manifest_path: PathBuf,
        manifest_bytes: Vec<u8>,
        manifest_extents: Vec<ManifestExtent>,
        orphan_segment_bytes: Option<u64>,
    }

    fn append_crash_committed_operations() -> [PackOperation; 1] {
        [put(key(210), b"committed")]
    }

    fn append_crash_candidate_operations() -> [PackOperation; 1] {
        [put(key(211), b"candidate")]
    }

    fn append_crash_config() -> PackStoreConfig {
        let target = PACK_SEGMENT_HEADER_LEN
            + u64::try_from(
                encoded_test_frame(0, TEST_FRAME_CONTEXT, &append_crash_committed_operations()).len(),
            )
            .expect("append crash frame length fits u64");
        store_config(16 * 1024 * 1024)
            .with_segment_limits(target, target.saturating_mul(2))
            .expect("valid append crash segment limits")
    }

    fn create_append_crash_fixture(root: &Path) -> AppendCrashBaseline {
        let config = append_crash_config();
        let mut store = PackStore::create(root, config).expect("create append crash fixture");
        append_without_maintenance(&mut store, &append_crash_committed_operations());
        let receipt = store
            .last_frame_receipt()
            .expect("append crash committed receipt");
        let evidence = store
            .materialized_view_evidence(64)
            .expect("append crash committed evidence");
        let generation = evidence.generation;
        let segment_bytes = fs::read(root.join(PackSegmentId::INITIAL.file_name()))
            .expect("read append crash committed segment");
        let baseline = AppendCrashBaseline {
            receipt,
            horizon: PackCommitHorizon {
                epoch: receipt.epoch,
                segment_id: receipt.segment_id,
                frame_end: receipt.frame_end,
                context: receipt.context,
                frame_sha256: receipt.frame_sha256,
            },
            evidence,
            segment_sha256: digest(&segment_bytes),
            generation,
        };
        drop(store);
        baseline
    }

    fn run_append_crash_worker(root: &Path, boundary: &str) {
        let output = std::process::Command::new(
            std::env::current_exe().expect("resolve current test executable"),
        )
        .arg("--ignored")
        .arg("--exact")
        .arg("engine::store::tests::append_rotation_crash_worker")
        .arg("--test-threads=1")
        .env(APPEND_CRASH_ROOT_ENV, root)
        .env(crate::engine::failpoint::ENVIRONMENT_VARIABLE, boundary)
        .output()
        .expect("run append crash worker");
        assert_eq!(
            output.status.code(),
            Some(crate::engine::failpoint::EXIT_CODE),
            "failpoint {boundary} was not reached\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    fn assert_append_crash_artifacts(
        root: &Path,
        baseline: &AppendCrashBaseline,
        boundary: AppendCrashBoundary,
    ) {
        let next_segment = PackSegmentId::new(1);
        let pending_segment = segment::pending_segment_path(root, next_segment);
        let canonical_segment = root.join(next_segment.file_name());
        assert_eq!(
            pending_segment.exists(),
            boundary.pending_segment,
            "unexpected pending segment state at {}",
            boundary.name
        );
        assert_eq!(
            canonical_segment.exists(),
            boundary.canonical_segment,
            "unexpected canonical segment state at {}",
            boundary.name
        );
        if boundary.pending_segment {
            assert_eq!(
                fs::metadata(&pending_segment)
                    .expect("stat pending crash segment")
                    .len(),
                PACK_SEGMENT_HEADER_LEN,
                "pending segment must contain exactly its header at {}",
                boundary.name
            );
        }
        if boundary.canonical_segment {
            let bytes = fs::metadata(&canonical_segment)
                .expect("stat canonical crash segment")
                .len();
            assert_eq!(
                bytes > PACK_SEGMENT_HEADER_LEN,
                boundary.segment_has_frame,
                "unexpected canonical segment extent at {}",
                boundary.name
            );
        }

        let next_epoch = baseline.receipt.epoch + 1;
        let temporary_run = root.join("runs").join(format!("run-{next_epoch:020}.tmp"));
        let canonical_run = root
            .join("runs")
            .join(run_file_name(0, next_epoch, next_epoch));
        assert_eq!(
            temporary_run.exists(),
            boundary.run_artifact == CrashRunArtifact::Temporary,
            "unexpected temporary run state at {}",
            boundary.name
        );
        assert_eq!(
            canonical_run.exists(),
            boundary.run_artifact == CrashRunArtifact::Canonical,
            "unexpected canonical run state at {}",
            boundary.name
        );
        let next_manifest = root.join(manifest::manifest_file_name(baseline.generation + 1));
        assert_eq!(
            next_manifest.exists(),
            boundary.publishes_manifest,
            "unexpected next manifest state at {}",
            boundary.name
        );
    }

    fn assert_append_crash_recovers_prior_horizon(
        root: &Path,
        baseline: &AppendCrashBaseline,
        boundary: &str,
    ) {
        let mut recovered =
            PackStore::open_at_commit_horizon(root, append_crash_config(), Some(baseline.horizon))
                .unwrap_or_else(|error| panic!("recover prior horizon at {boundary}: {error:#}"));
        assert_eq!(recovered.last_frame_receipt(), Some(baseline.receipt));
        assert_eq!(
            recovered
                .get(&key(210))
                .expect("read committed append crash value"),
            Some(b"committed".to_vec())
        );
        assert_eq!(
            recovered
                .get(&key(211))
                .expect("read discarded append crash value"),
            None
        );
        let evidence = recovered
            .materialized_view_evidence(64)
            .expect("recovered append crash evidence");
        assert!(
            baseline.evidence.state_matches(&evidence),
            "materialized state changed at {boundary}"
        );
        assert_eq!(
            digest(
                &fs::read(root.join(PackSegmentId::INITIAL.file_name()))
                    .expect("read recovered committed segment")
            ),
            baseline.segment_sha256,
            "committed segment changed at {boundary}"
        );
        assert!(!segment::pending_segment_path(root, PackSegmentId::new(1)).exists());
        assert!(!root.join(PackSegmentId::new(1).file_name()).exists());
        assert_no_temporary_pack_files(root);
        recovered
            .gc()
            .expect("reclaim append crash derived orphans");
        assert_eq!(
            count_index_files(root),
            1,
            "derived orphan survived reclamation at {boundary}"
        );
    }

    fn create_recovery_rebuild_crash_fixture(
        root: &Path,
        with_orphan_suffix: bool,
    ) -> RecoveryRebuildCrashBaseline {
        let config = store_config(16 * 1024 * 1024);
        let mut store = PackStore::create(root, config).expect("create recovery-rebuild crash fixture");
        append_without_maintenance(
            &mut store,
            &[
                put(key(220), b"v0"),
                put(key(221), b"delete-me"),
                put(key(222), b"stable"),
            ],
        );
        append_without_maintenance(&mut store, &[put(key(220), b"v1")]);
        append_without_maintenance(&mut store, &[tombstone(key(221)), put(key(223), b"tail")]);

        let receipt = store
            .last_frame_receipt()
            .expect("recovery-rebuild crash receipt");
        let evidence = store
            .materialized_view_evidence(64)
            .expect("recovery-rebuild baseline evidence");
        assert_eq!(evidence.live_runs, 3);
        let generation = evidence.generation;
        let segment_bytes = fs::read(root.join(PackSegmentId::INITIAL.file_name()))
            .expect("read recovery-rebuild committed segment");
        let manifest_path = manifest::list_manifest_files(root)
            .expect("list recovery-rebuild baseline manifests")
            .first()
            .expect("recovery-rebuild baseline manifest")
            .1
            .clone();
        let manifest_bytes = fs::read(&manifest_path).expect("read recovery-rebuild manifest bytes");
        let baseline_manifest =
            manifest::read_manifest(&manifest_path).expect("read recovery-rebuild manifest");
        assert_eq!(baseline_manifest.generation, generation);
        let first_run = store
            .runs
            .iter()
            .find(|live| live.level == 0 && live.min_epoch == 0 && live.max_epoch == 0)
            .expect("first recovery-rebuild run");
        let corrupt_offset = first_run
            .run
            .records_offset
            .checked_add(1)
            .expect("recovery-rebuild corruption offset");
        let orphan_segment_bytes = if with_orphan_suffix {
            let orphan = store
                .prepare_frame(TEST_FRAME_CONTEXT, &[put(key(225), b"orphan-suffix")])
                .expect("prepare recovery-rebuild orphan suffix");
            assert_eq!(orphan.receipt().epoch, receipt.epoch + 1);
            Some(
                fs::metadata(root.join(PackSegmentId::INITIAL.file_name()))
                    .expect("stat recovery-rebuild orphan suffix")
                    .len(),
            )
        } else {
            None
        };
        drop(store);

        flip_persisted_run_byte(
            &root.join("runs").join(run_file_name(0, 0, 0)),
            corrupt_offset,
        );
        fs::write(
            root.join(RECOVERY_REBUILD_ROOT_SENTINEL),
            RECOVERY_REBUILD_ROOT_SENTINEL_BYTES,
        )
        .expect("write recovery-rebuild root sentinel");
        fs::write(
            root.join("runs").join(RECOVERY_REBUILD_RUNS_SENTINEL),
            RECOVERY_REBUILD_RUNS_SENTINEL_BYTES,
        )
        .expect("write recovery-rebuild runs sentinel");
        RecoveryRebuildCrashBaseline {
            receipt,
            horizon: PackCommitHorizon {
                epoch: receipt.epoch,
                segment_id: receipt.segment_id,
                frame_end: receipt.frame_end,
                context: receipt.context,
                frame_sha256: receipt.frame_sha256,
            },
            evidence,
            segment_sha256: digest(&segment_bytes),
            segment_bytes: segment_bytes.len() as u64,
            generation,
            manifest_path,
            manifest_bytes,
            manifest_extents: baseline_manifest.extents,
            orphan_segment_bytes,
        }
    }

    fn recovery_rebuild_bytes_environment(bytes: &[u8; 32]) -> String {
        bytes
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(",")
    }

    fn recovery_rebuild_bytes_from_environment(name: &str) -> [u8; 32] {
        let value = std::env::var(name).unwrap_or_else(|_| panic!("{name} is set"));
        let mut bytes = [0u8; 32];
        let mut parts = value.split(',');
        for byte in &mut bytes {
            *byte = parts
                .next()
                .unwrap_or_else(|| panic!("{name} contains 32 bytes"))
                .parse()
                .unwrap_or_else(|_| panic!("{name} contains decimal bytes"));
        }
        assert!(parts.next().is_none(), "{name} contains exactly 32 bytes");
        bytes
    }

    fn recovery_rebuild_u64_from_environment(name: &str) -> u64 {
        std::env::var(name)
            .unwrap_or_else(|_| panic!("{name} is set"))
            .parse()
            .unwrap_or_else(|_| panic!("{name} contains a u64"))
    }

    fn recovery_rebuild_horizon_from_environment() -> PackCommitHorizon {
        PackCommitHorizon {
            epoch: recovery_rebuild_u64_from_environment(
                RECOVERY_REBUILD_HORIZON_EPOCH_ENV,
            ),
            segment_id: PackSegmentId::new(recovery_rebuild_u64_from_environment(
                RECOVERY_REBUILD_HORIZON_SEGMENT_ENV,
            )),
            frame_end: recovery_rebuild_u64_from_environment(
                RECOVERY_REBUILD_HORIZON_END_ENV,
            ),
            context: PackFrameContext {
                block_start: u32::try_from(recovery_rebuild_u64_from_environment(
                    RECOVERY_REBUILD_HORIZON_BLOCK_START_ENV,
                ))
                .expect("horizon block start fits u32"),
                block_end: u32::try_from(recovery_rebuild_u64_from_environment(
                    RECOVERY_REBUILD_HORIZON_BLOCK_END_ENV,
                ))
                .expect("horizon block end fits u32"),
                previous_root: recovery_rebuild_bytes_from_environment(
                    RECOVERY_REBUILD_HORIZON_PREVIOUS_ROOT_ENV,
                ),
                resulting_root: recovery_rebuild_bytes_from_environment(
                    RECOVERY_REBUILD_HORIZON_RESULTING_ROOT_ENV,
                ),
            },
            frame_sha256: recovery_rebuild_bytes_from_environment(
                RECOVERY_REBUILD_HORIZON_FRAME_DIGEST_ENV,
            ),
        }
    }

    fn run_recovery_rebuild_crash_worker(root: &Path, boundary: &str) {
        let output = std::process::Command::new(
            std::env::current_exe().expect("resolve current test executable"),
        )
        .arg("--ignored")
        .arg("--exact")
        .arg("engine::store::tests::recovery_rebuild_crash_worker")
        .arg("--test-threads=1")
        .env(RECOVERY_REBUILD_CRASH_ROOT_ENV, root)
        .env(crate::engine::failpoint::ENVIRONMENT_VARIABLE, boundary)
        .output()
        .expect("run recovery-rebuild crash worker");
        assert_eq!(
            output.status.code(),
            Some(crate::engine::failpoint::EXIT_CODE),
            "failpoint {boundary} was not reached\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    fn run_external_recovery_rebuild_crash_worker(
        root: &Path,
        horizon: PackCommitHorizon,
        boundary: &str,
    ) {
        let output = std::process::Command::new(
            std::env::current_exe().expect("resolve current test executable"),
        )
        .arg("--ignored")
        .arg("--exact")
        .arg("engine::store::tests::external_recovery_rebuild_crash_worker")
        .arg("--test-threads=1")
        .env(RECOVERY_REBUILD_EXTERNAL_CRASH_ROOT_ENV, root)
        .env(RECOVERY_REBUILD_HORIZON_EPOCH_ENV, horizon.epoch.to_string())
        .env(
            RECOVERY_REBUILD_HORIZON_SEGMENT_ENV,
            horizon.segment_id.get().to_string(),
        )
        .env(
            RECOVERY_REBUILD_HORIZON_END_ENV,
            horizon.frame_end.to_string(),
        )
        .env(
            RECOVERY_REBUILD_HORIZON_BLOCK_START_ENV,
            horizon.context.block_start.to_string(),
        )
        .env(
            RECOVERY_REBUILD_HORIZON_BLOCK_END_ENV,
            horizon.context.block_end.to_string(),
        )
        .env(
            RECOVERY_REBUILD_HORIZON_PREVIOUS_ROOT_ENV,
            recovery_rebuild_bytes_environment(&horizon.context.previous_root),
        )
        .env(
            RECOVERY_REBUILD_HORIZON_RESULTING_ROOT_ENV,
            recovery_rebuild_bytes_environment(&horizon.context.resulting_root),
        )
        .env(
            RECOVERY_REBUILD_HORIZON_FRAME_DIGEST_ENV,
            recovery_rebuild_bytes_environment(&horizon.frame_sha256),
        )
        .env(crate::engine::failpoint::ENVIRONMENT_VARIABLE, boundary)
        .output()
        .expect("run external recovery-rebuild crash worker");
        assert_eq!(
            output.status.code(),
            Some(crate::engine::failpoint::EXIT_CODE),
            "external failpoint {boundary} was not reached\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    fn assert_recovery_rebuild_sentinels(root: &Path) {
        assert_eq!(
            fs::read(root.join(RECOVERY_REBUILD_ROOT_SENTINEL))
                .expect("read recovery-rebuild root sentinel"),
            RECOVERY_REBUILD_ROOT_SENTINEL_BYTES
        );
        assert_eq!(
            fs::read(root.join("runs").join(RECOVERY_REBUILD_RUNS_SENTINEL))
                .expect("read recovery-rebuild runs sentinel"),
            RECOVERY_REBUILD_RUNS_SENTINEL_BYTES
        );
    }

    fn assert_rebuilt_manifest_binding(
        path: &Path,
        baseline: &RecoveryRebuildCrashBaseline,
    ) {
        let rebuilt = manifest::read_manifest(path).expect("read rebuilt crash manifest");
        assert_eq!(rebuilt.generation, baseline.generation + 1);
        assert_eq!(rebuilt.extents, baseline.manifest_extents);
        assert_eq!(
            rebuilt
                .frame_count()
                .expect("rebuilt crash manifest frame count"),
            baseline.receipt.epoch + 1
        );
        assert_eq!(
            rebuilt
                .entries
                .iter()
                .map(|entry| (entry.level, entry.min_epoch, entry.max_epoch))
                .collect::<Vec<_>>(),
            vec![(1, 0, 1), (0, 2, 2)]
        );
        for entry in &rebuilt.entries {
            assert_eq!(
                entry.file_name,
                run_file_name(entry.level, entry.min_epoch, entry.max_epoch)
            );
            assert!(
                path.parent()
                    .expect("manifest store root")
                    .join("runs")
                    .join(&entry.file_name)
                    .exists(),
                "rebuilt manifest references missing run {}",
                entry.file_name
            );
        }
    }

    fn assert_recovery_rebuild_crash_artifacts(
        root: &Path,
        baseline: &RecoveryRebuildCrashBaseline,
        boundary: RecoveryRebuildCrashBoundary,
    ) {
        let staging = root.join("runs").join(recovery::REBUILD_STAGING_DIRECTORY);
        assert_eq!(
            staging.exists(),
            boundary.leaves_staging,
            "unexpected staging state at {}",
            boundary.name
        );
        let next_generation = baseline
            .generation
            .checked_add(1)
            .expect("recovery manifest generation");
        assert_eq!(
            root.join(format!("manifest-{next_generation:020}.tmp"))
                .exists(),
            boundary.leaves_manifest_temp,
            "unexpected temporary manifest state at {}",
            boundary.name
        );
        assert_eq!(
            root.join(manifest::manifest_file_name(next_generation))
                .exists(),
            boundary.publishes_manifest,
            "unexpected recovered manifest state at {}",
            boundary.name
        );
        if boundary.publishes_manifest {
            assert_rebuilt_manifest_binding(
                &root.join(manifest::manifest_file_name(next_generation)),
                baseline,
            );
        } else {
            assert_eq!(
                fs::read(&baseline.manifest_path)
                    .expect("read pre-publication authoritative manifest"),
                baseline.manifest_bytes,
                "pre-publication crash changed the authoritative manifest at {}",
                boundary.name
            );
        }
        assert_eq!(
            digest(
                &fs::read(root.join(PackSegmentId::INITIAL.file_name()))
                    .expect("read committed segment after recovery crash")
            ),
            baseline.segment_sha256,
            "recovery crash changed committed frame bytes at {}",
            boundary.name
        );
        assert_recovery_rebuild_sentinels(root);
    }

    fn assert_no_recovery_rebuild_artifacts(root: &Path) {
        assert!(
            !root
                .join("runs")
                .join(recovery::REBUILD_STAGING_DIRECTORY)
                .exists(),
            "recovery staging directory survived restart"
        );
        assert_no_temporary_pack_files(root);
        assert_recovery_rebuild_sentinels(root);
    }

    fn assert_recovered_rebuild_state(
        store: &PackStore,
        baseline: &RecoveryRebuildCrashBaseline,
        boundary: &str,
    ) -> PackMaterializedViewEvidence {
        assert_eq!(store.last_frame_receipt(), Some(baseline.receipt));
        assert_eq!(
            store.get(&key(220)).expect("read recovered update"),
            Some(b"v1".to_vec())
        );
        assert_eq!(
            store.get(&key(221)).expect("read recovered tombstone"),
            None
        );
        assert_eq!(
            store.get(&key(222)).expect("read recovered stable value"),
            Some(b"stable".to_vec())
        );
        assert_eq!(
            store.get(&key(223)).expect("read recovered tail value"),
            Some(b"tail".to_vec())
        );
        assert_eq!(
            store.get(&key(225)).expect("read discarded orphan suffix"),
            None
        );
        let evidence = store
            .materialized_view_evidence(64)
            .expect("recovery-rebuild evidence after restart");
        assert!(
            baseline.evidence.state_matches(&evidence),
            "materialized state changed at {boundary}"
        );
        assert_eq!(evidence.generation, baseline.generation + 1);
        assert_eq!(evidence.live_runs, 2);
        store
            .scrub_committed_frames()
            .expect("scrub frames after recovery-rebuild restart");
        store
            .scrub_index_runs()
            .expect("scrub indexes after recovery-rebuild restart");
        evidence
    }

    fn create_crash_fixture(root: &Path) -> CrashBaseline {
        let mut store = PackStore::create(root, store_config(16 * 1024 * 1024))
            .expect("create crash fixture");
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
        let frame_bytes = fs::read(root.join(PackSegmentId::INITIAL.file_name()))
            .expect("read fixture frames");
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
        let mut store = PackStore::open(&root, store_config(16 * 1024 * 1024))
            .expect("open crash fixture");
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

            let frame_bytes = fs::read(root.path().join(PackSegmentId::INITIAL.file_name()))
                .expect("read frames");
            assert_eq!(frame_bytes.len() as u64, baseline.frame_len);
            assert_eq!(digest(&frame_bytes), baseline.frame_sha256);
            let mut recovered = PackStore::open(root.path(), store_config(16 * 1024 * 1024))
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
                if boundary.publishes_manifest { 1 } else { 9 },
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
                gc.runs_deleted, 0,
                "maintenance left superseded source runs after its validated GC at {}",
                boundary.name,
            );
            drop(recovered);

            let reopened = PackStore::open(root.path(), store_config(16 * 1024 * 1024))
                .expect("second reopen after GC");
            let final_evidence = reopened
                .materialized_view_evidence(64)
                .expect("final evidence");
            assert!(baseline.evidence.state_matches(&final_evidence));
            assert_no_temporary_pack_files(root.path());
        }
    }

    #[test]
    #[ignore = "subprocess worker selected by append_rotation_crash_boundaries_recover_prior_horizon"]
    fn append_rotation_crash_worker() {
        let root = std::env::var_os(APPEND_CRASH_ROOT_ENV)
            .map(PathBuf::from)
            .expect("append crash fixture root is set");
        let mut store =
            PackStore::open(&root, append_crash_config()).expect("open append crash fixture");
        let prepared = store
            .prepare_frame(TEST_FRAME_CONTEXT, &append_crash_candidate_operations())
            .expect("prepare append crash candidate");
        let _sealed = store
            .seal_prepared(prepared)
            .expect("seal append crash candidate");
        panic!("configured append crash failpoint was not reached");
    }

    #[test]
    fn append_rotation_crash_boundaries_recover_prior_horizon() {
        for boundary in APPEND_CRASH_BOUNDARIES {
            let root = tempdir().expect("temporary append crash fixture");
            let baseline = create_append_crash_fixture(root.path());
            run_append_crash_worker(root.path(), boundary.name);
            assert_append_crash_artifacts(root.path(), &baseline, boundary);
            assert_append_crash_recovers_prior_horizon(root.path(), &baseline, boundary.name);
        }
    }

    #[test]
    fn append_manifest_crash_reopens_durable_generation() {
        let root = tempdir().expect("temporary append manifest crash fixture");
        let baseline = create_append_crash_fixture(root.path());
        let boundary = APPEND_CRASH_BOUNDARIES
            .iter()
            .copied()
            .find(|boundary| boundary.name == "append.manifest.after-publication")
            .expect("append manifest publication boundary");
        run_append_crash_worker(root.path(), boundary.name);
        assert_append_crash_artifacts(root.path(), &baseline, boundary);

        let reopened = PackStore::open(root.path(), append_crash_config())
            .expect("standalone reopen selects durable append manifest");
        let candidate_receipt = reopened
            .last_frame_receipt()
            .expect("durably published append receipt");
        assert_eq!(candidate_receipt.epoch, baseline.receipt.epoch + 1);
        assert_eq!(candidate_receipt.segment_id, PackSegmentId::new(1));
        assert_eq!(
            reopened
                .get(&key(211))
                .expect("read durably published append candidate"),
            Some(b"candidate".to_vec())
        );
        drop(reopened);

        assert_append_crash_recovers_prior_horizon(
            root.path(),
            &baseline,
            "append.manifest.after-publication.external-prior-horizon",
        );
    }

    #[test]
    #[ignore = "subprocess worker selected by recovery_rebuild_crash_boundaries_are_restartable"]
    fn recovery_rebuild_crash_worker() {
        let root = std::env::var_os(RECOVERY_REBUILD_CRASH_ROOT_ENV)
            .map(PathBuf::from)
            .expect("recovery-rebuild crash fixture root is set");
        let _store = PackStore::open(&root, store_config(16 * 1024 * 1024))
            .expect("open recovery-rebuild crash fixture");
        panic!("configured recovery-rebuild crash failpoint was not reached");
    }

    #[test]
    #[ignore = "subprocess worker selected by external_recovery_rebuild_crash_boundaries_are_restartable"]
    fn external_recovery_rebuild_crash_worker() {
        let root = std::env::var_os(RECOVERY_REBUILD_EXTERNAL_CRASH_ROOT_ENV)
            .map(PathBuf::from)
            .expect("external recovery-rebuild crash fixture root is set");
        let horizon = recovery_rebuild_horizon_from_environment();
        let _store = PackStore::open_at_commit_horizon(
            &root,
            store_config(16 * 1024 * 1024),
            Some(horizon),
        )
        .expect("open external recovery-rebuild crash fixture");
        panic!("configured external recovery-rebuild crash failpoint was not reached");
    }

    #[test]
    fn recovery_rebuild_crash_boundaries_are_restartable() {
        for boundary in RECOVERY_REBUILD_CRASH_BOUNDARIES {
            let root = tempdir().expect("temporary recovery-rebuild crash fixture");
            let baseline = create_recovery_rebuild_crash_fixture(root.path(), false);
            run_recovery_rebuild_crash_worker(root.path(), boundary.name);
            assert_recovery_rebuild_crash_artifacts(root.path(), &baseline, boundary);

            let mut recovered = PackStore::open(root.path(), store_config(16 * 1024 * 1024))
                .unwrap_or_else(|error| panic!("restart after {}: {error:#}", boundary.name));
            let evidence = assert_recovered_rebuild_state(&recovered, &baseline, boundary.name);
            assert_no_recovery_rebuild_artifacts(root.path());

            let manifests = manifest::list_manifest_files(root.path())
                .expect("list manifests after recovery-rebuild restart");
            let (generation, manifest_path) = manifests.first().expect("authoritative manifest");
            assert_eq!(*generation, evidence.generation);
            assert_rebuilt_manifest_binding(manifest_path, &baseline);

            recovered
                .append_frame(
                    TEST_FRAME_CONTEXT,
                    &[put(key(220), b"v2"), put(key(224), b"post-recovery")],
                )
                .unwrap_or_else(|error| panic!("append after {}: {error:#}", boundary.name));
            assert_eq!(
                recovered.get(&key(220)).expect("read post-recovery update"),
                Some(b"v2".to_vec())
            );
            assert_eq!(
                recovered.get(&key(224)).expect("read post-recovery append"),
                Some(b"post-recovery".to_vec())
            );
            drop(recovered);

            let reopened = PackStore::open(root.path(), store_config(16 * 1024 * 1024))
                .unwrap_or_else(|error| panic!("second restart after {}: {error:#}", boundary.name));
            let final_receipt = reopened
                .last_frame_receipt()
                .expect("post-recovery append receipt");
            assert_eq!(final_receipt.epoch, baseline.receipt.epoch + 1);
            assert_eq!(
                reopened
                    .get(&key(220))
                    .expect("reopen post-recovery update"),
                Some(b"v2".to_vec())
            );
            assert_eq!(
                reopened.get(&key(221)).expect("reopen recovered tombstone"),
                None
            );
            assert_eq!(
                reopened
                    .get(&key(224))
                    .expect("reopen post-recovery append"),
                Some(b"post-recovery".to_vec())
            );
            reopened
                .scrub_index_runs()
                .expect("scrub indexes after post-recovery append");
            assert_no_recovery_rebuild_artifacts(root.path());
        }
    }

    #[test]
    fn external_recovery_rebuild_crash_boundaries_are_restartable() {
        for boundary in EXTERNAL_RECOVERY_REBUILD_CRASH_BOUNDARIES {
            let root = tempdir().expect("temporary external recovery-rebuild crash fixture");
            let baseline = create_recovery_rebuild_crash_fixture(root.path(), true);
            let orphan_segment_bytes = baseline
                .orphan_segment_bytes
                .expect("external fixture has an orphan suffix");
            assert!(orphan_segment_bytes > baseline.segment_bytes);

            run_external_recovery_rebuild_crash_worker(
                root.path(),
                baseline.horizon,
                boundary.name,
            );
            assert_recovery_rebuild_crash_artifacts(root.path(), &baseline, boundary);
            assert_eq!(
                fs::metadata(root.path().join(PackSegmentId::INITIAL.file_name()))
                    .expect("stat externally reconciled segment")
                    .len(),
                baseline.segment_bytes,
                "external recovery did not truncate the orphan suffix at {}",
                boundary.name
            );

            let mut recovered = PackStore::open_at_commit_horizon(
                root.path(),
                store_config(16 * 1024 * 1024),
                Some(baseline.horizon),
            )
            .unwrap_or_else(|error| {
                panic!("external restart after {}: {error:#}", boundary.name)
            });
            let evidence = assert_recovered_rebuild_state(&recovered, &baseline, boundary.name);
            assert_no_recovery_rebuild_artifacts(root.path());
            assert!(
                !root.path().join("runs").join(run_file_name(0, 3, 3)).exists(),
                "orphan suffix run survived external restart at {}",
                boundary.name
            );
            let manifests = manifest::list_manifest_files(root.path())
                .expect("list manifests after external recovery-rebuild restart");
            let (generation, manifest_path) = manifests.first().expect("external manifest");
            assert_eq!(*generation, evidence.generation);
            assert_rebuilt_manifest_binding(manifest_path, &baseline);

            recovered
                .append_frame(
                    TEST_FRAME_CONTEXT,
                    &[put(key(220), b"v2"), put(key(226), b"external-recovery")],
                )
                .unwrap_or_else(|error| {
                    panic!("append after external {}: {error:#}", boundary.name)
                });
            let appended = recovered
                .last_frame_receipt()
                .expect("external post-recovery append receipt");
            let appended_horizon = PackCommitHorizon {
                epoch: appended.epoch,
                segment_id: appended.segment_id,
                frame_end: appended.frame_end,
                context: appended.context,
                frame_sha256: appended.frame_sha256,
            };
            drop(recovered);

            let reopened = PackStore::open_at_commit_horizon(
                root.path(),
                store_config(16 * 1024 * 1024),
                Some(appended_horizon),
            )
            .unwrap_or_else(|error| {
                panic!("second external restart after {}: {error:#}", boundary.name)
            });
            assert_eq!(reopened.last_frame_receipt(), Some(appended));
            assert_eq!(
                reopened
                    .get(&key(226))
                    .expect("reopen external post-recovery append"),
                Some(b"external-recovery".to_vec())
            );
            assert_eq!(
                reopened.get(&key(225)).expect("reopen discarded orphan"),
                None
            );
            reopened
                .scrub_index_runs()
                .expect("scrub external indexes after post-recovery append");
            assert_no_recovery_rebuild_artifacts(root.path());
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
