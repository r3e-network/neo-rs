import importlib.util
import json
import subprocess
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).resolve().parents[1] / "plan-stateroot-recovery.py"


def load_module():
    spec = importlib.util.spec_from_file_location("plan_stateroot_recovery", MODULE_PATH)
    if spec is None or spec.loader is None:
        raise ImportError(f"unable to load module from {MODULE_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def write_config(path: Path, *, chain_db: Path, stateroot_db: Path) -> None:
    path.write_text(
        f"""
[network]
network_magic = 0x334F454E

[storage]
backend = "rocksdb"
data_dir = "{chain_db}"

[state_service]
enabled = true
path = "{stateroot_db}"
track_during_catchup = true
""",
        encoding="utf-8",
    )


def create_store_dirs(*paths: Path) -> None:
    for path in paths:
        path.mkdir(parents=True)
        (path / "CURRENT").write_text("MANIFEST-000001\n", encoding="utf-8")


def write_checkpoint(root: Path, *, height: int, full_state: bool) -> Path:
    checkpoint = root / f"h{height}"
    (checkpoint / "mainnet").mkdir(parents=True)
    (checkpoint / "mainnet" / "CURRENT").write_text("MANIFEST-chain\n", encoding="utf-8")
    lines = [f"height={height}"]
    if full_state:
        (checkpoint / "StateRoot").mkdir()
        (checkpoint / "StateRoot" / "CURRENT").write_text(
            "MANIFEST-state\n",
            encoding="utf-8",
        )
        lines.append("state_root_included=true")
    else:
        lines.append("state_root_included=false")
    (checkpoint / "CHECKPOINT_INFO").write_text("\n".join(lines) + "\n", encoding="utf-8")
    return checkpoint


class Completed:
    def __init__(self, payload):
        self.stdout = json.dumps(payload)
        self.returncode = 0


def fake_probe_runner(*, chain_height, state_height):
    def run(command, *, check, capture_output, text):
        del check, capture_output, text
        if "--mpt-state-height" in command:
            payload = {
                "height": {
                    "found": state_height is not None,
                    "decoded": {"current_local_root_index": state_height},
                }
            }
        else:
            payload = {
                "found": chain_height is not None,
                "decoded": {"index": chain_height},
            }
        return Completed(payload)

    return run


class StateRootRecoveryPlanTests(unittest.TestCase):
    def test_fresh_empty_chain_store_is_ready_for_clean_replay(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            config = root / "neo.toml"
            chain_db = root / "chain"
            stateroot_db = root / "state"
            create_store_dirs(chain_db, stateroot_db)
            write_config(config, chain_db=chain_db, stateroot_db=stateroot_db)

            plan = module.build_recovery_plan(
                node_config=config,
                checkpoint_root=root / "checkpoints",
                runner=fake_probe_runner(chain_height=None, state_height=None),
            )

        self.assertEqual(plan["mode"], "fresh-replay-ready")
        self.assertEqual(plan["recommended_action"]["action"], "start-validation-stack")

    def test_missing_chain_and_state_dirs_are_treated_as_fresh(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            config = root / "neo.toml"
            write_config(config, chain_db=root / "missing-chain", stateroot_db=root / "missing-state")

            plan = module.build_recovery_plan(
                node_config=config,
                checkpoint_root=root / "checkpoints",
            )

        self.assertEqual(plan["mode"], "fresh-replay-ready")
        self.assertFalse(plan["chain"]["found"])
        self.assertFalse(plan["state_service"]["found"])
        self.assertNotIn("error", plan["chain"])
        self.assertNotIn("error", plan["state_service"])

    def test_ready_when_chain_and_stateroot_heights_match(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            config = root / "neo.toml"
            chain_db = root / "chain"
            stateroot_db = root / "state"
            create_store_dirs(chain_db, stateroot_db)
            write_config(config, chain_db=chain_db, stateroot_db=stateroot_db)

            plan = module.build_recovery_plan(
                node_config=config,
                checkpoint_root=root / "checkpoints",
                runner=fake_probe_runner(chain_height=42, state_height=42),
            )

        self.assertEqual(plan["mode"], "ready")
        self.assertTrue(plan["state_service"]["matches_chain"])
        self.assertEqual(plan["recommended_action"]["action"], "start-validation-stack")

    def test_chooses_nearest_full_state_checkpoint_before_chain_height(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            checkpoints = root / "checkpoints"
            write_checkpoint(checkpoints, height=100, full_state=True)
            write_checkpoint(checkpoints, height=200, full_state=False)
            config = root / "neo.toml"
            chain_db = root / "chain"
            stateroot_db = root / "state"
            create_store_dirs(chain_db, stateroot_db)
            write_config(config, chain_db=chain_db, stateroot_db=stateroot_db)

            plan = module.build_recovery_plan(
                node_config=config,
                checkpoint_root=checkpoints,
                runner=fake_probe_runner(chain_height=250, state_height=0),
            )

        self.assertEqual(plan["mode"], "restore-full-state-checkpoint")
        self.assertEqual(plan["recommended_action"]["checkpoint"]["height"], 100)
        command = plan["recommended_action"]["commands"][0]
        self.assertIn("--stateroot-db", command)
        self.assertIn("--dry-run", command)
        self.assertEqual(len(plan["checkpoints"]["full_state"]), 1)
        self.assertEqual(len(plan["checkpoints"]["chain_only"]), 1)

    def test_requires_clean_replay_when_only_chain_checkpoints_exist(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            checkpoints = root / "checkpoints"
            write_checkpoint(checkpoints, height=624433, full_state=False)
            config = root / "neo.toml"
            chain_db = root / "chain"
            stateroot_db = root / "state"
            create_store_dirs(chain_db, stateroot_db)
            write_config(config, chain_db=chain_db, stateroot_db=stateroot_db)

            plan = module.build_recovery_plan(
                node_config=config,
                checkpoint_root=checkpoints,
                runner=fake_probe_runner(chain_height=474701, state_height=0),
            )

        self.assertEqual(plan["mode"], "clean-replay-required")
        self.assertEqual(plan["recommended_action"]["action"], "clean-replay-from-genesis")
        self.assertIn("chain-only", plan["recommended_action"]["reason"])
        self.assertIn(
            "scripts/prepare-clean-stateroot-validation.py",
            plan["recommended_action"]["commands"][0],
        )
        self.assertIn("--base-config", plan["recommended_action"]["commands"][0])
        self.assertEqual(plan["checkpoints"]["full_state"], [])
        self.assertEqual(plan["checkpoints"]["chain_only"][0]["height"], 624433)

    def test_probe_failure_is_reported_without_crashing_plan(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            config = root / "neo.toml"
            chain_db = root / "chain"
            stateroot_db = root / "state"
            create_store_dirs(chain_db, stateroot_db)
            write_config(config, chain_db=chain_db, stateroot_db=stateroot_db)

            def fail_probe(command, *, check, capture_output, text):
                del capture_output, text
                raise subprocess.CalledProcessError(1, command, stderr="probe failed")

            plan = module.build_recovery_plan(
                node_config=config,
                checkpoint_root=root / "checkpoints",
                runner=fail_probe,
            )

        self.assertEqual(plan["mode"], "clean-replay-required")
        self.assertIn("error", plan["chain"])
        self.assertIn("error", plan["state_service"])


if __name__ == "__main__":
    unittest.main()
