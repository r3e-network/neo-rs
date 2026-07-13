import importlib.util
import sys
import tempfile
import unittest
from types import SimpleNamespace
from pathlib import Path


MODULE_PATH = Path(__file__).resolve().parents[1] / "run-bounded-replay-with-repairs.py"


def load_module():
    spec = importlib.util.spec_from_file_location("run_bounded_replay_with_repairs", MODULE_PATH)
    if spec is None or spec.loader is None:
        raise ImportError(f"unable to load module from {MODULE_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class RunBoundedReplayWithRepairsTests(unittest.TestCase):
    def test_timeout_continues_and_new_failure_repairs_from_attempt_offset(self):
        module = load_module()
        reports = iter(
            [
                {"status": "timeout", "last_height": 596897},
                {"status": "target-reached", "last_height": 663386},
            ]
        )
        offsets = iter([100, 200])
        runner_calls = []
        repair_calls = []

        def fake_runner():
            runner_calls.append("run")
            return next(reports)

        def fake_repair(offset):
            repair_calls.append(offset)
            return {"applied": True, "failure": {"height": 596898}}

        summary = module.run_replay_with_repairs(
            target_height=663386,
            max_attempts=5,
            log_size_reader=lambda: next(offsets),
            replay_runner=fake_runner,
            repairer=fake_repair,
        )

        self.assertEqual(summary["status"], "target-reached")
        self.assertEqual(len(runner_calls), 2)
        self.assertEqual(repair_calls, [100])
        self.assertEqual(summary["attempts"][0]["action"], "repair-and-retry")
        self.assertEqual(summary["attempts"][0]["repair"]["failure"]["height"], 596898)

    def test_timeout_without_new_repairable_failure_continues(self):
        module = load_module()
        reports = iter(
            [
                {"status": "timeout", "last_height": 596897},
                {"status": "target-reached", "last_height": 663386},
            ]
        )
        repair_calls = []

        def fake_repair(offset):
            repair_calls.append(offset)
            raise ValueError("no GasToken::burn balance failure found in log")

        summary = module.run_replay_with_repairs(
            target_height=663386,
            max_attempts=3,
            log_size_reader=lambda: 100,
            replay_runner=lambda: next(reports),
            repairer=fake_repair,
        )

        self.assertEqual(summary["status"], "target-reached")
        self.assertEqual(repair_calls, [100])
        self.assertEqual(summary["attempts"][0]["action"], "retry-after-timeout")
        self.assertIn("no GasToken", summary["attempts"][0]["repair_error"])

    def test_process_exit_without_new_repairable_failure_stops(self):
        module = load_module()

        def fake_repair(_offset):
            raise ValueError("no GasToken::burn balance failure found in log")

        summary = module.run_replay_with_repairs(
            target_height=663386,
            max_attempts=3,
            log_size_reader=lambda: 0,
            replay_runner=lambda: {"status": "process-exited", "last_height": 600000},
            repairer=fake_repair,
        )

        self.assertEqual(summary["status"], "process-exited")
        self.assertEqual(summary["attempts"][0]["action"], "stop-unrepaired-exit")
        self.assertIn("no GasToken", summary["attempts"][0]["repair_error"])

    def test_cli_passes_db_height_reader_to_bounded_runner(self):
        module = load_module()
        captured = {}

        def fake_load_script_module(_filename, module_name):
            if module_name == "run_bounded_mainnet_replay":
                def fake_run_until_target(**kwargs):
                    captured["height_reader"] = kwargs.get("height_reader")
                    captured["node_output"] = kwargs.get("node_output")
                    self.assertIsNotNone(captured["height_reader"])
                    height = captured["height_reader"]()
                    return {"status": "target-reached", "last_height": height}

                def fake_read_probe_ledger_height(db_path, probe_bin):
                    captured["probe_args"] = (db_path, probe_bin)
                    return 677297

                return SimpleNamespace(
                    node_command=lambda node_bin, config, target: [
                        str(node_bin),
                        str(config),
                        str(target),
                    ],
                    run_until_target=fake_run_until_target,
                    read_probe_ledger_height=fake_read_probe_ledger_height,
                )
            return SimpleNamespace(
                read_log_text=lambda _path, _offset: "",
                parse_gas_burn_failures=lambda _text: [],
                repair_bounded_replay_gas=lambda **_kwargs: {},
            )

        with tempfile.TemporaryDirectory() as tmp:
            log = Path(tmp) / "neo-node.log"
            node_output_log = Path(tmp) / "node-output.log"
            log.write_text("", encoding="utf-8")
            old_argv = sys.argv
            old_loader = module.load_script_module
            try:
                module.load_script_module = fake_load_script_module
                sys.argv = [
                    "run-bounded-replay-with-repairs.py",
                    "--config",
                    "bounded.toml",
                    "--db",
                    "bounded/data",
                    "--log",
                    str(log),
                    "--target-height",
                    "700000",
                    "--probe-bin",
                    "target/release/neo-db-probe",
                    "--node-output-log",
                    str(node_output_log),
                ]

                code = module.main()
            finally:
                sys.argv = old_argv
                module.load_script_module = old_loader

        self.assertEqual(code, 0)
        self.assertEqual(
            captured["probe_args"],
            (Path("bounded/data"), Path("target/release/neo-db-probe")),
        )
        self.assertIsNotNone(captured["node_output"])

    def test_cli_uses_mdbx_probe_and_repair_without_backend_selector(self):
        module = load_module()
        captured = {}

        def fake_load_script_module(_filename, module_name):
            if module_name == "run_bounded_mainnet_replay":
                def fake_run_until_target(**kwargs):
                    captured["height_reader"] = kwargs["height_reader"]
                    captured["height_reader"]()
                    return {"status": "process-exited", "last_height": 151116}

                def fake_read_probe_ledger_height(db_path, probe_bin):
                    captured["height_reader_args"] = (db_path, probe_bin)
                    return 151116

                return SimpleNamespace(
                    node_command=lambda node_bin, config, target: [
                        str(node_bin),
                        str(config),
                        str(target),
                    ],
                    run_until_target=fake_run_until_target,
                    read_probe_ledger_height=fake_read_probe_ledger_height,
                )

            def fake_repair(**kwargs):
                captured["repair_kwargs"] = kwargs
                return {"applied": True}

            return SimpleNamespace(
                read_log_text=lambda _path, _offset: (
                    "native GasToken TriggerType(ON_PERSIST) hook failed at block 151117: "
                    "Invalid operation: GasToken::burn: insufficient balance 55342295 to burn 91584640\n"
                ),
                parse_gas_burn_failures=lambda _text: [{"height": 151117}],
                repair_bounded_replay_gas=fake_repair,
            )

        with tempfile.TemporaryDirectory() as tmp:
            log = Path(tmp) / "neo-node.log"
            log.write_text("", encoding="utf-8")
            old_argv = sys.argv
            old_loader = module.load_script_module
            try:
                module.load_script_module = fake_load_script_module
                sys.argv = [
                    "run-bounded-replay-with-repairs.py",
                    "--config",
                    "bounded.toml",
                    "--db",
                    "bounded/data",
                    "--log",
                    str(log),
                    "--target-height",
                    "151120",
                    "--probe-bin",
                    "target/release/neo-db-probe",
                    "--max-attempts",
                    "1",
                ]

                code = module.main()
            finally:
                sys.argv = old_argv
                module.load_script_module = old_loader

        self.assertEqual(code, 124)
        self.assertEqual(
            captured["height_reader_args"],
            (Path("bounded/data"), Path("target/release/neo-db-probe")),
        )
        self.assertNotIn("storage_provider", captured["repair_kwargs"])


if __name__ == "__main__":
    unittest.main()
