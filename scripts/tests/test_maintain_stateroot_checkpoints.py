import importlib.util
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).resolve().parents[1] / "maintain-stateroot-checkpoints.py"
REPO_ROOT = Path(__file__).resolve().parents[2]


def load_module():
    spec = importlib.util.spec_from_file_location(
        "maintain_stateroot_checkpoints",
        MODULE_PATH,
    )
    if spec is None or spec.loader is None:
        raise ImportError(f"unable to load module from {MODULE_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class MaintainStateRootCheckpointsTests(unittest.TestCase):
    def test_plan_skips_existing_checkpoint_and_creates_missing_stages(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "checkpoints"
            existing = root / "h0"
            (existing / "mainnet").mkdir(parents=True)
            (existing / "StateRoot").mkdir()
            (existing / "CHECKPOINT_INFO").write_text("height=0\n", encoding="utf-8")

            plan = module.build_checkpoint_plan(
                {
                    "checkpoint_stages": [
                        {"stage": "base", "height": 0, "label": "base-h0"},
                        {"stage": "mid", "height": 60_000, "label": "mid-h60000"},
                        {
                            "stage": "latest",
                            "height": 120_000,
                            "label": "latest-h120000",
                        },
                    ]
                },
                checkpoint_root=root,
                data_dir=Path(tmp) / "data",
                writer_pid="1234",
                script_path=Path("scripts/checkpoint-on-height.sh"),
                chain_db=None,
                stateroot_db=None,
            )

            self.assertEqual(plan["status"], "ready")
            self.assertEqual(
                [(item["stage"], item["height"], item["action"]) for item in plan["actions"]],
                [
                    ("base", 0, "skip"),
                    ("mid", 60_000, "create"),
                    ("latest", 120_000, "create"),
                ],
            )
            latest = plan["actions"][2]
            self.assertIn("--once", latest["command"])
            self.assertIn("--height", latest["command"])
            self.assertIn("120000", latest["command"])
            self.assertIn("--data-dir", latest["command"])
            self.assertIn("--root", latest["command"])

    def test_plan_waits_until_status_contains_validated_checkpoint_stages(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            chain_db = Path(tmp) / "data" / "mainnet-validate"
            stateroot_db = Path(tmp) / "Data_MPT_validate_334F454E"
            plan = module.build_checkpoint_plan(
                {"checkpoint_stages": []},
                checkpoint_root=Path(tmp) / "checkpoints",
                data_dir=Path(tmp) / "data",
                writer_pid="none",
                script_path=Path("scripts/checkpoint-on-height.sh"),
                chain_db=chain_db,
                stateroot_db=stateroot_db,
            )

            self.assertEqual(plan["status"], "waiting")
            self.assertEqual(plan["actions"], [])
            self.assertEqual(plan["chain_db"], str(chain_db))
            self.assertEqual(plan["stateroot_db"], str(stateroot_db))

    def test_missing_status_file_loads_waiting_plan_input(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            status = module.load_status(Path(tmp) / "missing-status.json")

            self.assertEqual(status["checkpoint_stages"], [])
            self.assertIn("missing", status["status"])

    def test_plan_blocks_incomplete_checkpoint_directory(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "checkpoints"
            incomplete = root / "h60000"
            incomplete.mkdir(parents=True)
            (incomplete / "CHECKPOINT_IN_PROGRESS").write_text(
                "height=60000\n",
                encoding="utf-8",
            )

            plan = module.build_checkpoint_plan(
                {
                    "checkpoint_stages": [
                        {"stage": "mid", "height": 60_000, "label": "mid-h60000"}
                    ]
                },
                checkpoint_root=root,
                data_dir=Path(tmp) / "data",
                writer_pid="none",
                script_path=Path("scripts/checkpoint-on-height.sh"),
                chain_db=None,
                stateroot_db=None,
            )

            self.assertEqual(plan["status"], "blocked")
            self.assertEqual(plan["actions"][0]["action"], "blocked")
            self.assertIn("incomplete", plan["actions"][0]["reason"])

    def test_plan_creates_duplicate_checkpoint_height_only_once(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            plan = module.build_checkpoint_plan(
                {
                    "checkpoint_stages": [
                        {"stage": "base", "height": 0, "label": "base-h0"},
                        {"stage": "mid", "height": 0, "label": "mid-h0"},
                        {"stage": "latest", "height": 0, "label": "latest-h0"},
                    ]
                },
                checkpoint_root=Path(tmp) / "checkpoints",
                data_dir=Path(tmp) / "data",
                writer_pid="none",
                script_path=Path("scripts/checkpoint-on-height.sh"),
                chain_db=None,
                stateroot_db=None,
            )

            self.assertEqual(
                [(item["stage"], item["action"]) for item in plan["actions"]],
                [
                    ("base", "create"),
                    ("mid", "skip"),
                    ("latest", "skip"),
                ],
            )
            self.assertEqual(plan["actions"][1]["reason"], "checkpoint height already planned")

    def test_plan_defers_historical_stage_when_live_chain_is_already_ahead(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            plan = module.build_checkpoint_plan(
                {
                    "checkpoint_stages": [
                        {"stage": "base", "height": 0, "label": "base-h0"},
                    ],
                    "last_validated_block": 0,
                    "local_block_count": 474_702,
                    "local_state_height": 0,
                    "local_validated_height": 0,
                },
                checkpoint_root=Path(tmp) / "checkpoints",
                data_dir=Path(tmp) / "data",
                writer_pid="1234",
                script_path=Path("scripts/checkpoint-on-height.sh"),
                chain_db=None,
                stateroot_db=None,
            )

            self.assertEqual(plan["status"], "waiting")
            self.assertEqual(plan["actions"][0]["action"], "defer")
            self.assertIn("current durable chain height", plan["actions"][0]["reason"])

    def test_plan_passes_explicit_validation_data_dirs_to_checkpoint_script(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            chain_db = root / "data" / "mainnet-validate"
            stateroot_db = root / "Data_MPT_validate_334F454E"
            plan = module.build_checkpoint_plan(
                {
                    "checkpoint_stages": [
                        {"stage": "latest", "height": 50_900, "label": "latest-h50900"}
                    ]
                },
                checkpoint_root=root / "checkpoints",
                data_dir=root / "data",
                writer_pid="none",
                script_path=Path("scripts/checkpoint-on-height.sh"),
                chain_db=chain_db,
                stateroot_db=stateroot_db,
            )

            command = plan["actions"][0]["command"]
            self.assertIn("--chain-db", command)
            self.assertIn(str(chain_db), command)
            self.assertIn("--stateroot-db", command)
            self.assertIn(str(stateroot_db), command)

    def test_checkpoint_paths_can_be_derived_from_node_config(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            config = Path(tmp) / "neo_mainnet_validate.toml"
            config.write_text(
                """
[network]
network_magic = 0x334F454E

[storage]
path = "./data/mainnet-validate"

[state_service]
enabled = true
path = "Data_MPT_validate_{0}"
""".strip()
                + "\n",
                encoding="utf-8",
            )

            paths = module.derive_checkpoint_paths_from_config(config)

            self.assertEqual(paths["data_dir"], Path("./data"))
            self.assertEqual(paths["chain_db"], Path("./data/mainnet-validate"))
            self.assertEqual(
                paths["stateroot_db"],
                Path("Data_MPT_validate_334F454E"),
            )

    def test_execute_plan_runs_only_create_actions(self):
        module = load_module()
        commands = []
        plan = {
            "actions": [
                {"action": "skip", "command": ["skip-me"]},
                {"action": "blocked", "command": ["blocked"]},
                {"action": "create", "command": ["create-mid"]},
                {"action": "create", "command": ["create-latest"]},
            ]
        }

        result = module.execute_plan(plan, execute=True, runner=commands.append)

        self.assertEqual(commands, [["create-mid"], ["create-latest"]])
        self.assertEqual(result["executed"], 2)
        self.assertEqual(result["skipped"], 1)

    def test_dry_run_plan_does_not_run_create_actions(self):
        module = load_module()
        commands = []
        plan = {"actions": [{"action": "create", "command": ["create-mid"]}]}

        result = module.execute_plan(plan, execute=False, runner=commands.append)

        self.assertEqual(commands, [])
        self.assertEqual(result["executed"], 0)
        self.assertEqual(result["skipped"], 0)

    def test_watch_loop_reloads_status_and_maintains_each_iteration(self):
        module = load_module()
        statuses = [
            {"checkpoint_stages": [{"stage": "base", "height": 10}]},
            {"checkpoint_stages": [{"stage": "latest", "height": 20}]},
        ]
        commands = []
        sleeps = []

        result = module.run_watch_loop(
            status_loader=lambda: statuses.pop(0),
            plan_builder=lambda status: module.build_checkpoint_plan(
                status,
                checkpoint_root=Path("/tmp/checkpoints"),
                data_dir=Path("/tmp/data"),
                writer_pid="1234",
                script_path=Path("scripts/checkpoint-on-height.sh"),
                chain_db=Path("/tmp/data/mainnet-validate"),
                stateroot_db=Path("/tmp/Data_MPT_validate_334F454E"),
            ),
            execute=True,
            runner=commands.append,
            interval_seconds=30,
            max_iterations=2,
            sleep_fn=sleeps.append,
        )

        self.assertEqual(result["mode"], "watch")
        self.assertEqual(result["iterations"], 2)
        self.assertEqual([command[4] for command in commands], ["10", "20"])
        self.assertEqual(sleeps, [30])

    def test_watch_loop_uses_short_retry_interval_while_status_is_waiting(self):
        module = load_module()
        statuses = [
            {"checkpoint_stages": []},
            {"checkpoint_stages": [{"stage": "base", "height": 10}]},
        ]
        sleeps = []

        result = module.run_watch_loop(
            status_loader=lambda: statuses.pop(0),
            plan_builder=lambda status: module.build_checkpoint_plan(
                status,
                checkpoint_root=Path("/tmp/checkpoints"),
                data_dir=Path("/tmp/data"),
                writer_pid="none",
                script_path=Path("scripts/checkpoint-on-height.sh"),
                chain_db=None,
                stateroot_db=None,
            ),
            execute=False,
            runner=None,
            interval_seconds=600,
            waiting_interval_seconds=5,
            max_iterations=2,
            sleep_fn=sleeps.append,
        )

        self.assertEqual(result["iterations"], 2)
        self.assertEqual(sleeps, [5])

    def test_operations_doc_describes_checkpoint_maintenance_entrypoint(self):
        text = (REPO_ROOT / "docs" / "operations.md").read_text(encoding="utf-8")

        self.assertIn("scripts/maintain-stateroot-checkpoints.py", text)
        self.assertIn("--node-config", text)
        self.assertIn("--watch-interval", text)
        self.assertIn("--execute", text)
        self.assertIn("dry-run", text)


if __name__ == "__main__":
    unittest.main()
