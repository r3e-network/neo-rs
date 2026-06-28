"""Shared helpers for the MainNet state-root validation process stack."""

from __future__ import annotations

import os
import signal
import subprocess
from pathlib import Path
from typing import Callable, Iterable, Any


DEFAULT_NODE_CONFIG = "neo_mainnet_validate.toml"
DEFAULT_NODE_BIN = "target/release/neo-node"
DEFAULT_STATUS_FILE = "/tmp/stateroot-validation.json"
DEFAULT_RESUME_FILE = "/tmp/stateroot-last-validated"
DEFAULT_LOG_DIR = "logs/mainnet-validation"
DEFAULT_PID_DIR = "logs/mainnet-validation/pids"
DEFAULT_CHECKPOINT_WATCH_INTERVAL = 600
DEFAULT_CHECKPOINT_WAITING_INTERVAL = 30
RUNTIME_STEP_NAMES = ["node", "state-root-validator", "checkpoint-maintainer"]
PID_FILES = {
    "node": "neo-node.pid",
    "state-root-validator": "state-root-validator.pid",
    "checkpoint-maintainer": "checkpoint-maintainer.pid",
}


def as_str(path: Path) -> str:
    return str(path)


def build_plan(
    *,
    node_config: Path,
    node_bin: Path,
    status_file: Path,
    resume_file: Path,
    log_dir: Path,
    batch: int,
    poll_interval: int,
    checkpoint_execute: bool,
    checkpoint_watch_interval: int = DEFAULT_CHECKPOINT_WATCH_INTERVAL,
    checkpoint_waiting_interval: int = DEFAULT_CHECKPOINT_WAITING_INTERVAL,
) -> dict:
    node_config_arg = as_str(node_config)
    node_bin_arg = as_str(node_bin)
    status_file_arg = as_str(status_file)
    resume_file_arg = as_str(resume_file)
    log_dir_arg = as_str(log_dir)

    checkpoint_command = [
        "python3",
        "scripts/maintain-stateroot-checkpoints.py",
        "--node-config",
        node_config_arg,
        "--status-file",
        status_file_arg,
        "--writer-pid",
        "<neo-node-pid>",
        "--watch-interval",
        str(checkpoint_watch_interval),
        "--waiting-interval",
        str(checkpoint_waiting_interval),
    ]
    if checkpoint_execute:
        checkpoint_command.append("--execute")

    return {
        "mode": "dry-run",
        "node_config": node_config_arg,
        "status_file": status_file_arg,
        "resume_file": resume_file_arg,
        "log_dir": log_dir_arg,
        "steps": [
            {
                "name": "preflight",
                "purpose": (
                    "Validate node config and storage before starting services, including "
                    "chain/StateService MPT height consistency."
                ),
                "command": [
                    node_bin_arg,
                    "--config",
                    node_config_arg,
                    "--check-all",
                ],
                "failure_hint": (
                    "If StateService MPT height does not match chain height, restore a matching "
                    "StateRoot checkpoint or replay from genesis with track_during_catchup enabled."
                ),
            },
            {
                "name": "node",
                "purpose": "Run the isolated MainNet validation node.",
                "command": [
                    node_bin_arg,
                    "--config",
                    node_config_arg,
                ],
                "stdout_log": f"{log_dir_arg}/neo-node.log",
            },
            {
                "name": "state-root-validator",
                "purpose": "Continuously compare local state roots with Neo reference RPCs.",
                "command": [
                    "python3",
                    "scripts/continuous-stateroot-validation.py",
                    "--local-config",
                    node_config_arg,
                    "--status-file",
                    status_file_arg,
                    "--resume-file",
                    resume_file_arg,
                    "--batch",
                    str(batch),
                    "--poll-interval",
                    str(poll_interval),
                ],
                "stdout_log": f"{log_dir_arg}/state-root-validator.log",
            },
            {
                "name": "checkpoint-maintainer",
                "purpose": "Create or refresh base/mid/latest checkpoints from validator status.",
                "command": checkpoint_command,
                "stdout_log": f"{log_dir_arg}/checkpoint-maintainer.log",
            },
        ],
    }


def step_by_name(plan: dict, name: str) -> dict:
    for step in plan.get("steps", []):
        if step.get("name") == name:
            return step
    raise KeyError(f"plan is missing step: {name}")


def command_with_writer_pid(command: Iterable[str], node_pid: int) -> list[str]:
    return [str(node_pid) if value == "<neo-node-pid>" else value for value in command]


def pid_file(pid_dir: Path, name: str) -> Path:
    return pid_dir / PID_FILES[name]


def write_pid(pid_dir: Path, name: str, pid: int) -> None:
    pid_dir.mkdir(parents=True, exist_ok=True)
    pid_file(pid_dir, name).write_text(f"{pid}\n", encoding="utf-8")


def read_pid(path: Path) -> int | None:
    if not path.exists():
        return None
    try:
        return int(path.read_text(encoding="utf-8").strip())
    except ValueError:
        return None


def is_pid_running(pid: int) -> bool:
    if pid <= 0:
        return False
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def spawn_step(
    step: dict,
    *,
    command: list[str],
    pid_dir: Path,
    spawner: Callable[..., Any],
) -> Any:
    log_path = Path(step["stdout_log"])
    log_path.parent.mkdir(parents=True, exist_ok=True)
    with log_path.open("ab") as log_handle:
        process = spawner(
            command,
            stdout=log_handle,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )
    write_pid(pid_dir, str(step["name"]), int(process.pid))
    return process


def preflight_failure_payload(
    *,
    command: list[str],
    returncode: int | None,
    stdout: str = "",
    stderr: str = "",
) -> dict:
    return {
        "ok": False,
        "command": command,
        "returncode": returncode,
        "stdout": stdout or "",
        "stderr": stderr or "",
        "hint": (
            "Fix the reported config/storage issue before starting the validation stack. "
            "For StateService height mismatches, restore a matching StateRoot checkpoint "
            "or replay from genesis with [state_service].track_during_catchup = true. "
            "Run scripts/plan-stateroot-recovery.py for a read-only recovery plan."
        ),
    }


def run_preflight(step: dict, runner: Callable[..., Any]) -> dict:
    command = list(step["command"])
    try:
        completed = runner(command, check=True, capture_output=True, text=True)
    except TypeError:
        try:
            completed = runner(command, check=True)
        except subprocess.CalledProcessError as error:
            return preflight_failure_payload(
                command=command,
                returncode=error.returncode,
                stdout=getattr(error, "stdout", None) or getattr(error, "output", "") or "",
                stderr=getattr(error, "stderr", "") or "",
            )
        except OSError as error:
            return preflight_failure_payload(
                command=command,
                returncode=None,
                stderr=str(error),
            )
    except subprocess.CalledProcessError as error:
        return preflight_failure_payload(
            command=command,
            returncode=error.returncode,
            stdout=getattr(error, "stdout", None) or getattr(error, "output", "") or "",
            stderr=getattr(error, "stderr", "") or "",
        )
    except OSError as error:
        return preflight_failure_payload(
            command=command,
            returncode=None,
            stderr=str(error),
        )

    return {
        "ok": True,
        "command": command,
        "returncode": getattr(completed, "returncode", 0),
    }


def start_stack(
    plan: dict,
    *,
    pid_dir: Path,
    preflight_runner: Callable[..., Any] = subprocess.run,
    spawner: Callable[..., Any] = subprocess.Popen,
) -> dict:
    preflight = step_by_name(plan, "preflight")
    preflight_result = run_preflight(preflight, preflight_runner)
    if not preflight_result["ok"]:
        return {
            "mode": "preflight-failed",
            "pid_dir": str(pid_dir),
            "preflight": preflight_result,
            "processes": [],
        }

    pid_dir.mkdir(parents=True, exist_ok=True)
    processes = []

    node_step = step_by_name(plan, "node")
    node_process = spawn_step(
        node_step,
        command=node_step["command"],
        pid_dir=pid_dir,
        spawner=spawner,
    )
    processes.append({"name": "node", "pid": int(node_process.pid)})

    validator_step = step_by_name(plan, "state-root-validator")
    validator_process = spawn_step(
        validator_step,
        command=validator_step["command"],
        pid_dir=pid_dir,
        spawner=spawner,
    )
    processes.append({"name": "state-root-validator", "pid": int(validator_process.pid)})

    checkpoint_step = step_by_name(plan, "checkpoint-maintainer")
    checkpoint_command = command_with_writer_pid(
        checkpoint_step["command"],
        int(node_process.pid),
    )
    checkpoint_process = spawn_step(
        checkpoint_step,
        command=checkpoint_command,
        pid_dir=pid_dir,
        spawner=spawner,
    )
    processes.append({"name": "checkpoint-maintainer", "pid": int(checkpoint_process.pid)})

    return {
        "mode": "started",
        "pid_dir": str(pid_dir),
        "preflight": preflight_result,
        "processes": processes,
    }


def stack_status(
    pid_dir: Path,
    *,
    checker: Callable[[int], bool] = is_pid_running,
) -> dict:
    processes = []
    for name in RUNTIME_STEP_NAMES:
        pid = read_pid(pid_file(pid_dir, name))
        processes.append(
            {
                "name": name,
                "pid": pid,
                "running": bool(pid is not None and checker(pid)),
            }
        )
    return {"mode": "status", "pid_dir": str(pid_dir), "processes": processes}


def stop_stack(
    pid_dir: Path,
    *,
    checker: Callable[[int], bool] = is_pid_running,
    killer: Callable[[int], Any] | None = None,
) -> dict:
    killer = killer or (lambda pid: os.kill(pid, signal.SIGTERM))
    processes = []
    for name in reversed(RUNTIME_STEP_NAMES):
        path = pid_file(pid_dir, name)
        pid = read_pid(path)
        stopped = False
        if pid is not None and checker(pid):
            killer(pid)
            stopped = True
            path.unlink(missing_ok=True)
        processes.append({"name": name, "pid": pid, "stopped": stopped})
    return {"mode": "stopped", "pid_dir": str(pid_dir), "processes": processes}
