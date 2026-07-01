import importlib.util
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).resolve().parents[1] / "prepare-bounded-mainnet-replay.py"
VERIFIED_ROOT = "0x" + "a" * 64


def load_module():
    spec = importlib.util.spec_from_file_location("prepare_bounded_mainnet_replay", MODULE_PATH)
    if spec is None or spec.loader is None:
        raise ImportError(f"unable to load module from {MODULE_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def write_checkpoint(
    root: Path,
    *,
    mpt: bool,
    include_info: bool = True,
    metadata: dict[str, str | None] | None = None,
    chain_dir: str = "data",
    state_dir: str = "mpt",
) -> None:
    data = root / chain_dir
    data.mkdir(parents=True)
    (data / "CURRENT").write_text("MANIFEST-000001\n", encoding="utf-8")
    if mpt:
        state = root / state_dir
        state.mkdir()
        (state / "CURRENT").write_text("MANIFEST-000001\n", encoding="utf-8")
        mpt_dir = state_dir
    else:
        mpt_dir = "missing-mpt"
    if not include_info:
        return

    info = {
        "label": "v509k",
        "saved_at": "2026-06-27T04:49:44Z",
        "height": "511289",
        "data_dir": "data/mainnet-replay",
        "mpt_dir": mpt_dir,
        "restore_verified": "true",
        "verified_height": "511289",
        "verified_stateroot_root": VERIFIED_ROOT,
        "verified_against_reference": "true",
    }
    if metadata:
        for key, value in metadata.items():
            if value is None:
                info.pop(key, None)
            else:
                info[key] = value
    (root / "CHECKPOINT_INFO").write_text(
        "".join(f"{key}={value}\n" for key, value in info.items()),
        encoding="utf-8",
    )


class BoundedMainnetReplayTests(unittest.TestCase):
    def test_plan_rejects_checkpoint_without_restore_verification_metadata(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            checkpoint = Path(tmp) / "checkpoint"
            write_checkpoint(checkpoint, mpt=True, include_info=False)

            with self.assertRaisesRegex(
                ValueError, "missing restore verification metadata"
            ):
                module.build_replay_plan(
                    checkpoint=checkpoint,
                    work_root=Path(tmp) / "work",
                    label="v509k-to-607262",
                    start_height=511289,
                    target_height=607262,
                    addresses=["NVU2QwsVdttjfTHQK7RYD6iwwfXRkSezGU"],
                )

    def test_plan_rejects_checkpoint_when_restore_verified_is_false(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            checkpoint = Path(tmp) / "checkpoint"
            write_checkpoint(
                checkpoint,
                mpt=True,
                metadata={"restore_verified": "false"},
            )

            with self.assertRaisesRegex(ValueError, "restore_verified=true"):
                module.build_replay_plan(
                    checkpoint=checkpoint,
                    work_root=Path(tmp) / "work",
                    label="v509k-to-607262",
                    start_height=511289,
                    target_height=607262,
                    addresses=["NVU2QwsVdttjfTHQK7RYD6iwwfXRkSezGU"],
                )

    def test_plan_rejects_checkpoint_without_reference_verification(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            checkpoint = Path(tmp) / "checkpoint"
            write_checkpoint(
                checkpoint,
                mpt=True,
                metadata={"verified_against_reference": "false"},
            )

            with self.assertRaisesRegex(ValueError, "verified_against_reference=true"):
                module.build_replay_plan(
                    checkpoint=checkpoint,
                    work_root=Path(tmp) / "work",
                    label="v509k-to-607262",
                    start_height=511289,
                    target_height=607262,
                    addresses=["NVU2QwsVdttjfTHQK7RYD6iwwfXRkSezGU"],
                )

    def test_plan_rejects_checkpoint_with_mismatched_verified_height(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            checkpoint = Path(tmp) / "checkpoint"
            write_checkpoint(
                checkpoint,
                mpt=True,
                metadata={"verified_height": "511288"},
            )

            with self.assertRaisesRegex(ValueError, "height does not match"):
                module.build_replay_plan(
                    checkpoint=checkpoint,
                    work_root=Path(tmp) / "work",
                    label="v509k-to-607262",
                    start_height=511289,
                    target_height=607262,
                    addresses=["NVU2QwsVdttjfTHQK7RYD6iwwfXRkSezGU"],
                )

    def test_plan_rejects_checkpoint_without_verified_stateroot_root(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            checkpoint = Path(tmp) / "checkpoint"
            write_checkpoint(
                checkpoint,
                mpt=True,
                metadata={"verified_stateroot_root": None},
            )

            with self.assertRaisesRegex(ValueError, "verified_stateroot_root"):
                module.build_replay_plan(
                    checkpoint=checkpoint,
                    work_root=Path(tmp) / "work",
                    label="v509k-to-607262",
                    start_height=511289,
                    target_height=607262,
                    addresses=["NVU2QwsVdttjfTHQK7RYD6iwwfXRkSezGU"],
                )

    def test_plan_rejects_checkpoint_without_state_root(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            checkpoint = Path(tmp) / "checkpoint"
            write_checkpoint(checkpoint, mpt=False)

            with self.assertRaisesRegex(ValueError, "full-state checkpoint"):
                module.build_replay_plan(
                    checkpoint=checkpoint,
                    work_root=Path(tmp) / "work",
                    label="v509k-to-607262",
                    start_height=511289,
                    target_height=607262,
                    addresses=["NVU2QwsVdttjfTHQK7RYD6iwwfXRkSezGU"],
                )

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
            write_checkpoint(checkpoint, mpt=True, metadata={"height": None})

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
            self.assertIn('backend = "mdbx"', config_text)
            self.assertIn('data_dir = "', config_text)
            self.assertIn("mdbx_geometry_upper_gb = 512", config_text)
            self.assertIn("mdbx_geometry_growth_mb = 256", config_text)
            self.assertIn("mdbx_max_readers = 4096", config_text)
            self.assertNotIn('backend = "rocksdb"', config_text)
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

    def test_plan_uses_real_checkpoint_layout_and_provider_metadata(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            checkpoint = Path(tmp) / "checkpoint"
            write_checkpoint(
                checkpoint,
                mpt=True,
                chain_dir="mainnet",
                state_dir="StateRoot",
                metadata={
                    "storage_provider": "rocksdb",
                    "chain_db": "data/mainnet-verified-h511289/chain",
                    "stateroot_db": "data/mainnet-verified-h511289/state-root-334F454E",
                    "mpt_dir": None,
                },
            )

            plan = module.build_replay_plan(
                checkpoint=checkpoint,
                work_root=Path(tmp) / "work",
                label="rocksdb-real-layout",
                start_height=511289,
                target_height=511300,
                addresses=[],
            )

        self.assertEqual(
            plan["chain_checkpoint"]["path"],
            str((checkpoint / "mainnet").resolve()),
        )
        self.assertEqual(
            plan["state_checkpoint"]["path"],
            str((checkpoint / "StateRoot").resolve()),
        )
        self.assertEqual(plan["storage_provider"], "rocksdb")
        self.assertIn('backend = "rocksdb"', plan["config_text"])
        self.assertNotIn("mdbx_geometry_upper_gb", plan["config_text"])
        self.assertIn("--storage-provider", module.step_by_name(plan, "run-node-with-repairs")["command"])
        self.assertIn("rocksdb", module.step_by_name(plan, "run-node-with-repairs")["command"])

    def test_plan_keeps_mdbx_default_for_checkpoints_without_provider_metadata(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            checkpoint = Path(tmp) / "checkpoint"
            write_checkpoint(checkpoint, mpt=True)

            plan = module.build_replay_plan(
                checkpoint=checkpoint,
                work_root=Path(tmp) / "work",
                label="default-mdbx",
                start_height=511289,
                target_height=511300,
                addresses=[],
            )

        self.assertEqual(plan["storage_provider"], "mdbx")
        self.assertIn('backend = "mdbx"', plan["config_text"])
        self.assertIn("mdbx_geometry_upper_gb = 512", plan["config_text"])


if __name__ == "__main__":
    unittest.main()
