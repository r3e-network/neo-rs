#!/usr/bin/env python3
"""Prepare an isolated clean MainNet StateService validation workspace."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any


DEFAULT_BASE_CONFIG = "neo_mainnet_validate.toml"
DEFAULT_WORK_ROOT = "data/mainnet-stateroot-clean"
DEFAULT_NODE_BIN = "target/debug/neo-node"
DEFAULT_PROBE_BIN = "target/debug/neo-db-probe"
DEFAULT_SMOKE_TARGET_HEIGHT = 10
DEFAULT_REFERENCE_RPCS = [
    "http://seed1.neo.org:10332",
    "http://seed2.neo.org:10332",
    "http://seed3.neo.org:10332",
    "http://seed4.neo.org:10332",
    "http://seed5.neo.org:10332",
]


def toml_value(value: Any) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, int):
        return str(value)
    return json.dumps(str(value))


def rewrite_config_text(text: str, replacements: dict[str, dict[str, Any]]) -> str:
    lines = text.splitlines()
    output: list[str] = []
    current_section = ""
    seen: dict[str, set[str]] = {section: set() for section in replacements}
    section_header = re.compile(r"^\s*\[([^\]]+)\]\s*$")
    key_line = re.compile(r"^(\s*)([A-Za-z0-9_]+)(\s*=).*$")

    def append_missing(section: str) -> None:
        for key, value in replacements.get(section, {}).items():
            if key not in seen[section] and value is not None:
                output.append(f"{key} = {toml_value(value)}")
                seen[section].add(key)

    for line in lines:
        header = section_header.match(line)
        if header:
            append_missing(current_section)
            current_section = header.group(1)
            output.append(line)
            continue

        match = key_line.match(line)
        if match and current_section in replacements:
            key = match.group(2)
            if key in replacements[current_section]:
                if replacements[current_section][key] is None:
                    seen[current_section].add(key)
                    continue
                indent = match.group(1)
                output.append(f"{indent}{key} = {toml_value(replacements[current_section][key])}")
                seen[current_section].add(key)
                continue
        output.append(line)

    append_missing(current_section)
    return "\n".join(output) + "\n"


def build_replacements(
    *,
    work_root: Path,
    rpc_port: int,
    p2p_port: int,
    metrics_port: int,
) -> dict[str, dict[str, Any]]:
    chain_db = work_root / "chain"
    logs = work_root / "logs"
    return {
        "storage": {
            "data_dir": chain_db,
            "read_only": False,
            "mdbx_geometry_upper_gb": 512,
            "mdbx_geometry_growth_mb": 256,
            "mdbx_max_readers": 4096,
        },
        "p2p": {
            "port": p2p_port,
        },
        "rpc": {
            "port": rpc_port,
            "bind_address": "127.0.0.1",
            "auth_enabled": False,
        },
        "indexer": {
            "enabled": False,
            "store_path": work_root / "indexer",
        },
        "application_logs": {
            "enabled": False,
            "path": work_root / "application-logs",
        },
        "tokens_tracker": {
            "enabled": False,
            "db_path": work_root / "tokens",
        },
        "telemetry.metrics": {
            "port": metrics_port,
            "bind_address": "127.0.0.1",
        },
        "logging": {
            "file_path": logs / "neo-node-validate.log",
        },
        "state_service": {
            "enabled": True,
            "path": None,
            "full_state": True,
            "track_during_catchup": True,
        },
    }


def build_plan(
    *,
    base_config: Path,
    work_root: Path,
    rpc_port: int,
    p2p_port: int,
    metrics_port: int,
    smoke_target_height: int,
    node_bin: Path,
    probe_bin: Path,
) -> dict[str, Any]:
    config_path = work_root / "neo_mainnet_validate.toml"
    log_dir = work_root / "logs"
    pid_dir = work_root / "pids"
    status_file = work_root / "stateroot-validation.json"
    resume_file = work_root / "stateroot-last-validated"
    milestone_heights = [
        smoke_target_height,
        smoke_target_height * 2,
        smoke_target_height * 3,
    ]
    return {
        "mode": "clean-stateroot-validation-workspace",
        "base_config": str(base_config),
        "work_root": str(work_root),
        "config_path": str(config_path),
        "chain_db": str(work_root / "chain"),
        "rpc_port": rpc_port,
        "p2p_port": p2p_port,
        "metrics_port": metrics_port,
        "commands": {
            "preflight": [
                str(node_bin),
                "--config",
                str(config_path),
                "--enable-stateroot",
                "--check-all",
            ],
            "recovery-plan": [
                "python3",
                "scripts/plan-stateroot-recovery.py",
                "--node-config",
                str(config_path),
                "--probe-bin",
                str(probe_bin),
            ],
            "bounded-smoke": [
                "python3",
                "scripts/run-bounded-mainnet-replay.py",
                "--config",
                str(config_path),
                "--node-bin",
                str(node_bin),
                "--rpc",
                f"http://127.0.0.1:{rpc_port}",
                "--target-height",
                str(smoke_target_height),
                "--db",
                str(work_root / "chain"),
                "--probe-bin",
                str(probe_bin),
                "--require-stateroot-height-match",
                "--reference",
                ",".join(DEFAULT_REFERENCE_RPCS),
                "--require-reference-stateroot-match",
                "--node-output-log",
                str(log_dir / "neo-node-bounded-smoke.log"),
            ],
            "checkpoint-smoke": [
                "scripts/checkpoint-on-height.sh",
                "none",
                "--once",
                "--height",
                str(smoke_target_height),
                "--data-dir",
                str(work_root),
                "--chain-db",
                str(work_root / "chain"),
                "--root",
                str(work_root / "checkpoints"),
            ],
            "milestone-smoke": [
                "python3",
                "scripts/run-stateroot-milestones.py",
                "--config",
                str(config_path),
                "--node-bin",
                str(node_bin),
                "--rpc",
                f"http://127.0.0.1:{rpc_port}",
                "--milestone",
                ",".join(str(height) for height in milestone_heights),
                "--chain-db",
                str(work_root / "chain"),
                "--probe-bin",
                str(probe_bin),
                "--reference",
                ",".join(DEFAULT_REFERENCE_RPCS),
                "--data-dir",
                str(work_root),
                "--checkpoint-root",
                str(work_root / "checkpoints"),
                "--log-dir",
                str(log_dir),
                "--summary-jsonl",
                str(work_root / "milestone-summary.jsonl"),
                "--fast-sync",
                "--fast-sync-cache",
                str(work_root / "fast-sync-cache"),
                "--initial-height",
                "0",
            ],
            "start-stack": [
                "python3",
                "scripts/run-mainnet-validation-stack.py",
                "--start",
                "--node-config",
                str(config_path),
                "--node-bin",
                str(node_bin),
                "--status-file",
                str(status_file),
                "--resume-file",
                str(resume_file),
                "--log-dir",
                str(log_dir),
                "--pid-dir",
                str(pid_dir),
                "--checkpoint-execute",
            ],
        },
    }


def prepare_workspace(
    *,
    base_config: Path,
    work_root: Path,
    rpc_port: int,
    p2p_port: int,
    metrics_port: int,
    smoke_target_height: int,
    node_bin: Path,
    probe_bin: Path,
    dry_run: bool,
    force: bool,
) -> dict[str, Any]:
    if not base_config.is_file():
        raise FileNotFoundError(f"base config not found: {base_config}")
    config_path = work_root / "neo_mainnet_validate.toml"
    if work_root.exists() and not force and not dry_run:
        raise FileExistsError(f"work root already exists: {work_root}")

    plan = build_plan(
        base_config=base_config,
        work_root=work_root,
        rpc_port=rpc_port,
        p2p_port=p2p_port,
        metrics_port=metrics_port,
        smoke_target_height=smoke_target_height,
        node_bin=node_bin,
        probe_bin=probe_bin,
    )
    if dry_run:
        plan["dry_run"] = True
        return plan

    work_root.mkdir(parents=True, exist_ok=force)
    (work_root / "logs").mkdir(parents=True, exist_ok=True)
    config_text = rewrite_config_text(
        base_config.read_text(encoding="utf-8"),
        build_replacements(
            work_root=work_root.resolve(),
            rpc_port=rpc_port,
            p2p_port=p2p_port,
            metrics_port=metrics_port,
        ),
    )
    config_path.write_text(config_text, encoding="utf-8")
    plan["dry_run"] = False
    return plan


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Create a fresh isolated config for full MainNet StateRoot replay."
    )
    parser.add_argument("--base-config", default=DEFAULT_BASE_CONFIG)
    parser.add_argument("--work-root", default=DEFAULT_WORK_ROOT)
    parser.add_argument("--rpc-port", type=int, default=21332)
    parser.add_argument("--p2p-port", type=int, default=21333)
    parser.add_argument("--metrics-port", type=int, default=21990)
    parser.add_argument("--smoke-target-height", type=int, default=DEFAULT_SMOKE_TARGET_HEIGHT)
    parser.add_argument("--node-bin", default=DEFAULT_NODE_BIN)
    parser.add_argument("--probe-bin", default=DEFAULT_PROBE_BIN)
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument(
        "--force",
        action="store_true",
        help="Allow writing into an existing work root. Existing config is replaced.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        plan = prepare_workspace(
            base_config=Path(args.base_config),
            work_root=Path(args.work_root),
            rpc_port=args.rpc_port,
            p2p_port=args.p2p_port,
            metrics_port=args.metrics_port,
            smoke_target_height=args.smoke_target_height,
            node_bin=Path(args.node_bin),
            probe_bin=Path(args.probe_bin),
            dry_run=args.dry_run,
            force=args.force,
        )
    except Exception as exc:  # pylint: disable=broad-except
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1
    print(json.dumps(plan, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
