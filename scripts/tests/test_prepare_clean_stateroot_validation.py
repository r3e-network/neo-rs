import importlib.util
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).resolve().parents[1] / "prepare-clean-stateroot-validation.py"


def load_module():
    spec = importlib.util.spec_from_file_location(
        "prepare_clean_stateroot_validation",
        MODULE_PATH,
    )
    if spec is None or spec.loader is None:
        raise ImportError(f"unable to load module from {MODULE_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


BASE_CONFIG = """
[network]
network_type = "MainNet"
network_magic = 0x334F454E

[storage]
backend = "rocksdb"
path = "./data/mainnet-validate"
read_only = false

[p2p]
port = 20333

[rpc]
enabled = true
port = 20332
bind_address = "127.0.0.1"
auth_enabled = false

[indexer]
enabled = false
backfill_on_startup = false
store_path = "./data/mainnet-validate/indexer"

[application_logs]
enabled = false
path = "./data/mainnet-validate/application-logs"

[tokens_tracker]
enabled = false
db_path = "./data/mainnet-validate/tokens"

[telemetry]
[telemetry.metrics]
enabled = false
port = 29090
bind_address = "127.0.0.1"

[logging]
file_path = "./logs/neo-node-validate.log"

[state_service]
enabled = true
path = "Data_MPT_validate_{0}"
full_state = true
track_during_catchup = true
"""


class PrepareCleanStateRootValidationTests(unittest.TestCase):
    def test_prepare_workspace_writes_isolated_config_and_commands(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            base_config = root / "neo_mainnet_validate.toml"
            base_config.write_text(BASE_CONFIG, encoding="utf-8")
            work_root = root / "clean"

            plan = module.prepare_workspace(
                base_config=base_config,
                work_root=work_root,
                rpc_port=31332,
                p2p_port=31333,
                metrics_port=31990,
                smoke_target_height=12,
                node_bin=Path("target/debug/neo-node"),
                probe_bin=Path("target/debug/neo-db-probe"),
                dry_run=False,
                force=False,
            )

            config = (work_root / "neo_mainnet_validate.toml").read_text(encoding="utf-8")
            self.assertIn(f'path = "{(work_root / "chain").resolve()}"', config)
            self.assertIn(f'data_dir = "{(work_root / "chain").resolve()}"', config)
            self.assertIn(f'path = "{(work_root / "state-root-{0}").resolve()}"', config)
            self.assertIn("port = 31332", config)
            self.assertIn("port = 31333", config)
            self.assertIn("port = 31990", config)
            self.assertIn("track_during_catchup = true", config)
            self.assertEqual(plan["commands"]["preflight"][0], "target/debug/neo-node")
            self.assertIn(str(work_root / "neo_mainnet_validate.toml"), plan["commands"]["preflight"])
            smoke_command = plan["commands"]["bounded-smoke"]
            self.assertIn("scripts/run-bounded-mainnet-replay.py", smoke_command)
            self.assertIn("--target-height", smoke_command)
            self.assertIn("12", smoke_command)
            self.assertIn("--stateroot-db", smoke_command)
            self.assertIn(str(work_root / "state-root-334F454E"), smoke_command)
            self.assertIn("--require-stateroot-height-match", smoke_command)
            self.assertIn("--reference", smoke_command)
            reference_arg = smoke_command[smoke_command.index("--reference") + 1]
            self.assertIn("http://seed1.neo.org:10332", reference_arg)
            self.assertIn("--require-reference-stateroot-match", smoke_command)
            checkpoint_command = plan["commands"]["checkpoint-smoke"]
            self.assertEqual(checkpoint_command[0], "scripts/checkpoint-on-height.sh")
            self.assertIn("--height", checkpoint_command)
            self.assertIn("12", checkpoint_command)
            self.assertIn(str(work_root / "chain"), checkpoint_command)
            self.assertIn(str(work_root / "state-root-334F454E"), checkpoint_command)
            self.assertIn(str(work_root / "checkpoints"), checkpoint_command)
            milestone_command = plan["commands"]["milestone-smoke"]
            self.assertIn("scripts/run-stateroot-milestones.py", milestone_command)
            self.assertIn("--milestone", milestone_command)
            self.assertIn("12,24,36", milestone_command)
            self.assertIn("--checkpoint-root", milestone_command)
            self.assertIn(str(work_root / "checkpoints"), milestone_command)
            self.assertIn("--reference", milestone_command)
            self.assertIn("--summary-jsonl", milestone_command)
            self.assertIn(str(work_root / "milestone-summary.jsonl"), milestone_command)
            self.assertIn("--checkpoint-execute", plan["commands"]["start-stack"])

    def test_prepare_workspace_refuses_existing_work_root_without_force(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            base_config = root / "neo_mainnet_validate.toml"
            base_config.write_text(BASE_CONFIG, encoding="utf-8")
            work_root = root / "clean"
            work_root.mkdir()

            with self.assertRaises(FileExistsError):
                module.prepare_workspace(
                    base_config=base_config,
                    work_root=work_root,
                    rpc_port=31332,
                    p2p_port=31333,
                    metrics_port=31990,
                    smoke_target_height=10,
                    node_bin=Path("target/debug/neo-node"),
                    probe_bin=Path("target/debug/neo-db-probe"),
                    dry_run=False,
                    force=False,
                )

    def test_dry_run_does_not_create_work_root(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            base_config = root / "neo_mainnet_validate.toml"
            base_config.write_text(BASE_CONFIG, encoding="utf-8")
            work_root = root / "clean"

            plan = module.prepare_workspace(
                base_config=base_config,
                work_root=work_root,
                rpc_port=31332,
                p2p_port=31333,
                metrics_port=31990,
                smoke_target_height=10,
                node_bin=Path("target/debug/neo-node"),
                probe_bin=Path("target/debug/neo-db-probe"),
                dry_run=True,
                force=False,
            )

            self.assertTrue(plan["dry_run"])
            self.assertFalse(work_root.exists())


if __name__ == "__main__":
    unittest.main()
