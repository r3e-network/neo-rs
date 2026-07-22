import os
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
REVIEW_BUDGET_LINES = 900

# These are exact no-growth ceilings, not approved target sizes. The catalog is
# normalized after the architecture migration so every remaining entry is
# established debt. A decrease intentionally fails until the ceiling is
# ratcheted down or removed.
RUST_SIZE_BASELINE = {
    "benches-package/src/append_benchmark/runner.rs": 926,
    "benches-package/src/mdbx_benchmark/mod.rs": 1033,
    "benches-package/src/mdbx_benchmark/runner.rs": 1481,
    "neo-blockchain/src/pipeline/signature_verification/mod.rs": 1048,
    "neo-blockchain/src/tests/ledger/ledger_provider.rs": 972,
    "neo-blockchain/src/tests/pipeline/handlers.rs": 1396,
    "neo-blockchain/src/tests/pipeline/handlers/block_flow.rs": 3389,
    "neo-blockchain/src/tests/pipeline/native_persist.rs": 1287,
    "neo-execution/src/application_engine/acceleration/shadow.rs": 982,
    "neo-execution/src/application_engine/state.rs": 1049,
    "neo-execution/src/host_access_audit/mod.rs": 901,
    "neo-execution/src/interop/application_engine_runtime.rs": 934,
    "neo-execution/src/optimistic_execution/application.rs": 1339,
    "neo-execution/src/optimistic_execution/artifact.rs": 1598,
    "neo-execution/src/tests/application_engine/contracts.rs": 1870,
    "neo-execution/src/tests/application_engine/shadow.rs": 1026,
    "neo-execution/src/tests/execution_artifact/mod.rs": 918,
    "neo-native-contracts/src/tests/std_lib/mod.rs": 956,
    "neo-node/src/bin/neo-db-probe.rs": 1651,
    "neo-node/src/bin/neo-pack-build.rs": 1571,
    "neo-node/src/node/append_shadow/mod.rs": 956,
    "neo-node/src/node/state_packs/authority.rs": 2030,
    "neo-node/src/tests/bin/neo_db_probe.rs": 1057,
    "neo-node/src/tests/node/chain_acc/mod.rs": 1407,
    "neo-node/src/tests/node/config_validation.rs": 942,
    "neo-node/src/tests/node/fast_sync/mod.rs": 1009,
    "neo-node/src/tests/node/runtime.rs": 2200,
    "neo-runtime/src/service/sync_metrics.rs": 952,
    "neo-state-packs/src/engine/manifest.rs": 1189,
    "neo-state-packs/src/engine/store/format/frame_codec.rs": 1661,
    "neo-state-packs/src/engine/store/lifecycle/recovery.rs": 1666,
    "neo-state-packs/src/engine/store/tests/compaction_recovery_tests.rs": 2097,
    "neo-state-packs/src/engine/store/tests/crash_failpoint_tests.rs": 1269,
    "neo-state-packs/src/engine/store/tests/mod.rs": 1518,
    "neo-state-packs/src/engine/store/validation/evidence.rs": 923,
    "neo-state-packs/src/shadow/mod.rs": 1022,
    "neo-state-service/src/service/commit_handlers.rs": 987,
    "neo-state-service/src/storage/mpt_store.rs": 2889,
    "neo-state-service/src/tests/storage/mpt_store.rs": 3212,
    "neo-storage/src/mdbx/store.rs": 1947,
    "neo-storage/src/persistence/data_cache/cache.rs": 1247,
    "neo-storage/src/tests/mdbx/mod.rs": 1649,
}

OPERATIONAL_PYTHON_SIZE_BASELINE = {
    "scripts/analyze-stateroot-milestone-history.py": 1032,
    "scripts/maintain-stateroot-checkpoints.py": 1040,
    "scripts/run-bounded-mainnet-replay.py": 1539,
    "scripts/run-stateroot-milestones.py": 1918,
}

PYTHON_TEST_SIZE_BASELINE = {
    "scripts/tests/test_analyze_stateroot_milestone_history.py": 1136,
    "scripts/tests/test_maintain_stateroot_checkpoints.py": 1119,
    "scripts/tests/test_run_bounded_replay.py": 1869,
    "scripts/tests/test_run_stateroot_milestones.py": 3058,
}

EXCLUDED_DIRECTORY_NAMES = {".git", ".idea", ".vscode", "logs", "target"}


def rust_source_files(root: Path = REPO_ROOT) -> list[Path]:
    paths = []
    for directory, child_dirs, filenames in os.walk(root):
        child_dirs[:] = [
            name for name in child_dirs if name not in EXCLUDED_DIRECTORY_NAMES
        ]
        for filename in filenames:
            if filename.endswith(".rs"):
                paths.append(Path(directory) / filename)
    return sorted(paths)


def line_count(path: Path) -> int:
    return len(path.read_text(encoding="utf-8").splitlines())


def relative_path(path: Path) -> str:
    return path.relative_to(REPO_ROOT).as_posix()


def baseline_for_paths(paths: list[Path], baseline: dict[str, int]) -> dict[str, int]:
    relatives = {relative_path(path) for path in paths}
    return {path: limit for path, limit in baseline.items() if path in relatives}


def assert_line_budget(
    test_case,
    paths: list[Path],
    baseline: dict[str, int],
    *,
    minimum_files: int,
    default_limit: int = REVIEW_BUDGET_LINES,
) -> None:
    paths = sorted(set(paths))
    test_case.assertGreaterEqual(
        len(paths), minimum_files, "source inventory is unexpectedly small"
    )

    relative_paths = {relative_path(path) for path in paths}
    test_case.assertEqual(
        sorted(set(baseline) - relative_paths),
        [],
        "file-size baseline contains stale or moved paths",
    )

    for path in paths:
        relative = relative_path(path)
        actual = line_count(path)
        with test_case.subTest(path=relative):
            if relative in baseline:
                expected = baseline[relative]
                test_case.assertGreater(
                    expected,
                    default_limit,
                    "baseline exceptions at or below the normal budget must be removed",
                )
                test_case.assertEqual(
                    actual,
                    expected,
                    "baseline exception changed; ratchet its exact ceiling down or split it",
                )
            else:
                test_case.assertLessEqual(
                    actual,
                    default_limit,
                    f"{relative} exceeds the {default_limit}-line review budget; "
                    "split it instead of adding an untracked exception",
                )


def oversized_files(paths: list[Path], limit: int = REVIEW_BUDGET_LINES) -> dict[str, int]:
    return {
        relative_path(path): line_count(path)
        for path in paths
        if line_count(path) > limit
    }
