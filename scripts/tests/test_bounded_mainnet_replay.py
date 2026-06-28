import importlib.util
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).resolve().parents[1] / "prepare-bounded-mainnet-replay.py"


def load_module():
    spec = importlib.util.spec_from_file_location("prepare_bounded_mainnet_replay", MODULE_PATH)
    if spec is None or spec.loader is None:
        raise ImportError(f"unable to load module from {MODULE_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def write_checkpoint(root: Path, *, mpt: bool) -> None:
    data = root / "data"
    data.mkdir(parents=True)
    (data / "CURRENT").write_text("MANIFEST-000001\n", encoding="utf-8")
    if mpt:
        state = root / "mpt"
        state.mkdir()
        (state / "CURRENT").write_text("MANIFEST-000001\n", encoding="utf-8")
        mpt_dir = "mpt"
    else:
        mpt_dir = "missing-mpt"
    (root / "CHECKPOINT_INFO").write_text(
        f"label=v509k\nsaved_at=2026-06-27T04:49:44Z\ndata_dir=data/mainnet-replay\nmpt_dir={mpt_dir}\n",
        encoding="utf-8",
    )


class BoundedMainnetReplayTests(unittest.TestCase):
    def test_plan_marks_missing_state_checkpoint_as_storage_sample_only(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            checkpoint = Path(tmp) / "checkpoint"
            write_checkpoint(checkpoint, mpt=False)

            plan = module.build_replay_plan(
                checkpoint=checkpoint,
                work_root=Path(tmp) / "work",
                label="v509k-to-607262",
                start_height=511289,
                target_height=607262,
                addresses=["NVU2QwsVdttjfTHQK7RYD6iwwfXRkSezGU"],
            )

        self.assertEqual(plan["mode"], "storage-sample")
        self.assertFalse(plan["state_checkpoint"]["present"])
        validator = module.step_by_name(plan, "state-root-validator")
        self.assertFalse(validator["enabled"])
        self.assertIn("StateRoot/MPT", validator["reason"])
        self.assertIn("--address", module.step_by_name(plan, "offline-gas-compare")["command"])

    def test_plan_enables_state_root_validator_when_state_checkpoint_exists(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            checkpoint = Path(tmp) / "checkpoint"
            write_checkpoint(checkpoint, mpt=True)

            plan = module.build_replay_plan(
                checkpoint=checkpoint,
                work_root=Path(tmp) / "work",
                label="v509k-to-607262",
                start_height=511289,
                target_height=607262,
                addresses=["NVU2QwsVdttjfTHQK7RYD6iwwfXRkSezGU"],
            )

        self.assertEqual(plan["mode"], "full-state")
        self.assertTrue(plan["state_checkpoint"]["present"])
        validator = module.step_by_name(plan, "state-root-validator")
        self.assertTrue(validator["enabled"])
        self.assertIn("--stop-at", validator["command"])
        self.assertIn("--start", validator["command"])
        self.assertIn("511289", validator["command"])
        self.assertIn("607262", validator["command"])

    def test_full_state_validator_requires_known_start_height(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            checkpoint = Path(tmp) / "checkpoint"
            write_checkpoint(checkpoint, mpt=True)

            plan = module.build_replay_plan(
                checkpoint=checkpoint,
                work_root=Path(tmp) / "work",
                label="v509k-to-607262",
                target_height=607262,
                addresses=["NVU2QwsVdttjfTHQK7RYD6iwwfXRkSezGU"],
            )

        validator = module.step_by_name(plan, "state-root-validator")
        self.assertFalse(validator["enabled"])
        self.assertIn("--start-height", validator["reason"])

    def test_prepare_copies_checkpoint_and_writes_isolated_config(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            checkpoint = Path(tmp) / "checkpoint"
            write_checkpoint(checkpoint, mpt=True)
            work_root = Path(tmp) / "work"

            plan = module.build_replay_plan(
                checkpoint=checkpoint,
                work_root=work_root,
                label="prepared",
                target_height=511300,
                addresses=["NUDcRfftT99w4m2puzTxQToHxZPjQ9NN9n"],
                rpc_port=31332,
                p2p_port=31333,
                metrics_port=31990,
            )
            prepared = module.prepare_workspace(plan)

            config_text = Path(prepared["config_path"]).read_text(encoding="utf-8")
            self.assertTrue((work_root / "prepared" / "data" / "CURRENT").exists())
            self.assertTrue((work_root / "prepared" / "mpt" / "CURRENT").exists())
            self.assertIn('path = "', config_text)
            self.assertIn("port = 31332", config_text)
            self.assertIn("port = 31333", config_text)
            self.assertIn("port = 31990", config_text)
            self.assertIn("enabled = true", config_text)

    def test_plan_includes_replay_with_repairs_command(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            checkpoint = Path(tmp) / "checkpoint"
            write_checkpoint(checkpoint, mpt=True)

            plan = module.build_replay_plan(
                checkpoint=checkpoint,
                work_root=Path(tmp) / "work",
                label="v509k-to-663386",
                start_height=511289,
                target_height=663386,
                addresses=[],
                node_bin=Path("target/release/neo-node"),
                probe_bin=Path("target/release/neo-db-probe"),
                reference_rpc="http://seed1.neo.org:10332",
                rpc_port=31332,
            )

        command = module.step_by_name(plan, "run-node-with-repairs")["command"]
        self.assertEqual(command[0:2], ["python3", "scripts/run-bounded-replay-with-repairs.py"])
        self.assertIn("--config", command)
        self.assertIn(str(Path(plan["work"]["config_path"])), command)
        self.assertIn("--db", command)
        self.assertIn(str(Path(plan["work"]["chain_db"])), command)
        self.assertIn("--log", command)
        self.assertIn(str(Path(plan["work"]["log_file"])), command)
        self.assertIn("--target-height", command)
        self.assertIn("663386", command)
        self.assertIn("--probe-bin", command)
        self.assertIn("target/release/neo-db-probe", command)
        self.assertIn("--rpc", command)
        self.assertIn("http://127.0.0.1:31332", command)


if __name__ == "__main__":
    unittest.main()
