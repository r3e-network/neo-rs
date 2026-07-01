#!/usr/bin/env python3
"""Maintain checkpoint phases reported by continuous state-root validation."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import time
import tomllib
from pathlib import Path
from typing import Any, Callable


DEFAULT_STATUS_FILE = "/tmp/stateroot-validation.json"
DEFAULT_DATA_DIR = "./data"
DEFAULT_CHECKPOINT_SCRIPT = "scripts/checkpoint-on-height.sh"
DEFAULT_RESTORE_SCRIPT = "scripts/restore-checkpoint.sh"
DEFAULT_PROBE_BIN = "target/debug/neo-db-probe"
DEFAULT_WAITING_INTERVAL_SECONDS = 30
DEFAULT_MINIMUM_FULL_STATE_CHECKPOINTS = 3
REQUIRED_CHECKPOINT_STAGES = ("base", "mid", "latest")
REQUIRED_DURABLE_HEIGHT_FIELDS = (
    "local_block_count",
    "local_state_height",
    "local_validated_height",
    "last_validated_block",
)
CHECKPOINT_VERIFICATION_FIELDS = (
    "restore_verified",
    "verified_height",
    "verified_stateroot_root",
    "verified_against_reference",
)
DEFAULT_STORAGE_PROVIDER = "mdbx"


def read_checkpoint_info(path: Path) -> dict[str, str]:
    info_path = path / "CHECKPOINT_INFO"
    fields: dict[str, str] = {}
    try:
        for line in info_path.read_text(encoding="utf-8").splitlines():
            key, separator, value = line.partition("=")
            if separator:
                fields[key.strip()] = value.strip()
    except OSError:
        return {}
    return fields


def command_stdout(result: Any) -> str:
    return str(getattr(result, "stdout", "") or "")


def parse_probe_chain_height(output: str) -> int | None:
    payload = json.loads(output)
    decoded = payload.get("decoded") or {}
    if "index" not in decoded:
        return None
    return int(decoded["index"])


def parse_probe_stateroot_height(output: str) -> int | None:
    payload = json.loads(output)
    height = payload.get("height") or {}
    decoded = height.get("decoded") or {}
    if "current_local_root_index" not in decoded:
        return None
    return int(decoded["current_local_root_index"])


def parse_probe_stateroot_root(output: str) -> str | None:
    payload = json.loads(output)
    state_root = payload.get("state_root") or {}
    decoded = state_root.get("decoded") or {}
    root = decoded.get("roothash")
    if root is None:
        return None
    return str(root)


def build_probe_chain_height_command(
    *,
    chain_db: Path,
    probe_bin: Path,
    storage_provider: str = DEFAULT_STORAGE_PROVIDER,
) -> list[str]:
    return [
        str(probe_bin),
        "--db",
        str(chain_db),
        "--storage-provider",
        storage_provider,
        "--contract-id",
        "-4",
        "--key-hex",
        "0c",
        "--decode",
        "hash-index",
    ]


def build_probe_stateroot_height_command(
    *,
    stateroot_db: Path,
    probe_bin: Path,
    storage_provider: str = DEFAULT_STORAGE_PROVIDER,
) -> list[str]:
    return [
        str(probe_bin),
        "--db",
        str(stateroot_db),
        "--storage-provider",
        storage_provider,
        "--mpt-state-height",
    ]


def build_probe_stateroot_root_command(
    *,
    stateroot_db: Path,
    height: int,
    probe_bin: Path,
    storage_provider: str = DEFAULT_STORAGE_PROVIDER,
) -> list[str]:
    return [
        str(probe_bin),
        "--db",
        str(stateroot_db),
        "--storage-provider",
        storage_provider,
        "--mpt-state-root",
        str(height),
    ]


def checkpoint_verification_reason(
    path: Path,
    *,
    probe_bin: Path | None = None,
    inventory_runner: Callable[[list[str]], Any] | None = None,
) -> str | None:
    info = read_checkpoint_info(path)
    missing = [field for field in CHECKPOINT_VERIFICATION_FIELDS if not info.get(field)]
    if missing:
        return "missing restore verification metadata: " + ", ".join(missing)

    height_text = info.get("height")
    verified_height_text = info.get("verified_height")
    if height_text is not None and verified_height_text != height_text:
        return (
            "restore verification height does not match checkpoint height: "
            f"height={height_text}, verified_height={verified_height_text}"
        )

    if info["restore_verified"].lower() != "true":
        return "restore verification metadata is not marked restore_verified=true"
    if info["verified_against_reference"].lower() != "true":
        return "restore verification metadata is not marked verified_against_reference=true"

    if probe_bin is None or inventory_runner is None:
        return "checkpoint content probe runner is required for usable checkpoint inventory"
    try:
        verified_height = int(info["verified_height"])
    except ValueError:
        return "restore verification metadata has invalid verified_height"
    chain_db = path / "mainnet"
    stateroot_db = path / "StateRoot"
    storage_provider = info.get("storage_provider") or DEFAULT_STORAGE_PROVIDER
    chain_height = parse_probe_chain_height(
        command_stdout(
            inventory_runner(
                build_probe_chain_height_command(
                    chain_db=chain_db,
                    probe_bin=probe_bin,
                    storage_provider=storage_provider,
                )
            )
        )
    )
    if chain_height != verified_height:
        return (
            "checkpoint chain database probe height mismatch: "
            f"height={chain_height}, verified_height={verified_height}"
        )
    stateroot_height = parse_probe_stateroot_height(
        command_stdout(
            inventory_runner(
                build_probe_stateroot_height_command(
                    stateroot_db=stateroot_db,
                    probe_bin=probe_bin,
                    storage_provider=storage_provider,
                )
            )
        )
    )
    if stateroot_height != verified_height:
        return (
            "checkpoint StateRoot database probe height mismatch: "
            f"height={stateroot_height}, verified_height={verified_height}"
        )
    stateroot_root = parse_probe_stateroot_root(
        command_stdout(
            inventory_runner(
                build_probe_stateroot_root_command(
                    stateroot_db=stateroot_db,
                    height=verified_height,
                    probe_bin=probe_bin,
                    storage_provider=storage_provider,
                )
            )
        )
    )
    if stateroot_root is None or stateroot_root.lower() != info["verified_stateroot_root"].lower():
        return (
            "checkpoint StateRoot root probe mismatch: "
            f"root={stateroot_root}, verified_root={info['verified_stateroot_root']}"
        )

    return None


def checkpoint_exists(
    path: Path,
    *,
    probe_bin: Path | None = None,
    inventory_runner: Callable[[list[str]], Any] | None = None,
) -> bool:
    return (
        path.is_dir()
        and (path / "mainnet").is_dir()
        and (path / "StateRoot").is_dir()
        and (path / "CHECKPOINT_INFO").is_file()
        and not (path / "CHECKPOINT_IN_PROGRESS").exists()
        and checkpoint_verification_reason(
            path,
            probe_bin=probe_bin,
            inventory_runner=inventory_runner,
        )
        is None
    )


def checkpoint_is_blocked(
    path: Path,
    *,
    probe_bin: Path | None = None,
    inventory_runner: Callable[[list[str]], Any] | None = None,
) -> bool:
    return path.exists() and not checkpoint_exists(
        path,
        probe_bin=probe_bin,
        inventory_runner=inventory_runner,
    )


def count_usable_full_state_checkpoints(
    checkpoint_root: Path,
    *,
    probe_bin: Path | None = None,
    inventory_runner: Callable[[list[str]], Any] | None = None,
) -> int:
    if not checkpoint_root.is_dir():
        return 0
    return sum(
        1
        for path in checkpoint_root.iterdir()
        if path.is_dir()
        and checkpoint_exists(
            path,
            probe_bin=probe_bin,
            inventory_runner=inventory_runner,
        )
    )


def usable_checkpoint_paths(
    checkpoint_root: Path,
    *,
    probe_bin: Path | None = None,
    inventory_runner: Callable[[list[str]], Any] | None = None,
) -> set[Path]:
    if not checkpoint_root.is_dir():
        return set()
    return {
        path
        for path in checkpoint_root.iterdir()
        if path.is_dir()
        and checkpoint_exists(
            path,
            probe_bin=probe_bin,
            inventory_runner=inventory_runner,
        )
    }


def checkpoint_height_readiness(status: dict, height: int) -> tuple[bool, str | None]:
    missing_fields = [
        field for field in REQUIRED_DURABLE_HEIGHT_FIELDS if status.get(field) is None
    ]
    if missing_fields:
        if missing_fields == ["local_validated_height"]:
            return False, "missing validated height field: local_validated_height"
        return (
            False,
            "missing durable height fields: " + ", ".join(missing_fields),
        )

    local_block_count = status.get("local_block_count")
    local_state_height = status.get("local_state_height")
    if int(local_block_count) != height + 1:
        return (
            False,
            "checkpoint height is not the current durable chain height; "
            "historical snapshots cannot be created from a later live DB",
        )
    if int(local_state_height) != height:
        return (
            False,
            "checkpoint height is not the current durable StateRoot height",
        )

    local_validated_height = status.get("local_validated_height")
    if int(local_validated_height) != height:
        return (
            False,
            "checkpoint height is not the current validated height",
        )

    last_validated_block = status.get("last_validated_block")
    if int(last_validated_block) < height:
        return (
            False,
            "checkpoint height is ahead of the last reference-validated block",
        )

    return True, None


def checkpoint_stage_payload_is_complete(status: dict) -> tuple[bool, str | None]:
    stages = status.get("checkpoint_stages") or []
    if not stages:
        return False, "missing checkpoint stages: base, mid, latest"
    stage_names = {
        str(stage["stage"])
        for stage in stages
        if isinstance(stage, dict) and stage.get("stage") is not None
    }
    missing_stages = [
        stage for stage in REQUIRED_CHECKPOINT_STAGES if stage not in stage_names
    ]
    unexpected_stages = sorted(
        stage for stage in stage_names if stage not in REQUIRED_CHECKPOINT_STAGES
    )
    if missing_stages or unexpected_stages:
        reason_parts = []
        if missing_stages:
            reason_parts.append(
                "missing required checkpoint stages: " + ", ".join(missing_stages)
            )
        if unexpected_stages:
            reason_parts.append(
                "unexpected checkpoint stages: " + ", ".join(unexpected_stages)
            )
        return False, "; ".join(reason_parts)
    return True, None


def load_status(path: Path) -> dict:
    if not path.exists():
        return {
            "status": f"missing status file: {path}",
            "checkpoint_stages": [],
        }
    return json.loads(path.read_text(encoding="utf-8"))


def format_network_magic(value: Any) -> str:
    if isinstance(value, int):
        return f"{value:08X}"
    if isinstance(value, str):
        stripped = value.strip()
        return f"{int(stripped, 0):08X}"
    raise ValueError(f"unsupported network_magic value: {value!r}")


def derive_checkpoint_paths_from_config(config_path: Path) -> dict[str, Path]:
    with config_path.open("rb") as handle:
        config = tomllib.load(handle)

    storage = config.get("storage") or {}
    state_service = config.get("state_service") or {}
    network = config.get("network") or {}

    chain_db = Path(storage.get("data_dir") or storage.get("path") or DEFAULT_DATA_DIR)
    data_dir = chain_db.parent if chain_db.name else Path(DEFAULT_DATA_DIR)

    magic = format_network_magic(network.get("network_magic", 0))
    state_template = str(state_service.get("path") or f"{DEFAULT_DATA_DIR}/Plugins/mainnet/StateRoot")
    stateroot_db = Path(state_template.replace("{0}", magic))

    return {
        "data_dir": data_dir,
        "chain_db": chain_db,
        "stateroot_db": stateroot_db,
    }


def build_checkpoint_plan(
    status: dict,
    *,
    checkpoint_root: Path,
    data_dir: Path,
    writer_pid: str,
    script_path: Path,
    chain_db: Path | None,
    stateroot_db: Path | None,
    probe_bin: Path = Path(DEFAULT_PROBE_BIN),
    inventory_runner: Callable[[list[str]], Any] | None = None,
) -> dict:
    plan_context = {
        "checkpoint_root": str(checkpoint_root),
        "data_dir": str(data_dir),
        "chain_db": str(chain_db) if chain_db is not None else None,
        "stateroot_db": str(stateroot_db) if stateroot_db is not None else None,
        "probe_bin": str(probe_bin),
    }
    existing_usable_checkpoints = usable_checkpoint_paths(
        checkpoint_root,
        probe_bin=probe_bin,
        inventory_runner=inventory_runner,
    )
    usable_checkpoint_count = len(existing_usable_checkpoints)
    stages = status.get("checkpoint_stages") or []
    if not stages:
        return {
            "status": "waiting",
            "actions": [],
            "usable_checkpoint_count": usable_checkpoint_count,
            "minimum_usable_checkpoint_count": DEFAULT_MINIMUM_FULL_STATE_CHECKPOINTS,
            "minimum_usable_checkpoint_count_met": usable_checkpoint_count
            >= DEFAULT_MINIMUM_FULL_STATE_CHECKPOINTS,
            "missing_usable_checkpoint_count": max(
                DEFAULT_MINIMUM_FULL_STATE_CHECKPOINTS - usable_checkpoint_count,
                0,
            ),
            "projected_usable_checkpoint_count": usable_checkpoint_count,
            "projected_minimum_usable_checkpoint_count_met": usable_checkpoint_count
            >= DEFAULT_MINIMUM_FULL_STATE_CHECKPOINTS,
            "projected_missing_usable_checkpoint_count": max(
                DEFAULT_MINIMUM_FULL_STATE_CHECKPOINTS - usable_checkpoint_count,
                0,
            ),
            **plan_context,
        }

    stages_complete, stages_reason = checkpoint_stage_payload_is_complete(status)
    if not stages_complete:
        return {
            "status": "blocked",
            "reason": stages_reason,
            "actions": [],
            "usable_checkpoint_count": usable_checkpoint_count,
            "minimum_usable_checkpoint_count": DEFAULT_MINIMUM_FULL_STATE_CHECKPOINTS,
            "minimum_usable_checkpoint_count_met": usable_checkpoint_count
            >= DEFAULT_MINIMUM_FULL_STATE_CHECKPOINTS,
            "missing_usable_checkpoint_count": max(
                DEFAULT_MINIMUM_FULL_STATE_CHECKPOINTS - usable_checkpoint_count,
                0,
            ),
            "projected_usable_checkpoint_count": usable_checkpoint_count,
            "projected_minimum_usable_checkpoint_count_met": usable_checkpoint_count
            >= DEFAULT_MINIMUM_FULL_STATE_CHECKPOINTS,
            "projected_missing_usable_checkpoint_count": max(
                DEFAULT_MINIMUM_FULL_STATE_CHECKPOINTS - usable_checkpoint_count,
                0,
            ),
            **plan_context,
        }

    actions = []
    blocked = False
    deferred = False
    blocked_reasons: list[str] = []
    deferred_reasons: list[str] = []
    planned_creates: set[Path] = set()
    for stage in stages:
        height = int(stage["height"])
        stage_name = str(stage["stage"])
        checkpoint_path = checkpoint_root / f"h{height}"
        item = {
            "stage": stage_name,
            "height": height,
            "label": stage.get("label", f"{stage_name}-h{height}"),
            "checkpoint_path": str(checkpoint_path),
        }
        if checkpoint_exists(
            checkpoint_path,
            probe_bin=probe_bin,
            inventory_runner=inventory_runner,
        ):
            item["action"] = "skip"
        elif checkpoint_is_blocked(
            checkpoint_path,
            probe_bin=probe_bin,
            inventory_runner=inventory_runner,
        ):
            item["action"] = "blocked"
            if (checkpoint_path / "CHECKPOINT_IN_PROGRESS").exists():
                item["reason"] = "incomplete checkpoint directory exists"
            else:
                verification_reason = checkpoint_verification_reason(
                    checkpoint_path,
                    probe_bin=probe_bin,
                    inventory_runner=inventory_runner,
                )
                item["reason"] = (
                    verification_reason or "incomplete checkpoint directory exists"
                )
            blocked = True
            blocked_reasons.append(item["reason"])
        elif checkpoint_path in planned_creates:
            item["action"] = "skip"
            item["reason"] = "checkpoint height already planned"
        else:
            ready, reason = checkpoint_height_readiness(status, height)
            if ready:
                item["action"] = "create"
                planned_creates.add(checkpoint_path)
                command = [
                    str(script_path),
                    writer_pid,
                    "--once",
                    "--height",
                    str(height),
                    "--data-dir",
                    str(data_dir),
                    "--root",
                    str(checkpoint_root),
                    "--storage-provider",
                    DEFAULT_STORAGE_PROVIDER,
                ]
                if chain_db is not None:
                    command.extend(["--chain-db", str(chain_db)])
                if stateroot_db is not None:
                    command.extend(["--stateroot-db", str(stateroot_db)])
                item["command"] = command
                item["requires_restore_probe"] = True
                item["restore_probe_command"] = build_restore_probe_command(
                    height=height,
                    checkpoint_root=checkpoint_root,
                    data_dir=data_dir,
                    restore_script=Path(DEFAULT_RESTORE_SCRIPT),
                )
                restore_probe_chain_db, restore_probe_stateroot_db = restore_probe_paths(
                    height=height,
                    data_dir=data_dir,
                )
                item["restore_probe_chain_height_command"] = build_probe_chain_height_command(
                    chain_db=restore_probe_chain_db,
                    probe_bin=probe_bin,
                    storage_provider=DEFAULT_STORAGE_PROVIDER,
                )
                item["restore_probe_stateroot_height_command"] = (
                    build_probe_stateroot_height_command(
                        stateroot_db=restore_probe_stateroot_db,
                        probe_bin=probe_bin,
                        storage_provider=DEFAULT_STORAGE_PROVIDER,
                    )
                )
                item["restore_probe_stateroot_root_command"] = (
                    build_probe_stateroot_root_command(
                        stateroot_db=restore_probe_stateroot_db,
                        height=height,
                        probe_bin=probe_bin,
                        storage_provider=DEFAULT_STORAGE_PROVIDER,
                    )
                )
                expected_root = (
                    stage.get("verified_stateroot_root")
                    or stage.get("stateroot_root")
                    or stage.get("local_root")
                    or stage.get("root")
                )
                if expected_root:
                    item["expected_stateroot_root"] = str(expected_root)
                item["verified_against_reference"] = (
                    stage.get("verified_against_reference") is True
                )
            else:
                item["action"] = "defer"
                item["reason"] = reason
                deferred = True
                if reason is not None:
                    deferred_reasons.append(reason)
        actions.append(item)

    projected_usable_checkpoint_count = len(existing_usable_checkpoints | planned_creates)
    projected_minimum_usable_checkpoint_count_met = (
        projected_usable_checkpoint_count >= DEFAULT_MINIMUM_FULL_STATE_CHECKPOINTS
    )
    projected_missing_usable_checkpoint_count = max(
        DEFAULT_MINIMUM_FULL_STATE_CHECKPOINTS - projected_usable_checkpoint_count,
        0,
    )

    if blocked:
        reason = "; ".join(blocked_reasons)
        return {
            "status": "blocked",
            "reason": reason,
            "actions": actions,
            "usable_checkpoint_count": usable_checkpoint_count,
            "minimum_usable_checkpoint_count": DEFAULT_MINIMUM_FULL_STATE_CHECKPOINTS,
            "minimum_usable_checkpoint_count_met": usable_checkpoint_count
            >= DEFAULT_MINIMUM_FULL_STATE_CHECKPOINTS,
            "missing_usable_checkpoint_count": max(
                DEFAULT_MINIMUM_FULL_STATE_CHECKPOINTS - usable_checkpoint_count,
                0,
            ),
            "projected_usable_checkpoint_count": projected_usable_checkpoint_count,
            "projected_minimum_usable_checkpoint_count_met": projected_minimum_usable_checkpoint_count_met,
            "projected_missing_usable_checkpoint_count": projected_missing_usable_checkpoint_count,
            **plan_context,
        }

    if deferred:
        reason = (
            "; ".join(deferred_reasons)
            if deferred_reasons
            else "checkpoint stages are not yet ready"
        )
        return {
            "status": "waiting",
            "reason": reason,
            "actions": actions,
            "usable_checkpoint_count": usable_checkpoint_count,
            "minimum_usable_checkpoint_count": DEFAULT_MINIMUM_FULL_STATE_CHECKPOINTS,
            "minimum_usable_checkpoint_count_met": usable_checkpoint_count
            >= DEFAULT_MINIMUM_FULL_STATE_CHECKPOINTS,
            "missing_usable_checkpoint_count": max(
                DEFAULT_MINIMUM_FULL_STATE_CHECKPOINTS - usable_checkpoint_count,
                0,
            ),
            "projected_usable_checkpoint_count": projected_usable_checkpoint_count,
            "projected_minimum_usable_checkpoint_count_met": projected_minimum_usable_checkpoint_count_met,
            "projected_missing_usable_checkpoint_count": projected_missing_usable_checkpoint_count,
            **plan_context,
        }

    if usable_checkpoint_count < DEFAULT_MINIMUM_FULL_STATE_CHECKPOINTS:
        if not projected_minimum_usable_checkpoint_count_met:
            reason = (
                "current checkpoint stages can produce at most "
                f"{projected_usable_checkpoint_count} usable full-state checkpoint"
                f"{'' if projected_usable_checkpoint_count == 1 else 's'}; "
                f"need {DEFAULT_MINIMUM_FULL_STATE_CHECKPOINTS} before the mainnet "
                "validation stack is fully recoverable"
            )
        else:
            reason = (
                "need at least "
                f"{DEFAULT_MINIMUM_FULL_STATE_CHECKPOINTS} usable full-state "
                f"checkpoints; currently have {usable_checkpoint_count}"
            )
        return {
            "status": "waiting",
            "reason": reason,
            "actions": actions,
            "usable_checkpoint_count": usable_checkpoint_count,
            "minimum_usable_checkpoint_count": DEFAULT_MINIMUM_FULL_STATE_CHECKPOINTS,
            "minimum_usable_checkpoint_count_met": False,
            "missing_usable_checkpoint_count": max(
                DEFAULT_MINIMUM_FULL_STATE_CHECKPOINTS - usable_checkpoint_count,
                0,
            ),
            "projected_usable_checkpoint_count": projected_usable_checkpoint_count,
            "projected_minimum_usable_checkpoint_count_met": projected_minimum_usable_checkpoint_count_met,
            "projected_missing_usable_checkpoint_count": projected_missing_usable_checkpoint_count,
            **plan_context,
        }

    return {
        "status": "ready",
        "actions": actions,
        "usable_checkpoint_count": usable_checkpoint_count,
        "minimum_usable_checkpoint_count": DEFAULT_MINIMUM_FULL_STATE_CHECKPOINTS,
        "minimum_usable_checkpoint_count_met": True,
        "missing_usable_checkpoint_count": 0,
        "projected_usable_checkpoint_count": projected_usable_checkpoint_count,
        "projected_minimum_usable_checkpoint_count_met": True,
        "projected_missing_usable_checkpoint_count": 0,
        **plan_context,
    }


def build_restore_probe_command(
    *,
    height: int,
    checkpoint_root: Path,
    data_dir: Path,
    restore_script: Path,
) -> list[str]:
    scratch = restore_probe_root(height=height, data_dir=data_dir)
    return [
        str(restore_script),
        str(height),
        "--root",
        str(checkpoint_root),
        "--chain-db",
        str(scratch / "mainnet"),
        "--stateroot-db",
        str(scratch / "StateRoot"),
        "--yes",
        "--allow-unverified",
    ]


def restore_probe_root(*, height: int, data_dir: Path) -> Path:
    return data_dir / "checkpoint-restore-probes" / f"h{height}"


def restore_probe_paths(*, height: int, data_dir: Path) -> tuple[Path, Path]:
    scratch = restore_probe_root(height=height, data_dir=data_dir)
    return scratch / "mainnet", scratch / "StateRoot"


def verify_restored_checkpoint(
    action: dict,
    *,
    runner: Callable[[list[str]], Any],
) -> str:
    expected_height = int(action["height"])
    chain_height = parse_probe_chain_height(
        command_stdout(runner(action["restore_probe_chain_height_command"]))
    )
    if chain_height != expected_height:
        raise RuntimeError(
            "restored chain height does not match checkpoint height: "
            f"height={expected_height}, restored_chain_height={chain_height}"
        )

    stateroot_height = parse_probe_stateroot_height(
        command_stdout(runner(action["restore_probe_stateroot_height_command"]))
    )
    if stateroot_height != expected_height:
        raise RuntimeError(
            "restored StateRoot height does not match checkpoint height: "
            f"height={expected_height}, restored_stateroot_height={stateroot_height}"
        )

    restored_root = parse_probe_stateroot_root(
        command_stdout(runner(action["restore_probe_stateroot_root_command"]))
    )
    if not restored_root:
        raise RuntimeError(
            "restored StateRoot root is missing from checkpoint probe: "
            f"height={expected_height}"
        )

    expected_root = action.get("expected_stateroot_root")
    if expected_root is None:
        checkpoint_info = read_checkpoint_info(Path(action["checkpoint_path"]))
        expected_root = (
            checkpoint_info.get("expected_stateroot_root")
            or checkpoint_info.get("verified_stateroot_root")
        )
    if expected_root is not None and str(expected_root) != restored_root:
        raise RuntimeError(
            "restored StateRoot root does not match checkpoint proof: "
            f"height={expected_height}, expected={expected_root}, restored={restored_root}"
        )

    return restored_root


def mark_checkpoint_restore_verified(
    checkpoint_path: Path,
    height: int,
    *,
    verified_stateroot_root: str,
    verified_against_reference: bool,
) -> None:
    info = read_checkpoint_info(checkpoint_path)
    info.update(
        {
            "restore_verified": "true",
            "verified_height": str(height),
            "verified_stateroot_root": verified_stateroot_root,
            "verified_against_reference": str(verified_against_reference).lower(),
        }
    )
    original_keys = [key for key in info if key not in CHECKPOINT_VERIFICATION_FIELDS]
    ordered_keys = [*original_keys, *CHECKPOINT_VERIFICATION_FIELDS]
    (checkpoint_path / "CHECKPOINT_INFO").write_text(
        "".join(f"{key}={info[key]}\n" for key in ordered_keys if key in info),
        encoding="utf-8",
    )


def execute_plan(
    plan: dict,
    *,
    execute: bool,
    runner: Callable[[list[str]], Any] | None = None,
) -> dict:
    executed = 0
    restore_probed = 0
    skipped = 0
    runner = runner or run_command
    for action in plan.get("actions", []):
        if action.get("action") == "skip":
            skipped += 1
            continue
        if action.get("action") != "create":
            continue
        if execute:
            runner(action["command"])
            executed += 1
            if action.get("requires_restore_probe"):
                runner(action["restore_probe_command"])
                verified_stateroot_root = verify_restored_checkpoint(action, runner=runner)
                mark_checkpoint_restore_verified(
                    Path(action["checkpoint_path"]),
                    int(action["height"]),
                    verified_stateroot_root=verified_stateroot_root,
                    verified_against_reference=bool(
                        action.get("verified_against_reference")
                    ),
                )
                restore_probed += 1
    result = dict(plan)
    result["executed"] = executed
    result["restore_probed"] = restore_probed
    result["skipped"] = skipped
    result["dry_run"] = not execute
    if execute and plan.get("checkpoint_root") is not None:
        usable_checkpoint_count = count_usable_full_state_checkpoints(
            Path(plan["checkpoint_root"]),
            probe_bin=Path(plan.get("probe_bin") or DEFAULT_PROBE_BIN),
            inventory_runner=runner,
        )
        result["usable_checkpoint_count"] = usable_checkpoint_count
        result["minimum_usable_checkpoint_count"] = DEFAULT_MINIMUM_FULL_STATE_CHECKPOINTS
        result["minimum_usable_checkpoint_count_met"] = (
            usable_checkpoint_count >= DEFAULT_MINIMUM_FULL_STATE_CHECKPOINTS
        )
        result["missing_usable_checkpoint_count"] = max(
            DEFAULT_MINIMUM_FULL_STATE_CHECKPOINTS - usable_checkpoint_count,
            0,
        )
        result["projected_usable_checkpoint_count"] = usable_checkpoint_count
        result["projected_minimum_usable_checkpoint_count_met"] = (
            usable_checkpoint_count >= DEFAULT_MINIMUM_FULL_STATE_CHECKPOINTS
        )
        result["projected_missing_usable_checkpoint_count"] = max(
            DEFAULT_MINIMUM_FULL_STATE_CHECKPOINTS - usable_checkpoint_count,
            0,
        )
        if (
            result.get("status") == "waiting"
            and executed > 0
            and result["minimum_usable_checkpoint_count_met"]
            and not any(action.get("action") == "blocked" for action in plan.get("actions", []))
            and not any(action.get("action") == "defer" for action in plan.get("actions", []))
        ):
            result["status"] = "ready"
            result.pop("reason", None)
    return result


def run_watch_loop(
    *,
    status_loader: Callable[[], dict],
    plan_builder: Callable[[dict], dict],
    execute: bool,
    runner: Callable[[list[str]], Any] | None,
    interval_seconds: int,
    waiting_interval_seconds: int | None = None,
    max_iterations: int | None = None,
    sleep_fn: Callable[[int], Any] = time.sleep,
    result_writer: Callable[[dict], Any] | None = None,
) -> dict:
    iterations = 0
    total_executed = 0
    total_skipped = 0

    while True:
        status = status_loader()
        plan = plan_builder(status)
        result = execute_plan(plan, execute=execute, runner=runner)
        iterations += 1
        total_executed += result.get("executed", 0)
        total_skipped += result.get("skipped", 0)
        if result_writer is not None:
            result_writer(result)

        if max_iterations is not None and iterations >= max_iterations:
            break
        sleep_seconds = interval_seconds
        if result.get("status") == "waiting" and waiting_interval_seconds is not None:
            sleep_seconds = min(interval_seconds, waiting_interval_seconds)
        sleep_fn(sleep_seconds)

    return {
        "mode": "watch",
        "iterations": iterations,
        "executed": total_executed,
        "skipped": total_skipped,
        "dry_run": not execute,
    }


def run_command(command: list[str]) -> None:
    return subprocess.run(
        command,
        check=True,
        capture_output=True,
        text=True,
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Maintain base/mid/latest checkpoints from a state-root status file."
    )
    parser.add_argument(
        "--status-file",
        default=DEFAULT_STATUS_FILE,
        help=f"Continuous validator status JSON (default: {DEFAULT_STATUS_FILE})",
    )
    parser.add_argument(
        "--node-config",
        default=None,
        help="Optional neo-node TOML used to infer --data-dir, --chain-db, and --stateroot-db.",
    )
    parser.add_argument(
        "--data-dir",
        default=None,
        help=f"neo-rs data root passed to checkpoint-on-height (default: {DEFAULT_DATA_DIR})",
    )
    parser.add_argument(
        "--chain-db",
        default=None,
        help="Explicit chain RocksDB path for validation/replay layouts.",
    )
    parser.add_argument(
        "--stateroot-db",
        default=None,
        help="Explicit StateRoot RocksDB path for validation/replay layouts.",
    )
    parser.add_argument(
        "--root",
        default=None,
        help="Checkpoint root (default: <data-dir>/checkpoints)",
    )
    parser.add_argument(
        "--writer-pid",
        default="none",
        help="neo-node writer PID, or 'none' for stopped/offline data dirs",
    )
    parser.add_argument(
        "--script",
        default=DEFAULT_CHECKPOINT_SCRIPT,
        help=f"Checkpoint script path (default: {DEFAULT_CHECKPOINT_SCRIPT})",
    )
    parser.add_argument(
        "--probe-bin",
        default=DEFAULT_PROBE_BIN,
        help=f"neo-db-probe binary used for restore verification (default: {DEFAULT_PROBE_BIN})",
    )
    parser.add_argument(
        "--execute",
        action="store_true",
        help="Actually create missing checkpoints. Default is JSON dry-run only.",
    )
    parser.add_argument(
        "--watch-interval",
        type=int,
        default=None,
        help="Repeat maintenance every N seconds. Default runs once.",
    )
    parser.add_argument(
        "--waiting-interval",
        type=int,
        default=DEFAULT_WAITING_INTERVAL_SECONDS,
        help=(
            "Retry interval while the validator status has no checkpoint stages "
            f"(default: {DEFAULT_WAITING_INTERVAL_SECONDS}s)."
        ),
    )
    parser.add_argument(
        "--max-iterations",
        type=int,
        default=None,
        help=argparse.SUPPRESS,
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    inferred_paths = (
        derive_checkpoint_paths_from_config(Path(args.node_config))
        if args.node_config
        else {}
    )
    data_dir = Path(args.data_dir) if args.data_dir else inferred_paths.get("data_dir", Path(DEFAULT_DATA_DIR))
    checkpoint_root = Path(args.root) if args.root else data_dir / "checkpoints"
    chain_db = Path(args.chain_db) if args.chain_db else inferred_paths.get("chain_db")
    stateroot_db = (
        Path(args.stateroot_db)
        if args.stateroot_db
        else inferred_paths.get("stateroot_db")
    )

    def status_loader() -> dict:
        return load_status(Path(args.status_file))

    def plan_builder(status: dict) -> dict:
        return build_checkpoint_plan(
            status,
            checkpoint_root=checkpoint_root,
            data_dir=data_dir,
            writer_pid=args.writer_pid,
            script_path=Path(args.script),
            chain_db=chain_db,
            stateroot_db=stateroot_db,
            probe_bin=Path(args.probe_bin),
            inventory_runner=run_command,
        )

    try:
        if args.watch_interval is None:
            result = execute_plan(plan_builder(status_loader()), execute=args.execute)
            print(json.dumps(result, indent=2, sort_keys=True))
        else:
            if args.watch_interval < 1:
                raise ValueError("--watch-interval must be >= 1")
            if args.waiting_interval < 1:
                raise ValueError("--waiting-interval must be >= 1")

            def write_result(result: dict) -> None:
                print(json.dumps(result, sort_keys=True), flush=True)

            summary = run_watch_loop(
                status_loader=status_loader,
                plan_builder=plan_builder,
                execute=args.execute,
                runner=None,
                interval_seconds=args.watch_interval,
                waiting_interval_seconds=args.waiting_interval,
                max_iterations=args.max_iterations,
                result_writer=write_result,
            )
            print(json.dumps(summary, indent=2, sort_keys=True))
    except Exception as exc:  # pylint: disable=broad-except
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
