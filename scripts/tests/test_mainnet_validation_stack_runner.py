import importlib.util
import subprocess
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).resolve().parents[1] / "mainnet_validation_stack.py"


def load_module():
    spec = importlib.util.spec_from_file_location("mainnet_validation_stack", MODULE_PATH)
    if spec is None or spec.loader is None:
        raise ImportError(f"unable to load module from {MODULE_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class FakeProcess:
    def __init__(self, pid):
        self.pid = pid


class MainnetValidationStackRunnerTests(unittest.TestCase):
    def test_start_stack_runs_preflight_and_starts_processes_with_pid_files(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            plan = module.build_plan(
                node_config=Path("neo_mainnet_validate.toml"),
                node_bin=Path("target/release/neo-node"),
                status_file=Path("/tmp/stateroot-validation.json"),
                resume_file=Path("/tmp/stateroot-last-validated"),
                log_dir=root / "logs",
                batch=250,
                poll_interval=3,
                checkpoint_execute=True,
            )
            calls = []
            pids = iter([101, 102, 103])

            def run_preflight(command, *, check, **kwargs):
                calls.append(("preflight", command, check, kwargs))

            def spawn(command, **kwargs):
                calls.append(("spawn", command, kwargs["stdout"].name))
                return FakeProcess(next(pids))

            result = module.start_stack(
                plan,
                pid_dir=root / "pids",
                preflight_runner=run_preflight,
                spawner=spawn,
            )

            self.assertEqual(
                calls[0],
                (
                    "preflight",
                    plan["steps"][0]["command"],
                    True,
                    {"capture_output": True, "text": True},
                ),
            )
            self.assertTrue(result["preflight"]["ok"])
            self.assertEqual([item["name"] for item in result["processes"]], module.RUNTIME_STEP_NAMES)
            self.assertEqual([item["pid"] for item in result["processes"]], [101, 102, 103])
            self.assertEqual((root / "pids" / "neo-node.pid").read_text(encoding="utf-8"), "101\n")
            self.assertEqual(
                (root / "pids" / "state-root-validator.pid").read_text(encoding="utf-8"),
                "102\n",
            )
            checkpoint_command = calls[3][1]
            writer_pid_index = checkpoint_command.index("--writer-pid") + 1
            self.assertEqual(checkpoint_command[writer_pid_index], "101")
            self.assertIn("--execute", checkpoint_command)

    def test_start_stack_returns_preflight_failure_without_spawning_processes(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            plan = module.build_plan(
                node_config=Path("neo_mainnet_validate.toml"),
                node_bin=Path("target/release/neo-node"),
                status_file=Path("/tmp/stateroot-validation.json"),
                resume_file=Path("/tmp/stateroot-last-validated"),
                log_dir=root / "logs",
                batch=250,
                poll_interval=3,
                checkpoint_execute=True,
            )

            def fail_preflight(command, *, check, **kwargs):
                raise subprocess.CalledProcessError(
                    1,
                    command,
                    output="",
                    stderr="StateService MPT height 0 does not match chain height 474701",
                )

            spawns = []
            result = module.start_stack(
                plan,
                pid_dir=root / "pids",
                preflight_runner=fail_preflight,
                spawner=spawns.append,
            )

            self.assertEqual(result["mode"], "preflight-failed")
            self.assertEqual(result["processes"], [])
            self.assertEqual(result["preflight"]["returncode"], 1)
            self.assertIn("StateService MPT height 0", result["preflight"]["stderr"])
            self.assertIn("restore a matching StateRoot checkpoint", result["preflight"]["hint"])
            self.assertEqual(spawns, [])
            self.assertFalse((root / "pids" / "neo-node.pid").exists())

    def test_stack_status_reports_pid_file_health(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            pid_dir = Path(tmp)
            (pid_dir / "neo-node.pid").write_text("101\n", encoding="utf-8")
            (pid_dir / "state-root-validator.pid").write_text("102\n", encoding="utf-8")

            status = module.stack_status(
                pid_dir,
                checker=lambda pid: pid == 101,
            )

            by_name = {item["name"]: item for item in status["processes"]}
            self.assertTrue(by_name["node"]["running"])
            self.assertFalse(by_name["state-root-validator"]["running"])
            self.assertIsNone(by_name["checkpoint-maintainer"]["pid"])

    def test_stop_stack_terminates_running_pids_in_reverse_runtime_order(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            pid_dir = Path(tmp)
            for filename, pid in [
                ("neo-node.pid", 101),
                ("state-root-validator.pid", 102),
                ("checkpoint-maintainer.pid", 103),
            ]:
                (pid_dir / filename).write_text(f"{pid}\n", encoding="utf-8")
            killed = []

            result = module.stop_stack(
                pid_dir,
                checker=lambda pid: pid != 102,
                killer=killed.append,
            )

            self.assertEqual(killed, [103, 101])
            self.assertEqual(
                [(item["name"], item["pid"], item["stopped"]) for item in result["processes"]],
                [
                    ("checkpoint-maintainer", 103, True),
                    ("state-root-validator", 102, False),
                    ("node", 101, True),
                ],
            )
            self.assertFalse((pid_dir / "checkpoint-maintainer.pid").exists())
            self.assertTrue((pid_dir / "state-root-validator.pid").exists())
            self.assertFalse((pid_dir / "neo-node.pid").exists())


if __name__ == "__main__":
    unittest.main()
