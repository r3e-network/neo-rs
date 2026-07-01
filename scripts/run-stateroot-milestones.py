#!/usr/bin/env python3
"""Run StateRoot validation milestones and checkpoint each successful height."""

from __future__ import annotations

import argparse
import importlib.util
import json
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable


DEFAULT_NODE_BIN = "target/debug/neo-node"
DEFAULT_PROBE_BIN = "target/debug/neo-db-probe"
DEFAULT_RPC = "http://127.0.0.1:21332"
DEFAULT_STORAGE_PROVIDER = "mdbx"
DEFAULT_RESTORE_SCRIPT = "scripts/restore-checkpoint.sh"
DEFAULT_SYNC_SPEED_FLOOR_BPS = 1500.0
DEFAULT_SYNC_SPEED_CEILING_BPS = 2000.0
DEFAULT_MINIMUM_CHECKPOINT_COUNT = 3
DEFAULT_MINIMUM_TRANSACTION_BLOCKS = 1000
DEFAULT_REFERENCE_RPCS = [
    "http://seed1.neo.org:10332",
    "http://seed2.neo.org:10332",
    "http://seed3.neo.org:10332",
    "http://seed4.neo.org:10332",
    "http://seed5.neo.org:10332",
]
CHECKPOINT_VERIFICATION_FIELDS = (
    "restore_verified",
    "verified_height",
    "verified_stateroot_root",
    "verified_against_reference",
)
BOUNDED_REPLAY_MODULE_PATH = Path(__file__).resolve().with_name(
    "run-bounded-mainnet-replay.py"
)
_BOUNDED_REPLAY_MODULE = None


def bounded_replay_module():
    global _BOUNDED_REPLAY_MODULE  # pylint: disable=global-statement
    if _BOUNDED_REPLAY_MODULE is None:
        spec = importlib.util.spec_from_file_location(
            "bounded_mainnet_replay_helpers",
            BOUNDED_REPLAY_MODULE_PATH,
        )
        if spec is None or spec.loader is None:
            raise ImportError(f"unable to load {BOUNDED_REPLAY_MODULE_PATH}")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        _BOUNDED_REPLAY_MODULE = module
    return _BOUNDED_REPLAY_MODULE


def parse_height_values(values: list[str]) -> list[int]:
    heights: list[int] = []
    for value in values:
        for part in str(value).split(","):
            part = part.strip()
            if not part:
                continue
            try:
                height = int(part, 10)
            except ValueError as exc:
                raise ValueError(f"invalid milestone height: {part}") from exc
            if height < 0:
                raise ValueError(f"milestone height must be >= 0: {height}")
            heights.append(height)
    if not heights:
        raise ValueError("at least one --milestone height is required")
    if heights != sorted(set(heights)):
        raise ValueError("milestone heights must be unique and strictly increasing")
    return heights


def parse_last_json_object(text: str) -> dict:
    decoder = json.JSONDecoder()
    last: dict | None = None
    for index, char in enumerate(text):
        if char != "{":
            continue
        try:
            candidate, end = decoder.raw_decode(text[index:])
        except json.JSONDecodeError:
            continue
        if isinstance(candidate, dict) and not text[index + end :].strip():
            last = candidate
            break
        if isinstance(candidate, dict):
            last = candidate
    if last is None:
        raise ValueError("command output did not contain a JSON object")
    return last


def normalize_reference_urls(values: list[str] | None) -> list[str]:
    urls: list[str] = []
    for value in values or []:
        for part in str(value).split(","):
            part = part.strip()
            if part:
                urls.append(part)
    return urls


def bounded_command(
    *,
    config: Path,
    node_bin: Path,
    rpc_url: str,
    target_height: int,
    poll_interval: float,
    max_seconds: float,
    chain_db: Path,
    stateroot_db: Path,
    probe_bin: Path,
    references: list[str],
    node_output_log: Path,
    storage_provider: str = DEFAULT_STORAGE_PROVIDER,
    metrics_url: str | None = None,
    sync_speed_floor_bps: float | None = DEFAULT_SYNC_SPEED_FLOOR_BPS,
    sync_speed_ceiling_bps: float | None = DEFAULT_SYNC_SPEED_CEILING_BPS,
    fast_sync: bool = False,
    fast_sync_cache: Path | None = None,
    initial_height: int | None = None,
    require_metrics_samples: bool = True,
) -> list[str]:
    command = [
        "python3",
        "scripts/run-bounded-mainnet-replay.py",
        "--config",
        str(config),
        "--node-bin",
        str(node_bin),
        "--rpc",
        rpc_url,
        "--target-height",
        str(target_height),
        "--poll-interval",
        str(poll_interval),
        "--max-seconds",
        str(max_seconds),
        "--db",
        str(chain_db),
        "--stateroot-db",
        str(stateroot_db),
        "--probe-bin",
        str(probe_bin),
        "--storage-provider",
        storage_provider,
        "--require-stateroot-height-match",
    ]
    if fast_sync:
        command.append("--fast-sync")
        if fast_sync_cache is not None:
            command.extend(["--fast-sync-cache", str(fast_sync_cache)])
    if initial_height is not None:
        command.extend(["--initial-height", str(initial_height)])
    if sync_speed_floor_bps is not None:
        command.extend(["--sync-speed-floor-bps", str(sync_speed_floor_bps)])
    if sync_speed_ceiling_bps is not None:
        command.extend(["--sync-speed-ceiling-bps", str(sync_speed_ceiling_bps)])
    if references:
        command.extend(
            [
                "--reference",
                ",".join(references),
                "--require-reference-stateroot-match",
            ]
        )
    if metrics_url:
        command.extend(["--metrics-url", metrics_url])
        if require_metrics_samples:
            command.append("--require-metrics-samples")
    command.extend(["--node-output-log", str(node_output_log)])
    return command


def checkpoint_command(
    *,
    height: int,
    data_dir: Path,
    chain_db: Path,
    stateroot_db: Path,
    checkpoint_root: Path,
    script: Path,
    storage_provider: str = DEFAULT_STORAGE_PROVIDER,
) -> list[str]:
    return [
        str(script),
        "none",
        "--once",
        "--height",
        str(height),
        "--data-dir",
        str(data_dir),
        "--chain-db",
        str(chain_db),
        "--stateroot-db",
        str(stateroot_db),
        "--storage-provider",
        storage_provider,
        "--root",
        str(checkpoint_root),
    ]


def checkpoint_restore_root(checkpoint_root: Path, height: int) -> Path:
    return checkpoint_root / ".restore-probe" / f"h{height}"


def checkpoint_restore_command(
    *,
    height: int,
    checkpoint_root: Path,
    script: Path,
) -> list[str]:
    restore_root = checkpoint_restore_root(checkpoint_root, height)
    return [
        str(script),
        str(height),
        "--root",
        str(checkpoint_root),
        "--chain-db",
        str(restore_root / "mainnet"),
        "--stateroot-db",
        str(restore_root / "StateRoot"),
        "--yes",
    ]


def checkpoint_plan_summary(milestones: list[int], minimum_checkpoint_count: int) -> dict:
    planned_count = len(milestones)
    return {
        "planned_checkpoint_count": planned_count,
        "minimum_checkpoint_count": minimum_checkpoint_count,
        "minimum_checkpoint_count_met": planned_count >= minimum_checkpoint_count,
        "missing_checkpoint_count": max(minimum_checkpoint_count - planned_count, 0),
    }


def checkpoint_metadata_value(path: Path, key: str) -> str | None:
    info = path / "CHECKPOINT_INFO"
    if not info.exists():
        return None
    for line in info.read_text(encoding="utf-8").splitlines():
        if line.startswith(f"{key}="):
            return line.split("=", 1)[1]
    return None


def checkpoint_metadata(path: Path) -> dict[str, str]:
    info = path / "CHECKPOINT_INFO"
    if not info.exists():
        return {}
    fields: dict[str, str] = {}
    for line in info.read_text(encoding="utf-8").splitlines():
        key, separator, value = line.partition("=")
        if separator:
            fields[key.strip()] = value.strip()
    return fields


def run_probe_json(command: list[str], runner: Callable[..., Any] = subprocess.run) -> dict:
    completed = runner(
        command,
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    return json.loads(completed.stdout)


def probe_command_prefix(probe_bin: Path, db_path: Path, storage_provider: str) -> list[str]:
    return [
        str(probe_bin),
        "--db",
        str(db_path),
        "--storage-provider",
        storage_provider,
    ]


def read_probe_ledger_height(
    db_path: Path,
    probe_bin: Path,
    runner: Callable[..., Any] = subprocess.run,
    *,
    storage_provider: str = DEFAULT_STORAGE_PROVIDER,
) -> int | None:
    payload = run_probe_json(
        [
            *probe_command_prefix(probe_bin, db_path, storage_provider),
            "--contract-id",
            "-4",
            "--key-hex",
            "0c",
            "--decode",
            "hash-index",
        ],
        runner,
    )
    decoded = payload.get("decoded") or {}
    if "index" not in decoded:
        return None
    return int(decoded["index"])


def read_probe_mpt_state_height(
    db_path: Path,
    probe_bin: Path,
    runner: Callable[..., Any] = subprocess.run,
    *,
    storage_provider: str = DEFAULT_STORAGE_PROVIDER,
) -> int | None:
    payload = run_probe_json(
        [
            *probe_command_prefix(probe_bin, db_path, storage_provider),
            "--mpt-state-height",
        ],
        runner,
    )
    height = payload.get("height") or {}
    decoded = height.get("decoded") or {}
    if "current_local_root_index" not in decoded:
        return None
    return int(decoded["current_local_root_index"])


def read_probe_mpt_state_root(
    db_path: Path,
    probe_bin: Path,
    index: int,
    runner: Callable[..., Any] = subprocess.run,
    *,
    storage_provider: str = DEFAULT_STORAGE_PROVIDER,
) -> str | None:
    payload = run_probe_json(
        [
            *probe_command_prefix(probe_bin, db_path, storage_provider),
            "--mpt-state-root",
            str(index),
        ],
        runner,
    )
    state_root = payload.get("state_root") or {}
    decoded = state_root.get("decoded") or {}
    return decoded.get("roothash")


def checkpoint_content_verification_reason(
    path: Path,
    *,
    expected_verified_height: int | None,
    expected_verified_stateroot_root: str | None,
    probe_bin: Path | None,
    storage_provider: str = DEFAULT_STORAGE_PROVIDER,
    probe_runner: Callable[..., Any],
) -> str | None:
    if probe_bin is None or expected_verified_height is None:
        return None
    metadata = checkpoint_metadata(path)
    effective_storage_provider = metadata.get("storage_provider") or storage_provider
    chain_db = path / "mainnet"
    stateroot_db = path / "StateRoot"
    try:
        chain_height = read_probe_ledger_height(
            chain_db,
            probe_bin,
            probe_runner,
            storage_provider=effective_storage_provider,
        )
    except Exception as exc:  # pylint: disable=broad-except
        return f"checkpoint chain database probe failed: {exc}"
    if chain_height != expected_verified_height:
        return (
            "checkpoint chain database height does not match expected verified height: "
            f"height={chain_height}, expected={expected_verified_height}"
        )
    try:
        stateroot_height = read_probe_mpt_state_height(
            stateroot_db,
            probe_bin,
            probe_runner,
            storage_provider=effective_storage_provider,
        )
    except Exception as exc:  # pylint: disable=broad-except
        return f"checkpoint StateRoot database height probe failed: {exc}"
    if stateroot_height != expected_verified_height:
        return (
            "checkpoint StateRoot database height does not match expected verified height: "
            f"height={stateroot_height}, expected={expected_verified_height}"
        )
    if expected_verified_stateroot_root is None:
        return None
    try:
        stateroot_root = read_probe_mpt_state_root(
            stateroot_db,
            probe_bin,
            expected_verified_height,
            probe_runner,
            storage_provider=effective_storage_provider,
        )
    except Exception as exc:  # pylint: disable=broad-except
        return f"checkpoint StateRoot root probe failed: {exc}"
    if (
        stateroot_root is None
        or stateroot_root.lower() != expected_verified_stateroot_root.lower()
    ):
        return (
            "checkpoint StateRoot root does not match expected verified root: "
            f"root={stateroot_root}, expected={expected_verified_stateroot_root}"
        )
    return None


def checkpoint_verification_reason(
    path: Path,
    *,
    expected_verified_height: int | None = None,
    expected_verified_stateroot_root: str | None = None,
    expected_verified_against_reference: bool | None = None,
) -> str | None:
    metadata = checkpoint_metadata(path)
    missing = [field for field in CHECKPOINT_VERIFICATION_FIELDS if not metadata.get(field)]
    if missing:
        return "missing restore verification metadata: " + ", ".join(missing)

    height_text = metadata.get("height")
    verified_height_text = metadata.get("verified_height")
    if height_text is not None and verified_height_text != height_text:
        return (
            "restore verification height does not match checkpoint height: "
            f"height={height_text}, verified_height={verified_height_text}"
        )
    if metadata["restore_verified"].lower() != "true":
        return "restore verification metadata is not marked restore_verified=true"
    if metadata["verified_against_reference"].lower() != "true":
        return "restore verification metadata is not marked verified_against_reference=true"
    if expected_verified_height is not None:
        expected_height_text = str(expected_verified_height)
        if height_text != expected_height_text:
            return (
                "checkpoint height does not match expected verified height: "
                f"height={height_text}, expected={expected_height_text}"
            )
        if verified_height_text != expected_height_text:
            return (
                "verified_height does not match expected height: "
                f"verified_height={verified_height_text}, expected={expected_height_text}"
            )
    if expected_verified_stateroot_root is not None:
        actual_root = metadata.get("verified_stateroot_root", "")
        if actual_root.lower() != expected_verified_stateroot_root.lower():
            return (
                "verified_stateroot_root does not match expected root: "
                f"verified_stateroot_root={actual_root}, "
                f"expected={expected_verified_stateroot_root}"
            )
    if (
        expected_verified_against_reference is not None
        and (metadata["verified_against_reference"].lower() == "true")
        != expected_verified_against_reference
    ):
        return (
            "verified_against_reference does not match expected value: "
            f"verified_against_reference={metadata['verified_against_reference']}, "
            f"expected={str(expected_verified_against_reference).lower()}"
        )
    return None


def checkpoint_inventory(
    path: Path,
    *,
    expected_verified_height: int | None = None,
    expected_verified_stateroot_root: str | None = None,
    expected_verified_against_reference: bool | None = None,
    probe_bin: Path | None = None,
    storage_provider: str = DEFAULT_STORAGE_PROVIDER,
    probe_runner: Callable[..., Any] = subprocess.run,
    restore_roundtrip_verified: bool | None = None,
) -> dict[str, Any]:
    has_chain = (path / "mainnet").is_dir()
    has_stateroot = (path / "StateRoot").is_dir()
    if checkpoint_metadata_value(path, "state_root_included") == "false":
        has_stateroot = False
    verification_reason = checkpoint_verification_reason(
        path,
        expected_verified_height=expected_verified_height,
        expected_verified_stateroot_root=expected_verified_stateroot_root,
        expected_verified_against_reference=expected_verified_against_reference,
    )
    content_verification_reason = None
    if verification_reason is None and has_chain and has_stateroot:
        content_verification_reason = checkpoint_content_verification_reason(
            path,
            expected_verified_height=expected_verified_height,
            expected_verified_stateroot_root=expected_verified_stateroot_root,
            probe_bin=probe_bin,
            storage_provider=storage_provider,
            probe_runner=probe_runner,
        )
    usable_for_state_validation = bool(
        path.is_dir()
        and (path / "CHECKPOINT_INFO").is_file()
        and not (path / "CHECKPOINT_IN_PROGRESS").exists()
        and has_chain
        and has_stateroot
        and verification_reason is None
        and content_verification_reason is None
        and restore_roundtrip_verified is not False
    )
    restore_verification_reason = None
    if restore_roundtrip_verified is False:
        restore_verification_reason = "checkpoint has not passed restore roundtrip verification"
    reason = None if usable_for_state_validation else checkpoint_inventory_reason(
        path,
        has_chain=has_chain,
        has_stateroot=has_stateroot,
        verification_reason=(
            verification_reason
            or content_verification_reason
            or restore_verification_reason
        ),
    )
    return {
        "path": str(path),
        "exists": path.is_dir(),
        "storage_provider": checkpoint_metadata(path).get(
            "storage_provider",
            storage_provider,
        ),
        "has_checkpoint_info": (path / "CHECKPOINT_INFO").is_file(),
        "in_progress": (path / "CHECKPOINT_IN_PROGRESS").exists(),
        "has_chain": has_chain,
        "has_stateroot": has_stateroot,
        "restore_roundtrip_verified": restore_roundtrip_verified is True,
        "usable_for_state_validation": usable_for_state_validation,
        "reason": reason,
    }


def retained_checkpoint_inventory(
    checkpoint_root: Path,
    milestones: list[int],
    *,
    expected_roots: dict[int, str] | None = None,
    restore_roundtrip_by_height: dict[int, bool] | None = None,
    probe_bin: Path | None = None,
    storage_provider: str = DEFAULT_STORAGE_PROVIDER,
    probe_runner: Callable[..., Any] = subprocess.run,
) -> dict[str, Any]:
    retained = []
    expected_roots = expected_roots or {}
    restore_roundtrip_by_height = restore_roundtrip_by_height or {}
    for height in milestones:
        inventory = checkpoint_inventory(
            checkpoint_root / f"h{height}",
            expected_verified_height=height,
            expected_verified_stateroot_root=expected_roots.get(height),
            expected_verified_against_reference=True,
            probe_bin=probe_bin,
            storage_provider=storage_provider,
            probe_runner=probe_runner,
            restore_roundtrip_verified=restore_roundtrip_by_height.get(height, False),
        )
        if inventory["usable_for_state_validation"]:
            retained.append(inventory)
    return {
        "retained_usable_checkpoint_count": len(retained),
        "retained_missing_checkpoint_count": max(len(milestones) - len(retained), 0),
        "retained_usable_checkpoints": retained,
    }


def checkpoint_inventory_reason(
    path: Path,
    *,
    has_chain: bool,
    has_stateroot: bool,
    verification_reason: str | None,
) -> str | None:
    if not path.is_dir():
        return "checkpoint directory is missing"
    if not (path / "CHECKPOINT_INFO").is_file():
        return "missing CHECKPOINT_INFO"
    if (path / "CHECKPOINT_IN_PROGRESS").exists():
        return "checkpoint is still in progress"
    if not has_chain:
        return "missing chain database snapshot"
    if not has_stateroot:
        return "missing StateRoot database snapshot"
    return verification_reason


def build_plan(
    *,
    config: Path,
    node_bin: Path,
    rpc_url: str,
    milestones: list[int],
    poll_interval: float,
    max_seconds: float,
    chain_db: Path,
    stateroot_db: Path,
    probe_bin: Path,
    references: list[str],
    data_dir: Path,
    checkpoint_root: Path,
    checkpoint_script: Path,
    log_dir: Path,
    restore_script: Path = Path(DEFAULT_RESTORE_SCRIPT),
    storage_provider: str = DEFAULT_STORAGE_PROVIDER,
    metrics_url: str | None = None,
    sync_speed_floor_bps: float | None = DEFAULT_SYNC_SPEED_FLOOR_BPS,
    sync_speed_ceiling_bps: float | None = DEFAULT_SYNC_SPEED_CEILING_BPS,
    minimum_checkpoint_count: int = DEFAULT_MINIMUM_CHECKPOINT_COUNT,
    minimum_transaction_blocks: int = DEFAULT_MINIMUM_TRANSACTION_BLOCKS,
    fast_sync: bool = False,
    fast_sync_cache: Path | None = None,
    initial_height: int | None = None,
) -> dict:
    steps = []
    for height in milestones:
        is_first_fast_sync_step = fast_sync and height == milestones[0]
        steps.append(
            {
                "height": height,
                "bounded_command": bounded_command(
                    config=config,
                    node_bin=node_bin,
                    rpc_url=rpc_url,
                    target_height=height,
                    poll_interval=1.0 if is_first_fast_sync_step else poll_interval,
                    max_seconds=max_seconds,
                    chain_db=chain_db,
                    stateroot_db=stateroot_db,
                    probe_bin=probe_bin,
                    storage_provider=storage_provider,
                    references=references,
                    node_output_log=log_dir / f"neo-node-milestone-h{height}.log",
                    metrics_url=metrics_url,
                    sync_speed_floor_bps=sync_speed_floor_bps,
                    sync_speed_ceiling_bps=sync_speed_ceiling_bps,
                    fast_sync=is_first_fast_sync_step,
                    fast_sync_cache=(fast_sync_cache if is_first_fast_sync_step else None),
                    initial_height=initial_height if is_first_fast_sync_step else None,
                    require_metrics_samples=metrics_url is not None,
                ),
                "checkpoint_command": checkpoint_command(
                    height=height,
                    data_dir=data_dir,
                    chain_db=chain_db,
                    stateroot_db=stateroot_db,
                    checkpoint_root=checkpoint_root,
                    script=checkpoint_script,
                    storage_provider=storage_provider,
                ),
                "checkpoint_restore_command": checkpoint_restore_command(
                    height=height,
                    checkpoint_root=checkpoint_root,
                    script=restore_script,
                ),
            }
        )
    return {
        "mode": "dry-run",
        "config": str(config),
        "node_bin": str(node_bin),
        "probe_bin": str(probe_bin),
        "storage_provider": storage_provider,
        "chain_db": str(chain_db),
        "stateroot_db": str(stateroot_db),
        "checkpoint_root": str(checkpoint_root),
        "restore_script": str(restore_script),
        "milestones": milestones,
        "reference_rpcs": references,
        "metrics_url": metrics_url,
        "fast_sync": fast_sync,
        "fast_sync_cache": str(fast_sync_cache) if fast_sync_cache is not None else None,
        "initial_height": initial_height,
        "sync_speed_floor_blocks_per_second": sync_speed_floor_bps,
        "sync_speed_ceiling_blocks_per_second": sync_speed_ceiling_bps,
        "minimum_checkpoint_count": minimum_checkpoint_count,
        "minimum_transaction_blocks_for_speed_proof": minimum_transaction_blocks,
        "checkpoint_plan": checkpoint_plan_summary(milestones, minimum_checkpoint_count),
        "steps": steps,
    }


def run_command(command: list[str], runner: Callable[..., Any]) -> Any:
    return runner(
        command,
        check=False,
        capture_output=True,
        text=True,
    )


def height_sample_rate_summary(report: dict) -> dict:
    return bounded_replay_module().height_sample_rate_summary(report)


def metrics_sample_summary(report: dict) -> dict:
    return bounded_replay_module().summarize_metric_samples(
        [sample for sample in report.get("height_samples") or [] if isinstance(sample, dict)]
    )


def transaction_work_summary(report: dict) -> dict:
    summary = report.get("transaction_work_summary")
    if isinstance(summary, dict):
        return summary
    sync_proof = report.get("sync_proof")
    if isinstance(sync_proof, dict):
        proof_summary = sync_proof.get("transaction_work_summary")
        if isinstance(proof_summary, dict):
            return proof_summary
    return bounded_replay_module().transaction_work_summary(metrics_sample_summary(report))


def milestone_summary(result: dict) -> dict:
    report = result.get("bounded_report") or {}
    post_probe = report.get("post_probe") or {}
    root = post_probe.get("stateroot_root") or {}
    reference = post_probe.get("reference_stateroot") or {}
    checkpoint = result.get("checkpoint_inventory") or {}
    speed_proof = speed_proof_summary(report)
    empty_speed_proof = empty_block_speed_proof(report)
    return {
        "height": result.get("height"),
        "status": report.get("status"),
        "last_height": report.get("last_height"),
        "blocks_per_second": report.get("blocks_per_second", 0.0),
        "elapsed_seconds": report.get("elapsed_seconds", 0.0),
        "speed_proof_source": speed_proof["source"],
        "import_window_blocks_per_second": speed_proof["import_window_blocks_per_second"],
        "replay_window_blocks_per_second": speed_proof["replay_window_blocks_per_second"],
        "empty_block_speed_proof_source": (
            empty_speed_proof["source"] if empty_speed_proof else None
        ),
        "empty_block_blocks_per_second": (
            empty_speed_proof["blocks_per_second"] if empty_speed_proof else None
        ),
        "empty_only_blocks": empty_speed_proof["empty_blocks"] if empty_speed_proof else None,
        "empty_block_import_seconds": (
            empty_speed_proof["empty_block_import_seconds"] if empty_speed_proof else None
        ),
        "empty_block_speed_proof_error": empty_block_speed_proof_error(report),
        "stateroot_matches_chain": post_probe.get("stateroot_matches_chain"),
        "reference_matches_local": reference.get("matches_local"),
        "successful_reference_samples": reference.get("successful_samples", 0),
        "local_root": root.get("root"),
        "checkpoint_created": checkpoint.get("usable_for_state_validation") is True,
        "checkpoint_path": checkpoint.get("path"),
        "sync_proof": report.get("sync_proof"),
        "height_sample_rate_summary": height_sample_rate_summary(report),
        "metrics_sample_summary": metrics_sample_summary(report),
        "transaction_work_summary": transaction_work_summary(report),
    }


def post_probe_height_error(
    post_probe: dict[str, Any],
    *,
    expected_height: int,
    references: list[str],
) -> str | None:
    checks = [
        ("chain_height", (post_probe.get("chain_height") or {}).get("height")),
        ("stateroot_height", (post_probe.get("stateroot_height") or {}).get("height")),
        ("stateroot_root", (post_probe.get("stateroot_root") or {}).get("height")),
    ]
    if references:
        checks.append(
            (
                "reference_stateroot",
                (post_probe.get("reference_stateroot") or {}).get("index"),
            )
        )
    for name, value in checks:
        try:
            actual_height = int(value)
        except (TypeError, ValueError):
            return f"{name} height proof is missing"
        if actual_height != expected_height:
            return (
                f"{name} height does not match milestone: "
                f"{actual_height} != {expected_height}"
            )
    return None


def post_probe_checkpoint_error(
    report: dict | None,
    *,
    references: list[str],
    expected_height: int | None = None,
) -> str | None:
    if not isinstance(report, dict):
        return "missing-bounded-report"
    post_probe = report.get("post_probe")
    if not isinstance(post_probe, dict):
        return "missing-post-probe"
    if post_probe.get("stateroot_matches_chain") is not True:
        return "stateroot-mismatch"
    stateroot_root = post_probe.get("stateroot_root")
    if not isinstance(stateroot_root, dict) or not stateroot_root.get("root"):
        return "missing-stateroot-root"
    if expected_height is not None:
        height_error = post_probe_height_error(
            post_probe,
            expected_height=expected_height,
            references=references,
        )
        if height_error is not None:
            return height_error
    if references:
        reference = post_probe.get("reference_stateroot")
        if not isinstance(reference, dict):
            return "missing-reference-stateroot"
        if reference.get("matches_local") is not True:
            return "reference-stateroot-mismatch"
        try:
            successful_samples = int(reference.get("successful_samples"))
        except (TypeError, ValueError):
            return "missing-reference-samples"
        if successful_samples <= 0:
            return "missing-reference-samples"
        try:
            sample_count = int(reference.get("sample_count"))
        except (TypeError, ValueError):
            return "missing-reference-samples"
        if sample_count < len(references):
            return (
                "reference sample count is below configured references: "
                f"{sample_count} < {len(references)}"
            )
        if successful_samples < len(references):
            return (
                "successful reference samples are below configured references: "
                f"{successful_samples} < {len(references)}"
            )
    return None


def bounded_height_proof_error(
    report: dict | None,
    *,
    expected_height: int,
) -> str | None:
    if not isinstance(report, dict):
        return "missing-bounded-report"
    try:
        target_height = int(report.get("target_height"))
        last_height = int(report.get("last_height"))
    except (TypeError, ValueError):
        return "missing bounded height proof"
    if target_height != expected_height:
        return (
            "bounded target height mismatch: "
            f"{target_height} != {expected_height}"
        )
    if last_height < expected_height:
        return (
            "bounded last height below milestone: "
            f"{last_height} < {expected_height}"
        )
    return None


def import_window_speed_proof(report: dict | None) -> dict[str, Any] | None:
    if not isinstance(report, dict):
        return None
    sync_proof = report.get("sync_proof")
    if not isinstance(sync_proof, dict):
        return None
    sync_source = sync_proof.get("sync_source") or report.get("sync_source")
    if sync_source != "fast-sync":
        return None
    import_report = sync_proof.get("fast_sync_import")
    if not isinstance(import_report, dict):
        return None
    has_transaction_proof_fields = any(
        field in import_report
        for field in (
            "transaction_blocks",
            "transaction_blocks_per_second",
            "transaction_block_import_seconds",
            "transactions",
        )
    )
    if not has_transaction_proof_fields:
        try:
            bps = float(import_report["average_blocks_per_second"])
        except (KeyError, TypeError, ValueError):
            return None
        return {
            "source": "fast-sync-import",
            "blocks_per_second": bps,
            "imported_blocks": import_report.get("imported_blocks"),
            "final_height": import_report.get("final_height"),
            "elapsed_seconds": import_report.get("elapsed_seconds"),
            "throughput_status": import_report.get("throughput_status"),
        }
    try:
        transaction_blocks = int(import_report.get("transaction_blocks", 0))
        transactions = int(import_report.get("transactions", 0))
    except (TypeError, ValueError):
        transaction_blocks = 0
        transactions = 0
    try:
        transaction_bps = float(import_report["transaction_blocks_per_second"])
    except (KeyError, TypeError, ValueError):
        return {
            "source": "fast-sync-transaction-blocks",
            "blocks_per_second": 0.0,
            "imported_blocks": import_report.get("imported_blocks"),
            "final_height": import_report.get("final_height"),
            "elapsed_seconds": import_report.get("elapsed_seconds"),
            "throughput_status": import_report.get("throughput_status"),
            "empty_blocks": import_report.get("empty_blocks"),
            "transaction_blocks": transaction_blocks,
            "transactions": transactions,
            "missing_transaction_blocks_per_second": True,
        }
    if transaction_blocks <= 0:
        return {
            "source": "fast-sync-transaction-blocks",
            "blocks_per_second": 0.0,
            "imported_blocks": import_report.get("imported_blocks"),
            "final_height": import_report.get("final_height"),
            "elapsed_seconds": import_report.get("elapsed_seconds"),
            "throughput_status": import_report.get("throughput_status"),
            "empty_blocks": import_report.get("empty_blocks"),
            "transaction_blocks": transaction_blocks,
            "transactions": transactions,
            "missing_transaction_blocks": True,
        }
    try:
        transaction_elapsed = float(import_report["transaction_block_import_seconds"])
    except (KeyError, TypeError, ValueError):
        return {
            "source": "fast-sync-transaction-blocks",
            "blocks_per_second": transaction_bps,
            "imported_blocks": import_report.get("imported_blocks"),
            "final_height": import_report.get("final_height"),
            "elapsed_seconds": import_report.get("elapsed_seconds"),
            "throughput_status": import_report.get("throughput_status"),
            "empty_blocks": import_report.get("empty_blocks"),
            "transaction_blocks": transaction_blocks,
            "transactions": transactions,
            "missing_transaction_elapsed": True,
            "overall_blocks_per_second": import_report.get("average_blocks_per_second"),
        }
    expected_transaction_bps = (
        transaction_blocks / transaction_elapsed if transaction_elapsed > 0.0 else 0.0
    )
    return {
        "source": "fast-sync-transaction-blocks",
        "blocks_per_second": transaction_bps,
        "imported_blocks": import_report.get("imported_blocks"),
        "final_height": import_report.get("final_height"),
        "elapsed_seconds": import_report.get("elapsed_seconds"),
        "throughput_status": import_report.get("throughput_status"),
        "empty_blocks": import_report.get("empty_blocks"),
        "transaction_blocks": transaction_blocks,
        "transactions": transactions,
        "transaction_block_import_seconds": transaction_elapsed,
        "expected_transaction_blocks_per_second": expected_transaction_bps,
        "overall_blocks_per_second": import_report.get("average_blocks_per_second"),
    }


def empty_block_speed_proof(report: dict | None) -> dict[str, Any] | None:
    if not isinstance(report, dict):
        return None
    sync_proof = report.get("sync_proof")
    if not isinstance(sync_proof, dict):
        return None
    sync_source = sync_proof.get("sync_source") or report.get("sync_source")
    if sync_source != "fast-sync":
        return None
    import_report = sync_proof.get("fast_sync_import")
    if not isinstance(import_report, dict):
        return None
    try:
        empty_blocks = int(
            import_report["empty_only_blocks"]
            if "empty_only_blocks" in import_report
            else import_report["empty_blocks"]
        )
        empty_elapsed = float(import_report["empty_block_import_seconds"])
        empty_bps = float(import_report["empty_blocks_per_second"])
    except (KeyError, TypeError, ValueError):
        return None
    expected_empty_bps = empty_blocks / empty_elapsed if empty_elapsed > 0.0 else 0.0
    return {
        "source": "fast-sync-empty-blocks",
        "blocks_per_second": empty_bps,
        "empty_blocks": empty_blocks,
        "empty_block_import_seconds": empty_elapsed,
        "expected_empty_blocks_per_second": expected_empty_bps,
        "overall_blocks_per_second": import_report.get("average_blocks_per_second"),
        "transaction_blocks_per_second": import_report.get("transaction_blocks_per_second"),
    }


def speed_proof_summary(report: dict | None) -> dict[str, Any]:
    import_proof = import_window_speed_proof(report)
    replay_bps = 0.0
    if isinstance(report, dict):
        try:
            replay_bps = float(report.get("blocks_per_second", 0.0))
        except (TypeError, ValueError):
            replay_bps = 0.0
    if import_proof is not None:
        return {
            "source": import_proof["source"],
            "import_window_blocks_per_second": import_proof["blocks_per_second"],
            "replay_window_blocks_per_second": replay_bps,
        }
    return {
        "source": "height-samples",
        "import_window_blocks_per_second": None,
        "replay_window_blocks_per_second": replay_bps,
    }


def speed_proof_error(
    report: dict | None,
    *,
    floor_bps: float | None,
    ceiling_bps: float | None,
    minimum_transaction_blocks: int = DEFAULT_MINIMUM_TRANSACTION_BLOCKS,
) -> str | None:
    if floor_bps is None and ceiling_bps is None:
        return None
    if not isinstance(report, dict):
        return "missing-bounded-report"

    def has_node_metrics_proof() -> bool:
        sync_proof = report.get("sync_proof")
        if isinstance(sync_proof, dict):
            hot_metrics = sync_proof.get("fast_sync_hot_metrics")
            if isinstance(hot_metrics, dict) and hot_metrics:
                return True
        try:
            if int(report.get("metrics_sample_count", 0)) > 0:
                return True
        except (TypeError, ValueError):
            pass
        summary = report.get("metrics_summary")
        if isinstance(summary, dict) and isinstance(summary.get("metrics"), dict):
            if summary["metrics"]:
                return True
        return bool(metrics_sample_summary(report)["metrics"])

    def fast_sync_reference_proof_error() -> str | None:
        sync_proof = report.get("sync_proof")
        if not isinstance(sync_proof, dict):
            return "missing fast-sync reference proof"
        reference = sync_proof.get("fast_sync_reference")
        if not isinstance(reference, dict):
            return "missing fast-sync reference proof"
        required = ["endpoint", "block_height", "block_hash"]
        missing = [field for field in required if not reference.get(field)]
        if missing:
            return "missing fast-sync reference proof fields: " + ", ".join(missing)
        if "state_root_height" not in reference or "state_root_hash" not in reference:
            return "missing fast-sync reference state-root proof"
        return None

    import_proof = import_window_speed_proof(report)
    if import_proof is not None:
        if import_proof.get("missing_transaction_blocks"):
            return "fast-sync speed proof has no transaction-bearing blocks"
        if import_proof.get("missing_transaction_blocks_per_second"):
            return "missing transaction-bearing import BPS proof for speed claim"
        transaction_blocks = int(import_proof.get("transaction_blocks") or 0)
        if transaction_blocks < minimum_transaction_blocks:
            return (
                "fast-sync speed proof has too few transaction-bearing blocks: "
                f"{transaction_blocks} < {minimum_transaction_blocks}"
            )
        if import_proof.get("missing_transaction_elapsed"):
            return "missing transaction-bearing import elapsed proof for speed claim"
        transaction_elapsed = float(import_proof.get("transaction_block_import_seconds") or 0.0)
        if transaction_elapsed <= 0.0:
            return "missing transaction-bearing import elapsed proof for speed claim"
        bps = float(import_proof["blocks_per_second"])
        expected_bps = float(import_proof["expected_transaction_blocks_per_second"])
        tolerance = max(1e-6, abs(expected_bps) * 1e-6)
        if abs(bps - expected_bps) > tolerance:
            return (
                "fast-sync transaction-bearing BPS does not match elapsed proof: "
                f"{bps:g} != {expected_bps:g} blocks/s"
            )
        if floor_bps is not None and bps < floor_bps:
            return (
                "fast-sync import speed below configured floor: "
                f"{bps:g} < {floor_bps:g} blocks/s"
            )
        if ceiling_bps is not None and bps > ceiling_bps:
            return (
                "fast-sync import speed above configured ceiling: "
                f"{bps:g} > {ceiling_bps:g} blocks/s"
            )
        if not has_node_metrics_proof():
            return "missing node metrics proof for speed claim"
        reference_error = fast_sync_reference_proof_error()
        if reference_error is not None:
            return reference_error
        if not transaction_work_summary(report).get("observed_transaction_work"):
            return "missing transaction-bearing replay proof for speed claim"
        return None
    sample_summary = height_sample_rate_summary(report)
    if sample_summary["interval_count"] <= 0:
        return "missing height-sample speed proof"
    min_blocks_per_second = float(sample_summary["min_blocks_per_second"])
    max_blocks_per_second = float(sample_summary["max_blocks_per_second"])
    if floor_bps is not None and min_blocks_per_second < floor_bps:
        return (
            "height sample speed below configured floor: "
            f"{min_blocks_per_second:g} < {floor_bps:g} blocks/s"
        )
    if ceiling_bps is not None and max_blocks_per_second > ceiling_bps:
        return (
            "height sample speed above configured ceiling: "
            f"{max_blocks_per_second:g} > {ceiling_bps:g} blocks/s"
        )
    if not has_node_metrics_proof():
        return "missing node metrics proof for speed claim"
    if not transaction_work_summary(report).get("observed_transaction_work"):
        return "missing transaction-bearing replay proof for speed claim"
    return None


def empty_block_speed_proof_error(report: dict | None) -> str | None:
    proof = empty_block_speed_proof(report)
    if proof is None:
        return None
    empty_elapsed = float(proof.get("empty_block_import_seconds") or 0.0)
    if empty_elapsed <= 0.0:
        if int(proof.get("empty_blocks") or 0) > 0:
            return "missing empty-block import elapsed proof for empty-block speed claim"
        return None
    bps = float(proof["blocks_per_second"])
    expected_bps = float(proof["expected_empty_blocks_per_second"])
    tolerance = max(1e-6, abs(expected_bps) * 1e-6)
    if abs(bps - expected_bps) > tolerance:
        return (
            "fast-sync empty-block BPS does not match elapsed proof: "
            f"{bps:g} != {expected_bps:g} blocks/s"
        )
    return None


def checkpoint_verification_command(
    command: list[str],
    *,
    verified_height: int,
    verified_stateroot_root: str,
    verified_against_reference: bool,
) -> list[str]:
    verified_command = [
        *command,
        "--restore-verified",
        "--verified-height",
        str(verified_height),
        "--verified-stateroot-root",
        verified_stateroot_root,
    ]
    if verified_against_reference:
        verified_command.append("--verified-against-reference")
    return verified_command


def checkpoint_restore_probe(
    command: list[str],
    *,
    checkpoint_root: Path,
    height: int,
    expected_verified_stateroot_root: str,
    probe_bin: Path,
    storage_provider: str,
    runner: Callable[..., Any],
) -> dict[str, Any]:
    restore = run_command(command, runner)
    restore_root = checkpoint_restore_root(checkpoint_root, height)
    result: dict[str, Any] = {
        "command": command,
        "returncode": restore.returncode,
        "path": str(restore_root),
        "verified": False,
    }
    if restore.returncode != 0:
        result["reason"] = "restore command failed"
        result["stdout"] = getattr(restore, "stdout", "") or ""
        result["stderr"] = getattr(restore, "stderr", "") or ""
        return result

    content_reason = checkpoint_content_verification_reason(
        restore_root,
        expected_verified_height=height,
        expected_verified_stateroot_root=expected_verified_stateroot_root,
        probe_bin=probe_bin,
        storage_provider=storage_provider,
        probe_runner=runner,
    )
    if content_reason is not None:
        result["reason"] = content_reason
        result["stdout"] = getattr(restore, "stdout", "") or ""
        result["stderr"] = getattr(restore, "stderr", "") or ""
        return result

    result["verified"] = True
    return result


def expected_checkpoint_roots(results: list[dict]) -> dict[int, str]:
    roots: dict[int, str] = {}
    for result in results:
        try:
            height = int(result["height"])
        except (KeyError, TypeError, ValueError):
            continue
        report = result.get("bounded_report") or {}
        root = (
            (report.get("post_probe") or {})
            .get("stateroot_root", {})
            .get("root")
        )
        if root:
            roots[height] = str(root)
    return roots


def restore_roundtrip_by_height(results: list[dict]) -> dict[int, bool]:
    verified: dict[int, bool] = {}
    for result in results:
        try:
            height = int(result["height"])
        except (KeyError, TypeError, ValueError):
            continue
        restore_probe = result.get("checkpoint_restore_probe") or {}
        verified[height] = restore_probe.get("verified") is True
    return verified


def build_run_summary(
    plan: dict,
    results: list[dict],
    mode: str,
    *,
    probe_runner: Callable[..., Any] = subprocess.run,
) -> dict:
    milestones = [milestone_summary(result) for result in results]
    completed = [item for item in milestones if item.get("checkpoint_created")]
    minimum_checkpoint_count = int(
        plan.get("minimum_checkpoint_count", DEFAULT_MINIMUM_CHECKPOINT_COUNT)
    )
    retained = retained_checkpoint_inventory(
        Path(plan["checkpoint_root"]),
        plan["milestones"],
        expected_roots=expected_checkpoint_roots(results),
        restore_roundtrip_by_height=restore_roundtrip_by_height(results),
        probe_bin=Path(plan["probe_bin"]),
        storage_provider=plan.get("storage_provider", DEFAULT_STORAGE_PROVIDER),
        probe_runner=probe_runner,
    )
    retained_minimum_checkpoint_count_met = (
        retained["retained_usable_checkpoint_count"] >= minimum_checkpoint_count
    )
    bps_values = [
        float(item["blocks_per_second"])
        for item in milestones
        if item.get("blocks_per_second") is not None
    ]
    latest = completed[-1] if completed else (milestones[-1] if milestones else {})
    return {
        "mode": mode,
        "requested_milestones": plan["milestones"],
        "completed_heights": [item["height"] for item in completed],
        "completed_checkpoint_count": len(completed),
        "minimum_checkpoint_count": minimum_checkpoint_count,
        "minimum_checkpoint_count_met": len(completed) >= minimum_checkpoint_count,
        "missing_checkpoint_count": max(minimum_checkpoint_count - len(completed), 0),
        "retained_usable_checkpoint_count": retained["retained_usable_checkpoint_count"],
        "retained_minimum_checkpoint_count_met": retained_minimum_checkpoint_count_met,
        "retained_missing_checkpoint_count": retained["retained_missing_checkpoint_count"],
        "retained_usable_checkpoints": retained["retained_usable_checkpoints"],
        "latest_height": latest.get("last_height"),
        "latest_root": latest.get("local_root"),
        "average_blocks_per_second": sum(bps_values) / len(bps_values) if bps_values else 0.0,
        "all_reference_matched": all(
            item.get("reference_matches_local") is True for item in completed
        )
        if completed
        else False,
        "milestones": milestones,
    }


def summary_history_record(plan: dict, result: dict) -> dict:
    return {
        "timestamp_utc": datetime.now(timezone.utc).isoformat(),
        "mode": result["mode"],
        "config": plan["config"],
        "node_bin": plan.get("node_bin"),
        "probe_bin": plan.get("probe_bin"),
        "chain_db": plan["chain_db"],
        "stateroot_db": plan["stateroot_db"],
        "checkpoint_root": plan["checkpoint_root"],
        "summary": result.get("summary", {}),
    }


def append_summary_jsonl(path: Path, record: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(record, sort_keys=True))
        handle.write("\n")


def run_milestones(
    plan: dict,
    *,
    runner: Callable[..., Any] = subprocess.run,
    include_command_output: bool = False,
) -> dict:
    results = []
    for step in plan["steps"]:
        bounded = run_command(step["bounded_command"], runner)
        bounded_report = None
        if getattr(bounded, "stdout", ""):
            try:
                bounded_report = parse_last_json_object(bounded.stdout)
            except ValueError as exc:
                bounded_report = {"parse_error": str(exc)}
        result = {
            "height": step["height"],
            "bounded_command": step["bounded_command"],
            "bounded_returncode": bounded.returncode,
            "bounded_report": bounded_report,
        }
        bounded_failed = bounded.returncode != 0 or (
            bounded_report or {}
        ).get("status") != "target-reached"
        if include_command_output or bounded_failed:
            result["bounded_stdout"] = getattr(bounded, "stdout", "") or ""
            result["bounded_stderr"] = getattr(bounded, "stderr", "") or ""
        results.append(result)
        if bounded_failed:
            return {
                "mode": "failed",
                "failed_height": step["height"],
                "failure": "bounded-replay",
                "summary": build_run_summary(plan, results, "failed", probe_runner=runner),
                "results": results,
            }

        height_proof_error = bounded_height_proof_error(
            bounded_report,
            expected_height=step["height"],
        )
        if height_proof_error is not None:
            result["bounded_height_proof_error"] = height_proof_error
            result["bounded_stdout"] = getattr(bounded, "stdout", "") or ""
            result["bounded_stderr"] = getattr(bounded, "stderr", "") or ""
            return {
                "mode": "failed",
                "failed_height": step["height"],
                "failure": "bounded-height-proof",
                "summary": build_run_summary(plan, results, "failed", probe_runner=runner),
                "results": results,
            }

        speed_error = speed_proof_error(
            bounded_report,
            floor_bps=plan.get("sync_speed_floor_blocks_per_second"),
            ceiling_bps=plan.get("sync_speed_ceiling_blocks_per_second"),
            minimum_transaction_blocks=plan.get(
                "minimum_transaction_blocks_for_speed_proof",
                DEFAULT_MINIMUM_TRANSACTION_BLOCKS,
            ),
        )
        if speed_error is not None:
            result["speed_proof_error"] = speed_error
            result["bounded_stdout"] = getattr(bounded, "stdout", "") or ""
            result["bounded_stderr"] = getattr(bounded, "stderr", "") or ""
            return {
                "mode": "failed",
                "failed_height": step["height"],
                "failure": "speed-proof",
                "summary": build_run_summary(plan, results, "failed", probe_runner=runner),
                "results": results,
            }

        post_probe_error = post_probe_checkpoint_error(
            bounded_report,
            references=plan.get("reference_rpcs") or [],
            expected_height=step["height"],
        )
        if post_probe_error is not None:
            result["post_probe_error"] = post_probe_error
            result["bounded_stdout"] = getattr(bounded, "stdout", "") or ""
            result["bounded_stderr"] = getattr(bounded, "stderr", "") or ""
            return {
                "mode": "failed",
                "failed_height": step["height"],
                "failure": "post-probe",
                "summary": build_run_summary(plan, results, "failed", probe_runner=runner),
                "results": results,
            }

        checkpoint_command_for_run = checkpoint_verification_command(
            step["checkpoint_command"],
            verified_height=step["height"],
            verified_stateroot_root=str(
                bounded_report["post_probe"]["stateroot_root"]["root"]
            ),
            verified_against_reference=bool(plan.get("reference_rpcs")),
        )
        checkpoint = run_command(checkpoint_command_for_run, runner)
        result["checkpoint_command"] = checkpoint_command_for_run
        result["checkpoint_returncode"] = checkpoint.returncode
        if include_command_output or checkpoint.returncode != 0:
            result["checkpoint_stdout"] = getattr(checkpoint, "stdout", "") or ""
            result["checkpoint_stderr"] = getattr(checkpoint, "stderr", "") or ""
        if checkpoint.returncode != 0:
            return {
                "mode": "failed",
                "failed_height": step["height"],
                "failure": "checkpoint",
                "summary": build_run_summary(plan, results, "failed", probe_runner=runner),
                "results": results,
            }
        expected_root = str(bounded_report["post_probe"]["stateroot_root"]["root"])
        inventory = checkpoint_inventory(
            Path(plan["checkpoint_root"]) / f"h{step['height']}",
            expected_verified_height=step["height"],
            expected_verified_stateroot_root=expected_root,
            expected_verified_against_reference=bool(plan.get("reference_rpcs")),
            probe_bin=Path(plan["probe_bin"]),
            storage_provider=plan.get("storage_provider", DEFAULT_STORAGE_PROVIDER),
            probe_runner=runner,
        )
        result["checkpoint_inventory"] = inventory
        if not inventory["usable_for_state_validation"]:
            result["checkpoint_stdout"] = getattr(checkpoint, "stdout", "") or ""
            result["checkpoint_stderr"] = getattr(checkpoint, "stderr", "") or ""
            return {
                "mode": "failed",
                "failed_height": step["height"],
                "failure": "checkpoint-inventory",
                "summary": build_run_summary(plan, results, "failed", probe_runner=runner),
                "results": results,
            }
        restore_probe = checkpoint_restore_probe(
            step["checkpoint_restore_command"],
            checkpoint_root=Path(plan["checkpoint_root"]),
            height=step["height"],
            expected_verified_stateroot_root=expected_root,
            probe_bin=Path(plan["probe_bin"]),
            storage_provider=plan.get("storage_provider", DEFAULT_STORAGE_PROVIDER),
            runner=runner,
        )
        result["checkpoint_restore_probe"] = restore_probe
        if not restore_probe["verified"]:
            result["checkpoint_inventory"] = {
                **inventory,
                "restore_roundtrip_verified": False,
                "usable_for_state_validation": False,
                "reason": restore_probe.get("reason")
                or "checkpoint restore roundtrip verification failed",
            }
            return {
                "mode": "failed",
                "failed_height": step["height"],
                "failure": "checkpoint-restore-probe",
                "summary": build_run_summary(plan, results, "failed", probe_runner=runner),
                "results": results,
            }
        inventory = {
            **inventory,
            "restore_roundtrip_verified": True,
            "usable_for_state_validation": True,
            "reason": None,
        }
        result["checkpoint_inventory"] = inventory

    summary = build_run_summary(plan, results, "completed", probe_runner=runner)
    if not summary["minimum_checkpoint_count_met"]:
        return {
            "mode": "failed",
            "failed_height": results[-1]["height"] if results else None,
            "failure": "minimum-checkpoints",
            "summary": summary,
            "results": results,
        }
    if summary["retained_usable_checkpoint_count"] < summary["minimum_checkpoint_count"]:
        return {
            "mode": "failed",
            "failed_height": results[-1]["height"] if results else None,
            "failure": "retained-minimum-checkpoints",
            "summary": summary,
            "results": results,
        }

    return {
        "mode": "completed",
        "milestones": plan["milestones"],
        "checkpoint_root": plan["checkpoint_root"],
        "summary": summary,
        "results": results,
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Run sequential StateRoot bounded replay milestones, compare each "
            "local root with reference RPCs, and checkpoint each successful height."
        )
    )
    parser.add_argument("--config", required=True, type=Path)
    parser.add_argument("--node-bin", default=DEFAULT_NODE_BIN, type=Path)
    parser.add_argument("--rpc", default=DEFAULT_RPC)
    parser.add_argument(
        "--milestone",
        action="append",
        default=[],
        help="Milestone height. Repeat the flag or pass comma-separated heights.",
    )
    parser.add_argument("--poll-interval", default=5.0, type=float)
    parser.add_argument("--max-seconds", default=900.0, type=float)
    parser.add_argument("--chain-db", required=True, type=Path)
    parser.add_argument("--stateroot-db", required=True, type=Path)
    parser.add_argument("--probe-bin", default=DEFAULT_PROBE_BIN, type=Path)
    parser.add_argument(
        "--storage-provider",
        default=DEFAULT_STORAGE_PROVIDER,
        choices=["mdbx", "rocksdb"],
        help="Storage backend used by neo-db-probe for bounded and checkpoint proof reads.",
    )
    parser.add_argument(
        "--fast-sync",
        action="store_true",
        help=(
            "Run the first bounded replay through neo-node's built-in fast-sync "
            "package path. Metrics are sampled when available but not required, "
            "because the import can satisfy --stop-at-height before telemetry starts."
        ),
    )
    parser.add_argument(
        "--fast-sync-cache",
        default=None,
        type=Path,
        help="Optional cache directory for the built-in fast-sync package.",
    )
    parser.add_argument(
        "--initial-height",
        default=None,
        type=int,
        help="Optional starting height for import/replay BPS calculation.",
    )
    parser.add_argument(
        "--reference",
        action="append",
        default=[],
        help="Reference RPC URL(s). Repeat the flag or pass comma-separated URLs.",
    )
    parser.add_argument("--data-dir", default=None, type=Path)
    parser.add_argument("--checkpoint-root", required=True, type=Path)
    parser.add_argument(
        "--checkpoint-script",
        default=Path("scripts/checkpoint-on-height.sh"),
        type=Path,
    )
    parser.add_argument(
        "--restore-script",
        default=Path(DEFAULT_RESTORE_SCRIPT),
        type=Path,
        help="Restore script used for scratch restore-probe verification.",
    )
    parser.add_argument("--log-dir", required=True, type=Path)
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument(
        "--include-command-output",
        action="store_true",
        help="Include raw child stdout/stderr in the final JSON even for successful steps.",
    )
    parser.add_argument(
        "--summary-jsonl",
        default=None,
        type=Path,
        help="Append one compact run summary JSON object per line for performance history.",
    )
    parser.add_argument(
        "--metrics-url",
        default=None,
        help="Optional Prometheus /metrics URL to sample during each bounded replay.",
    )
    parser.add_argument(
        "--sync-speed-floor-bps",
        default=DEFAULT_SYNC_SPEED_FLOOR_BPS,
        type=float,
        help=(
            "Required bounded replay speed floor in blocks per second. "
            "Production proof requires at least 1500 blocks/s."
        ),
    )
    parser.add_argument(
        "--sync-speed-ceiling-bps",
        default=DEFAULT_SYNC_SPEED_CEILING_BPS,
        type=float,
        help="Required bounded replay speed ceiling in blocks per second.",
    )
    parser.add_argument(
        "--minimum-checkpoint-count",
        default=DEFAULT_MINIMUM_CHECKPOINT_COUNT,
        type=int,
        help="Minimum successful checkpoint count required for the milestone run to complete.",
    )
    parser.add_argument(
        "--minimum-transaction-blocks",
        default=DEFAULT_MINIMUM_TRANSACTION_BLOCKS,
        type=int,
        help=(
            "Minimum transaction-bearing blocks required before accepting a "
            "fast-sync import speed proof."
        ),
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        milestones = parse_height_values(args.milestone)
    except ValueError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 2
    if args.minimum_checkpoint_count < DEFAULT_MINIMUM_CHECKPOINT_COUNT:
        print(
            "ERROR: --minimum-checkpoint-count must be >= "
            f"{DEFAULT_MINIMUM_CHECKPOINT_COUNT}",
            file=sys.stderr,
        )
        return 2
    if args.minimum_transaction_blocks < DEFAULT_MINIMUM_TRANSACTION_BLOCKS:
        print(
            "ERROR: --minimum-transaction-blocks must be >= "
            f"{DEFAULT_MINIMUM_TRANSACTION_BLOCKS}",
            file=sys.stderr,
        )
        return 2
    if (
        args.sync_speed_floor_bps is not None
        and args.sync_speed_floor_bps < DEFAULT_SYNC_SPEED_FLOOR_BPS
    ):
        print(
            "ERROR: --sync-speed-floor-bps must be >= "
            f"{DEFAULT_SYNC_SPEED_FLOOR_BPS:g} for production proof",
            file=sys.stderr,
        )
        return 2
    if len(milestones) < args.minimum_checkpoint_count:
        print(
            "ERROR: requested milestone count must be >= "
            "--minimum-checkpoint-count "
            f"({len(milestones)} < {args.minimum_checkpoint_count})",
            file=sys.stderr,
        )
        return 2
    if args.fast_sync_cache is not None and not args.fast_sync:
        print("ERROR: --fast-sync-cache requires --fast-sync", file=sys.stderr)
        return 2
    references = normalize_reference_urls(args.reference) or DEFAULT_REFERENCE_RPCS
    data_dir = args.data_dir or args.chain_db.parent
    plan = build_plan(
        config=args.config,
        node_bin=args.node_bin,
        rpc_url=args.rpc,
        milestones=milestones,
        poll_interval=args.poll_interval,
        max_seconds=args.max_seconds,
        chain_db=args.chain_db,
        stateroot_db=args.stateroot_db,
        probe_bin=args.probe_bin,
        storage_provider=args.storage_provider,
        references=references,
        data_dir=data_dir,
        checkpoint_root=args.checkpoint_root,
        checkpoint_script=args.checkpoint_script,
        restore_script=args.restore_script,
        log_dir=args.log_dir,
        metrics_url=args.metrics_url,
        sync_speed_floor_bps=args.sync_speed_floor_bps,
        sync_speed_ceiling_bps=args.sync_speed_ceiling_bps,
        minimum_checkpoint_count=args.minimum_checkpoint_count,
        minimum_transaction_blocks=args.minimum_transaction_blocks,
        fast_sync=args.fast_sync,
        fast_sync_cache=args.fast_sync_cache,
        initial_height=args.initial_height,
    )
    if args.dry_run:
        print(json.dumps(plan, indent=2, sort_keys=True))
        return 0

    args.log_dir.mkdir(parents=True, exist_ok=True)
    result = run_milestones(plan, include_command_output=args.include_command_output)
    if args.summary_jsonl is not None:
        append_summary_jsonl(args.summary_jsonl, summary_history_record(plan, result))
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0 if result["mode"] == "completed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
