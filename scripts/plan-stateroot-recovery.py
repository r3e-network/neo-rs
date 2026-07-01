#!/usr/bin/env python3
"""Plan recovery from a chain/StateService MPT height mismatch."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Any, Callable


DEFAULT_NODE_CONFIG = "neo_mainnet_validate.toml"
DEFAULT_PROBE_BIN = "target/debug/neo-db-probe"
DEFAULT_CHECKPOINT_SCRIPT = "scripts/restore-checkpoint.sh"
DEFAULT_STACK_RUNNER = "scripts/run-mainnet-validation-stack.py"
DEFAULT_CLEAN_PREP_SCRIPT = "scripts/prepare-clean-stateroot-validation.py"
DEFAULT_DATA_DIR = "./data"
DEFAULT_MINIMUM_FULL_STATE_CHECKPOINTS = 3


def format_network_magic(value: Any) -> str:
    if isinstance(value, int):
        return f"{value:08X}"
    if isinstance(value, str):
        return f"{int(value.strip(), 0):08X}"
    raise ValueError(f"unsupported network_magic value: {value!r}")


def network_scoped_path(path: str, network_magic: str) -> Path:
    return Path(path.replace("{0}", network_magic))


def load_config_paths(config_path: Path) -> dict[str, Any]:
    with config_path.open("rb") as handle:
        config = tomllib.load(handle)

    storage = config.get("storage") or {}
    state_service = config.get("state_service") or {}
    network = config.get("network") or {}

    network_magic = format_network_magic(network.get("network_magic", 0))
    chain_db = Path(storage.get("data_dir") or storage.get("path") or DEFAULT_DATA_DIR)
    data_dir = chain_db.parent if chain_db.name else Path(DEFAULT_DATA_DIR)
    stateroot_template = str(
        state_service.get("path") or f"{DEFAULT_DATA_DIR}/Plugins/mainnet/StateRoot"
    )
    stateroot_db = network_scoped_path(stateroot_template, network_magic)

    return {
        "network_magic": network_magic,
        "data_dir": data_dir,
        "chain_db": chain_db,
        "stateroot_db": stateroot_db,
        "state_service_enabled": bool(state_service.get("enabled", False)),
        "track_during_catchup": bool(state_service.get("track_during_catchup", False)),
    }


def run_json_command(
    command: list[str],
    *,
    runner: Callable[..., Any] = subprocess.run,
) -> dict[str, Any]:
    completed = runner(command, check=True, capture_output=True, text=True)
    return json.loads(completed.stdout)


def probe_chain_height(
    *,
    probe_bin: Path,
    chain_db: Path,
    runner: Callable[..., Any] = subprocess.run,
) -> dict[str, Any]:
    if not chain_db.exists():
        return {"path": str(chain_db), "height": None, "found": False}
    command = [
        str(probe_bin),
        "--db",
        str(chain_db),
        "--contract-id",
        "-4",
        "--key-hex",
        "0c",
        "--decode",
        "hash-index",
    ]
    try:
        payload = run_json_command(command, runner=runner)
        height = payload.get("decoded", {}).get("index") if payload.get("found") else None
        return {"path": str(chain_db), "height": height, "found": bool(payload.get("found"))}
    except Exception as exc:  # pylint: disable=broad-except
        return {"path": str(chain_db), "height": None, "error": str(exc)}


def probe_stateroot_height(
    *,
    probe_bin: Path,
    stateroot_db: Path,
    runner: Callable[..., Any] = subprocess.run,
) -> dict[str, Any]:
    if not (stateroot_db / "CURRENT").exists():
        return {"path": str(stateroot_db), "height": None, "found": False}
    command = [str(probe_bin), "--db", str(stateroot_db), "--mpt-state-height"]
    try:
        payload = run_json_command(command, runner=runner)
        decoded = payload.get("height", {}).get("decoded") or {}
        height = decoded.get("current_local_root_index")
        return {
            "path": str(stateroot_db),
            "height": height,
            "found": bool(payload.get("height", {}).get("found")),
        }
    except Exception as exc:  # pylint: disable=broad-except
        return {"path": str(stateroot_db), "height": None, "error": str(exc)}


def metadata_value(path: Path, key: str) -> str | None:
    info = path / "CHECKPOINT_INFO"
    if not info.exists():
        return None
    for line in info.read_text(encoding="utf-8").splitlines():
        if line.startswith(f"{key}="):
            return line.split("=", 1)[1]
    return None


def checkpoint_verification_reason(path: Path) -> str | None:
    height = metadata_value(path, "height")
    verified_height = metadata_value(path, "verified_height")
    verified_stateroot_root = metadata_value(path, "verified_stateroot_root")
    restore_verified = metadata_value(path, "restore_verified")
    verified_against_reference = metadata_value(path, "verified_against_reference")
    missing = [
        name
        for name, value in [
            ("restore_verified", restore_verified),
            ("verified_height", verified_height),
            ("verified_stateroot_root", verified_stateroot_root),
            ("verified_against_reference", verified_against_reference),
        ]
        if not value
    ]
    if missing:
        return "missing restore verification metadata: " + ", ".join(missing)
    if height is not None and verified_height != height:
        return (
            "restore verification height does not match checkpoint height: "
            f"height={height}, verified_height={verified_height}"
        )
    if restore_verified.lower() != "true":
        return "restore verification metadata is not marked restore_verified=true"
    if verified_against_reference.lower() != "true":
        return "restore verification metadata is not marked verified_against_reference=true"
    return None


def checkpoint_height(path: Path) -> int | None:
    name = path.name
    if name.startswith("h") and name[1:].isdigit():
        return int(name[1:])
    return None


def checkpoint_has_chain(path: Path) -> bool:
    return (path / "mainnet").is_dir()


def checkpoint_has_stateroot(path: Path) -> bool:
    if not (path / "StateRoot").is_dir():
        return False
    if metadata_value(path, "state_root_included") == "false":
        return False
    return True


def scan_checkpoints(root: Path) -> list[dict[str, Any]]:
    if not root.is_dir():
        return []
    checkpoints = []
    for path in sorted(item for item in root.iterdir() if item.is_dir()):
        height = checkpoint_height(path)
        if height is None:
            continue
        has_chain = checkpoint_has_chain(path)
        has_stateroot = checkpoint_has_stateroot(path)
        verification_reason = (
            checkpoint_verification_reason(path) if has_stateroot else None
        )
        checkpoints.append(
            {
                "path": str(path),
                "label": metadata_value(path, "label") or path.name,
                "height": height,
                "has_chain": has_chain,
                "has_stateroot": has_stateroot,
                "usable_for_state_validation": bool(
                    has_chain and has_stateroot and verification_reason is None
                ),
                "restore_verification_reason": verification_reason,
                "mode": metadata_value(path, "mode") or "height-labelled",
            }
        )
    return sorted(checkpoints, key=lambda item: int(item["height"]))


def choose_full_state_checkpoint(
    checkpoints: list[dict[str, Any]],
    chain_height: int | None,
) -> dict[str, Any] | None:
    candidates = [
        item
        for item in checkpoints
        if item["usable_for_state_validation"]
        and (chain_height is None or int(item["height"]) <= chain_height)
    ]
    return candidates[-1] if candidates else None


def checkpoint_inventory_summary(
    checkpoints: list[dict[str, Any]],
    *,
    minimum_full_state_checkpoints: int = DEFAULT_MINIMUM_FULL_STATE_CHECKPOINTS,
) -> dict[str, Any]:
    full_state = [item for item in checkpoints if item["usable_for_state_validation"]]
    count = len(full_state)
    missing = max(minimum_full_state_checkpoints - count, 0)
    return {
        "usable_full_state_count": count,
        "minimum_full_state_count": minimum_full_state_checkpoints,
        "minimum_full_state_count_met": count >= minimum_full_state_checkpoints,
        "missing_full_state_count": missing,
        "usable_full_state_heights": [int(item["height"]) for item in full_state],
    }


def build_recovery_plan(
    *,
    node_config: Path,
    checkpoint_root: Path | None = None,
    probe_bin: Path = Path(DEFAULT_PROBE_BIN),
    restore_script: Path = Path(DEFAULT_CHECKPOINT_SCRIPT),
    stack_runner: Path = Path(DEFAULT_STACK_RUNNER),
    clean_prep_script: Path = Path(DEFAULT_CLEAN_PREP_SCRIPT),
    runner: Callable[..., Any] = subprocess.run,
) -> dict[str, Any]:
    paths = load_config_paths(node_config)
    checkpoint_root = checkpoint_root or paths["data_dir"] / "checkpoints"

    chain = probe_chain_height(
        probe_bin=probe_bin,
        chain_db=paths["chain_db"],
        runner=runner,
    )
    stateroot = probe_stateroot_height(
        probe_bin=probe_bin,
        stateroot_db=paths["stateroot_db"],
        runner=runner,
    )
    checkpoints = scan_checkpoints(checkpoint_root)
    chain_height = chain.get("height")
    stateroot_height = stateroot.get("height")
    heights_match = (
        chain_height is not None
        and stateroot_height is not None
        and int(chain_height) == int(stateroot_height)
    )

    full_state = [item for item in checkpoints if item["usable_for_state_validation"]]
    full_state_checkpoint = choose_full_state_checkpoint(checkpoints, chain_height)
    chain_only = [item for item in checkpoints if item["has_chain"] and not item["has_stateroot"]]
    checkpoint_summary = checkpoint_inventory_summary(checkpoints)

    if chain_height is None and "error" not in chain:
        mode = "fresh-replay-ready"
        recommended_action = {
            "action": "start-validation-stack",
            "reason": "configured chain store has no persisted Ledger height yet",
            "commands": [
                [
                    "python3",
                    str(stack_runner),
                    "--start",
                    "--node-config",
                    str(node_config),
                ]
            ],
        }
    elif heights_match:
        mode = "ready"
        recommended_action = {
            "action": "start-validation-stack",
            "reason": "chain and StateService MPT heights match",
            "commands": [
                [
                    "python3",
                    str(stack_runner),
                    "--start",
                    "--node-config",
                    str(node_config),
                ]
            ],
        }
    elif full_state_checkpoint is not None:
        mode = "restore-full-state-checkpoint"
        recommended_action = {
            "action": "restore-full-state-checkpoint",
            "reason": (
                "nearest checkpoint includes both chain DB and StateRoot, so it can "
                "resume state-root validation without recomputing from genesis"
            ),
            "checkpoint": full_state_checkpoint,
            "commands": [
                [
                    str(restore_script),
                    str(full_state_checkpoint["height"]),
                    "--root",
                    str(checkpoint_root),
                    "--chain-db",
                    str(paths["chain_db"]),
                    "--stateroot-db",
                    str(paths["stateroot_db"]),
                    "--keep-current",
                    "--dry-run",
                ]
            ],
        }
    else:
        mode = "clean-replay-required"
        recommended_action = {
            "action": "clean-replay-from-genesis",
            "reason": (
                "no checkpoint with a matching StateRoot store is available; chain-only "
                "checkpoints cannot be used for StateService validation"
            ),
            "commands": [
                [
                    "python3",
                    str(clean_prep_script),
                    "--base-config",
                    str(node_config),
                    "--work-root",
                    "data/mainnet-stateroot-clean",
                ]
            ],
            "manual_steps": [
                "review the generated clean workspace plan",
                "run the generated preflight command and confirm --check-all passes",
                "start the validation stack with the generated clean config",
            ],
        }

    return {
        "mode": mode,
        "node_config": str(node_config),
        "checkpoint_root": str(checkpoint_root),
        "network_magic": paths["network_magic"],
        "chain": chain,
        "state_service": {
            **stateroot,
            "enabled": paths["state_service_enabled"],
            "track_during_catchup": paths["track_during_catchup"],
            "matches_chain": heights_match,
        },
        "checkpoints": {
            "full_state": full_state,
            "chain_only": chain_only,
            "all": checkpoints,
            **checkpoint_summary,
        },
        "recommended_action": recommended_action,
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Plan how to recover MainNet StateService validation from current storage."
    )
    parser.add_argument("--node-config", default=DEFAULT_NODE_CONFIG)
    parser.add_argument("--checkpoint-root", default=None)
    parser.add_argument("--probe-bin", default=DEFAULT_PROBE_BIN)
    parser.add_argument("--restore-script", default=DEFAULT_CHECKPOINT_SCRIPT)
    parser.add_argument("--stack-runner", default=DEFAULT_STACK_RUNNER)
    parser.add_argument("--clean-prep-script", default=DEFAULT_CLEAN_PREP_SCRIPT)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        plan = build_recovery_plan(
            node_config=Path(args.node_config),
            checkpoint_root=Path(args.checkpoint_root) if args.checkpoint_root else None,
            probe_bin=Path(args.probe_bin),
            restore_script=Path(args.restore_script),
            stack_runner=Path(args.stack_runner),
            clean_prep_script=Path(args.clean_prep_script),
        )
    except Exception as exc:  # pylint: disable=broad-except
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1
    print(json.dumps(plan, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
