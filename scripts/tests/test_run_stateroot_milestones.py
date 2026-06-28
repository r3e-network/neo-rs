import importlib.util
import json
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace


MODULE_PATH = Path(__file__).resolve().parents[1] / "run-stateroot-milestones.py"


def load_module():
    spec = importlib.util.spec_from_file_location("run_stateroot_milestones", MODULE_PATH)
    if spec is None or spec.loader is None:
        raise ImportError(f"unable to load module from {MODULE_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class RunStateRootMilestonesTests(unittest.TestCase):
    def test_parse_height_values_requires_unique_increasing_heights(self):
        module = load_module()

        self.assertEqual(module.parse_height_values(["10,20", "30"]), [10, 20, 30])

        with self.assertRaises(ValueError):
            module.parse_height_values(["20,10"])
        with self.assertRaises(ValueError):
            module.parse_height_values(["10,10"])
        with self.assertRaises(ValueError):
            module.parse_height_values([])

    def test_parse_last_json_object_ignores_progress_samples(self):
        module = load_module()
        output = (
            '{"elapsed_seconds": 0.0, "height": 10}\n'
            '{\n'
            '  "status": "target-reached",\n'
            '  "post_probe": {"stateroot_matches_chain": true}\n'
            '}\n'
        )

        parsed = module.parse_last_json_object(output)

        self.assertEqual(parsed["status"], "target-reached")
        self.assertTrue(parsed["post_probe"]["stateroot_matches_chain"])

    def test_build_plan_includes_reference_checked_bounded_and_checkpoint_commands(self):
        module = load_module()

        plan = module.build_plan(
            config=Path("clean/neo_mainnet_validate.toml"),
            node_bin=Path("target/debug/neo-node"),
            rpc_url="http://127.0.0.1:21332",
            milestones=[10, 20],
            poll_interval=5.0,
            max_seconds=120.0,
            chain_db=Path("clean/chain"),
            stateroot_db=Path("clean/state-root-334F454E"),
            probe_bin=Path("target/debug/neo-db-probe"),
            references=["http://seed1.neo.org:10332"],
            data_dir=Path("clean"),
            checkpoint_root=Path("clean/checkpoints"),
            checkpoint_script=Path("scripts/checkpoint-on-height.sh"),
            log_dir=Path("clean/logs"),
        )

        self.assertEqual(plan["mode"], "dry-run")
        self.assertEqual(plan["node_bin"], "target/debug/neo-node")
        self.assertEqual(plan["probe_bin"], "target/debug/neo-db-probe")
        self.assertIsNone(plan["metrics_url"])
        self.assertEqual(plan["milestones"], [10, 20])
        bounded = plan["steps"][0]["bounded_command"]
        self.assertIn("scripts/run-bounded-mainnet-replay.py", bounded)
        self.assertIn("--require-stateroot-height-match", bounded)
        self.assertIn("--require-reference-stateroot-match", bounded)
        self.assertIn("clean/logs/neo-node-milestone-h10.log", bounded)
        checkpoint = plan["steps"][1]["checkpoint_command"]
        self.assertEqual(checkpoint[0], "scripts/checkpoint-on-height.sh")
        self.assertIn("--height", checkpoint)
        self.assertIn("20", checkpoint)

    def test_build_plan_passes_metrics_url_to_bounded_command(self):
        module = load_module()

        plan = module.build_plan(
            config=Path("clean/neo_mainnet_validate.toml"),
            node_bin=Path("target/debug/neo-node"),
            rpc_url="http://127.0.0.1:21332",
            milestones=[10],
            poll_interval=5.0,
            max_seconds=120.0,
            chain_db=Path("clean/chain"),
            stateroot_db=Path("clean/state-root-334F454E"),
            probe_bin=Path("target/debug/neo-db-probe"),
            references=["http://seed1.neo.org:10332"],
            data_dir=Path("clean"),
            checkpoint_root=Path("clean/checkpoints"),
            checkpoint_script=Path("scripts/checkpoint-on-height.sh"),
            log_dir=Path("clean/logs"),
            metrics_url="http://127.0.0.1:21990/metrics",
        )

        bounded = plan["steps"][0]["bounded_command"]
        self.assertEqual(plan["metrics_url"], "http://127.0.0.1:21990/metrics")
        self.assertIn("--metrics-url", bounded)
        self.assertIn("http://127.0.0.1:21990/metrics", bounded)

    def test_parse_args_keeps_user_reference_without_default_duplicates(self):
        module = load_module()
        original_argv = module.sys.argv
        try:
            module.sys.argv = [
                "run-stateroot-milestones.py",
                "--config",
                "clean/neo_mainnet_validate.toml",
                "--milestone",
                "10",
                "--chain-db",
                "clean/chain",
                "--stateroot-db",
                "clean/state-root-334F454E",
                "--checkpoint-root",
                "clean/checkpoints",
                "--log-dir",
                "clean/logs",
                "--reference",
                "http://seed1.neo.org:10332",
            ]
            args = module.parse_args()
        finally:
            module.sys.argv = original_argv

        self.assertEqual(args.reference, ["http://seed1.neo.org:10332"])

    def test_run_milestones_checkpoints_each_successful_height(self):
        module = load_module()
        plan = module.build_plan(
            config=Path("clean/neo_mainnet_validate.toml"),
            node_bin=Path("target/debug/neo-node"),
            rpc_url="http://127.0.0.1:21332",
            milestones=[10, 20],
            poll_interval=5.0,
            max_seconds=120.0,
            chain_db=Path("clean/chain"),
            stateroot_db=Path("clean/state-root-334F454E"),
            probe_bin=Path("target/debug/neo-db-probe"),
            references=["http://seed1.neo.org:10332"],
            data_dir=Path("clean"),
            checkpoint_root=Path("clean/checkpoints"),
            checkpoint_script=Path("scripts/checkpoint-on-height.sh"),
            log_dir=Path("clean/logs"),
        )
        calls = []

        def fake_run(command, **kwargs):
            calls.append(command)
            if "scripts/run-bounded-mainnet-replay.py" in command:
                height = command[command.index("--target-height") + 1]
                return SimpleNamespace(
                    returncode=0,
                    stdout=json.dumps(
                        {
                            "status": "target-reached",
                            "target_height": int(height),
                            "last_height": int(height),
                            "blocks_per_second": 2.0,
                            "elapsed_seconds": 1.5,
                            "height_samples": [
                                {
                                    "elapsed_seconds": 0.0,
                                    "height": 0,
                                    "metrics": {
                                        "neo_sync_avg_persist_us": 100.0,
                                        "neo_state_service_mpt_apply_avg_total_us": 10.0,
                                    },
                                },
                                {
                                    "elapsed_seconds": 2.0,
                                    "height": 4,
                                    "metrics": {
                                        "neo_sync_avg_persist_us": 200.0,
                                        "neo_state_service_mpt_apply_avg_total_us": 20.0,
                                    },
                                },
                                {
                                    "elapsed_seconds": 5.0,
                                    "height": 10,
                                    "metrics_error": "metrics unavailable",
                                },
                            ],
                            "post_probe": {
                                "stateroot_matches_chain": True,
                                "stateroot_root": {"root": f"0xroot{height}"},
                                "reference_stateroot": {
                                    "matches_local": True,
                                    "successful_samples": 5,
                                },
                            },
                        }
                    ),
                    stderr="",
                )
            return SimpleNamespace(returncode=0, stdout="checkpoint created", stderr="")

        result = module.run_milestones(plan, runner=fake_run)

        self.assertEqual(result["mode"], "completed")
        self.assertEqual(len(result["results"]), 2)
        self.assertEqual(result["summary"]["completed_heights"], [10, 20])
        self.assertEqual(result["summary"]["latest_height"], 20)
        self.assertEqual(result["summary"]["latest_root"], "0xroot20")
        self.assertEqual(result["summary"]["average_blocks_per_second"], 2.0)
        self.assertTrue(result["summary"]["all_reference_matched"])
        sample_summary = result["summary"]["milestones"][0]["height_sample_rate_summary"]
        self.assertEqual(sample_summary["sample_count"], 3)
        self.assertEqual(sample_summary["interval_count"], 2)
        self.assertEqual(sample_summary["average_blocks_per_second"], 2.0)
        self.assertEqual(sample_summary["slowest_interval"]["from_height"], 0)
        self.assertEqual(sample_summary["fastest_interval"]["to_height"], 4)
        metrics_summary = result["summary"]["milestones"][0]["metrics_sample_summary"]
        self.assertEqual(metrics_summary["sample_count"], 3)
        self.assertEqual(metrics_summary["metrics_error_count"], 1)
        self.assertEqual(
            metrics_summary["metrics"]["neo_sync_avg_persist_us"]["last"],
            200.0,
        )
        self.assertEqual(
            metrics_summary["metrics"]["neo_state_service_mpt_apply_avg_total_us"]["average"],
            15.0,
        )
        self.assertNotIn("bounded_stdout", result["results"][0])
        self.assertNotIn("checkpoint_stdout", result["results"][0])
        self.assertEqual(len(calls), 4)
        self.assertEqual(calls[1][0], "scripts/checkpoint-on-height.sh")
        self.assertEqual(calls[3][0], "scripts/checkpoint-on-height.sh")

    def test_height_sample_rate_summary_ignores_invalid_samples(self):
        module = load_module()

        summary = module.height_sample_rate_summary(
            {
                "height_samples": [
                    {"elapsed_seconds": 0.0, "height": 10},
                    {"elapsed_seconds": 5.0, "height": 20},
                    {"elapsed_seconds": 6.0, "height": 20},
                    {"elapsed_seconds": 5.0, "height": 25},
                    {"elapsed_seconds": "bad", "height": 30},
                    {"elapsed_seconds": 10.0, "height": 40},
                ]
            }
        )

        self.assertEqual(summary["sample_count"], 6)
        self.assertEqual(summary["interval_count"], 1)
        self.assertEqual(summary["min_blocks_per_second"], 2.0)
        self.assertEqual(summary["max_blocks_per_second"], 2.0)

    def test_run_milestones_can_include_raw_child_output_on_success(self):
        module = load_module()
        plan = module.build_plan(
            config=Path("clean/neo_mainnet_validate.toml"),
            node_bin=Path("target/debug/neo-node"),
            rpc_url="http://127.0.0.1:21332",
            milestones=[10],
            poll_interval=5.0,
            max_seconds=120.0,
            chain_db=Path("clean/chain"),
            stateroot_db=Path("clean/state-root-334F454E"),
            probe_bin=Path("target/debug/neo-db-probe"),
            references=["http://seed1.neo.org:10332"],
            data_dir=Path("clean"),
            checkpoint_root=Path("clean/checkpoints"),
            checkpoint_script=Path("scripts/checkpoint-on-height.sh"),
            log_dir=Path("clean/logs"),
        )

        def fake_run(command, **kwargs):
            if "scripts/run-bounded-mainnet-replay.py" in command:
                return SimpleNamespace(
                    returncode=0,
                    stdout=json.dumps({"status": "target-reached", "target_height": 10}),
                    stderr="",
                )
            return SimpleNamespace(returncode=0, stdout="checkpoint created", stderr="")

        result = module.run_milestones(
            plan,
            runner=fake_run,
            include_command_output=True,
        )

        self.assertEqual(result["mode"], "completed")
        self.assertIn("bounded_stdout", result["results"][0])
        self.assertIn("checkpoint_stdout", result["results"][0])

    def test_run_milestones_stops_before_checkpoint_on_bounded_failure(self):
        module = load_module()
        plan = module.build_plan(
            config=Path("clean/neo_mainnet_validate.toml"),
            node_bin=Path("target/debug/neo-node"),
            rpc_url="http://127.0.0.1:21332",
            milestones=[10, 20],
            poll_interval=5.0,
            max_seconds=120.0,
            chain_db=Path("clean/chain"),
            stateroot_db=Path("clean/state-root-334F454E"),
            probe_bin=Path("target/debug/neo-db-probe"),
            references=["http://seed1.neo.org:10332"],
            data_dir=Path("clean"),
            checkpoint_root=Path("clean/checkpoints"),
            checkpoint_script=Path("scripts/checkpoint-on-height.sh"),
            log_dir=Path("clean/logs"),
        )
        calls = []

        def fake_run(command, **kwargs):
            calls.append(command)
            return SimpleNamespace(
                returncode=1,
                stdout=json.dumps({"status": "reference-stateroot-mismatch"}),
                stderr="mismatch",
            )

        result = module.run_milestones(plan, runner=fake_run)

        self.assertEqual(result["mode"], "failed")
        self.assertEqual(result["failure"], "bounded-replay")
        self.assertEqual(result["failed_height"], 10)
        self.assertEqual(result["summary"]["mode"], "failed")
        self.assertEqual(result["summary"]["completed_heights"], [])
        self.assertIn("bounded_stdout", result["results"][0])
        self.assertEqual(result["results"][0]["bounded_stderr"], "mismatch")
        self.assertEqual(len(calls), 1)

    def test_append_summary_jsonl_writes_one_compact_history_record(self):
        module = load_module()
        plan = {
            "config": "clean/neo_mainnet_validate.toml",
            "node_bin": "target/release/neo-node",
            "probe_bin": "target/release/neo-db-probe",
            "chain_db": "clean/chain",
            "stateroot_db": "clean/state-root-334F454E",
            "checkpoint_root": "clean/checkpoints",
        }
        result = {
            "mode": "completed",
            "summary": {
                "latest_height": 100,
                "latest_root": "0xroot",
                "average_blocks_per_second": 9.5,
            },
        }

        with tempfile.TemporaryDirectory() as tmp:
            history = Path(tmp) / "nested" / "milestone-summary.jsonl"
            module.append_summary_jsonl(
                history,
                module.summary_history_record(plan, result),
            )

            line = history.read_text(encoding="utf-8").strip()

        record = json.loads(line)
        self.assertEqual(record["mode"], "completed")
        self.assertEqual(record["config"], "clean/neo_mainnet_validate.toml")
        self.assertEqual(record["node_bin"], "target/release/neo-node")
        self.assertEqual(record["probe_bin"], "target/release/neo-db-probe")
        self.assertEqual(record["summary"]["latest_height"], 100)
        self.assertEqual(record["summary"]["latest_root"], "0xroot")
        self.assertIn("timestamp_utc", record)


if __name__ == "__main__":
    unittest.main()
