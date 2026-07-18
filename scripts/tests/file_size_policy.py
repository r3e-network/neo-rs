import os
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
REVIEW_BUDGET_LINES = 900

# These are exact no-growth ceilings, not approved target sizes. Phase 7 must
# split the remaining exceptions before RELEASE-01 can be accepted. A decrease
# intentionally fails until the ceiling is ratcheted down or removed.
RUST_SIZE_BASELINE = {
    "neo-blockchain/src/tests/ledger/ledger_provider.rs": 972,
    "neo-blockchain/src/tests/pipeline/handlers.rs": 1125,
    "neo-blockchain/src/tests/pipeline/handlers/block_flow.rs": 3070,
    "neo-blockchain/src/tests/pipeline/native_persist.rs": 1075,
    "neo-crypto/src/mpt_trie/cache.rs": 1114,
    "neo-crypto/src/mpt_trie/trie.rs": 1154,
    "neo-execution/src/tests/application_engine/contracts.rs": 1870,
    "neo-native-contracts/src/tests/std_lib/mod.rs": 956,
    "neo-node/src/bin/neo-db-probe.rs": 1503,
    "neo-node/src/tests/bin/neo_db_probe.rs": 940,
    "neo-node/src/tests/node/chain_acc/mod.rs": 1374,
    "neo-node/src/tests/node/fast_sync/mod.rs": 1009,
    "neo-node/src/tests/node/runtime.rs": 2132,
    "neo-runtime/src/service/sync_metrics.rs": 952,
    "neo-state-service/src/service/commit_handlers.rs": 944,
    "neo-state-service/src/storage/mpt_store.rs": 1957,
    "neo-state-service/src/tests/storage/mpt_store.rs": 2568,
    "neo-storage/src/mdbx/store.rs": 1246,
    "neo-storage/src/tests/mdbx/mod.rs": 1074,
    "tests/tests/architecture/layer_boundary_tests.rs": 1415,
}

OPERATIONAL_PYTHON_SIZE_BASELINE = {
    "scripts/analyze-stateroot-milestone-history.py": 1032,
    "scripts/maintain-stateroot-checkpoints.py": 1040,
    "scripts/run-bounded-mainnet-replay.py": 1654,
    "scripts/run-stateroot-milestones.py": 1918,
}

PYTHON_TEST_SIZE_BASELINE = {
    "scripts/tests/test_analyze_stateroot_milestone_history.py": 1136,
    "scripts/tests/test_maintain_stateroot_checkpoints.py": 1119,
    "scripts/tests/test_run_bounded_replay.py": 1927,
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
