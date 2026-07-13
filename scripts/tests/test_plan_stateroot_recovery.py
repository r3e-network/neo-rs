import importlib.util
import json
import subprocess
import tempfile
import unittest
import unittest.mock
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
    del stateroot_db
    path.write_text(
        f"""
[network]
network_magic = 0x334F454E

[storage]
backend = "mdbx"
data_dir = "{chain_db}"

[state_service]
enabled = true
track_during_catchup = true
""",
        encoding="utf-8",
    )


def create_store_dirs(*paths: Path) -> None:
    for path in paths:
        path.mkdir(parents=True)
        (path / "mdbx.dat").write_bytes(b"mdbx")


def create_mdbx_store_dirs(*paths: Path) -> None:
    for path in paths:
        path.mkdir(parents=True)
        (path / "mdbx.dat").write_bytes(b"mdbx")
        (path / "mdbx.lck").write_bytes(b"")


def write_checkpoint(
    root: Path,
    *,
    height: int,
    full_state: bool,
    restore_verified: bool = True,
) -> Path:
    checkpoint = root / f"h{height}"
    (checkpoint / "mainnet").mkdir(parents=True)
    (checkpoint / "mainnet" / "mdbx.dat").write_bytes(b"mdbx-checkpoint")
    lines = [
        f"height={height}",
        "storage_provider=mdbx",
        "state_root_layout=coordinated_mdbx",
    ]
    if full_state:
        lines.append("state_root_included=true")
        if restore_verified:
            lines.extend(
                [
                    "restore_verified=true",
                    f"verified_height={height}",
                    f"verified_stateroot_root=0xroot{height}",
                    "verified_against_reference=true",
                ]
            )
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

    def test_explicit_db_overrides_drive_probe_paths_and_restore_command(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            checkpoints = root / "checkpoints"
            write_checkpoint(checkpoints, height=100, full_state=True)
            config = root / "neo.toml"
            write_config(
                config,
                chain_db=root / "stale-config-chain",
                stateroot_db=root / "stale-config-state",
            )
            runtime_chain = root / "runtime" / "chain"
            create_mdbx_store_dirs(runtime_chain)
            probe_commands = []

            def run(command, *, check, capture_output, text):
                probe_commands.append([str(part) for part in command])
                return fake_probe_runner(chain_height=250, state_height=0)(
                    command,
                    check=check,
                    capture_output=capture_output,
                    text=text,
                )

            plan = module.build_recovery_plan(
                node_config=config,
                checkpoint_root=checkpoints,
                chain_db=runtime_chain,
                runner=run,
            )

        self.assertEqual(plan["mode"], "restore-full-state-checkpoint")
        self.assertEqual(plan["storage_provider"], "mdbx")
        self.assertEqual(plan["chain"]["path"], str(runtime_chain))
        self.assertEqual(plan["state_service"]["path"], str(runtime_chain))
        self.assertEqual(
            probe_commands[0][probe_commands[0].index("--db") + 1],
            str(runtime_chain),
        )
        self.assertEqual(
            probe_commands[1][probe_commands[1].index("--db") + 1],
            str(runtime_chain),
        )
        self.assertTrue(all("--storage-provider" not in command for command in probe_commands))
        restore_command = plan["recommended_action"]["commands"][0]
        self.assertEqual(
            restore_command[restore_command.index("--chain-db") + 1],
            str(runtime_chain),
        )
        self.assertNotIn("--stateroot-db", restore_command)

    def test_cli_accepts_explicit_db_overrides(self):
        module = load_module()
        with unittest.mock.patch.object(
            module.sys,
            "argv",
            [
                "plan-stateroot-recovery.py",
                "--node-config",
                "neo.toml",
                "--chain-db",
                "runtime-chain",
            ],
        ):
            args = module.parse_args()

        self.assertEqual(args.node_config, "neo.toml")
        self.assertEqual(args.chain_db, "runtime-chain")

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
        self.assertNotIn("--stateroot-db", command)
        self.assertIn("--dry-run", command)
        self.assertEqual(len(plan["checkpoints"]["full_state"]), 1)
        self.assertEqual(len(plan["checkpoints"]["chain_only"]), 1)
        self.assertEqual(plan["checkpoints"]["usable_full_state_count"], 1)
        self.assertEqual(plan["checkpoints"]["minimum_full_state_count"], 3)
        self.assertFalse(plan["checkpoints"]["minimum_full_state_count_met"])
        self.assertEqual(plan["checkpoints"]["missing_full_state_count"], 2)
        self.assertEqual(plan["checkpoints"]["usable_full_state_heights"], [100])

    def test_reports_when_three_usable_full_state_checkpoints_are_available(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            checkpoints = root / "checkpoints"
            for height in (100, 200, 300):
                write_checkpoint(checkpoints, height=height, full_state=True)
            config = root / "neo.toml"
            chain_db = root / "chain"
            stateroot_db = root / "state"
            create_store_dirs(chain_db, stateroot_db)
            write_config(config, chain_db=chain_db, stateroot_db=stateroot_db)

            plan = module.build_recovery_plan(
                node_config=config,
                checkpoint_root=checkpoints,
                runner=fake_probe_runner(chain_height=350, state_height=0),
            )

        self.assertEqual(plan["mode"], "restore-full-state-checkpoint")
        self.assertEqual(plan["recommended_action"]["checkpoint"]["height"], 300)
        self.assertEqual(plan["checkpoints"]["usable_full_state_count"], 3)
        self.assertEqual(plan["checkpoints"]["minimum_full_state_count"], 3)
        self.assertTrue(plan["checkpoints"]["minimum_full_state_count_met"])
        self.assertEqual(plan["checkpoints"]["missing_full_state_count"], 0)
        self.assertEqual(
            plan["checkpoints"]["usable_full_state_heights"],
            [100, 200, 300],
        )

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

    def test_ignores_legacy_storage_sample_checkpoint_directories(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            checkpoints = root / "checkpoints"
            legacy = checkpoints / "mainnet-bounded-700000-stable"
            (legacy / "data").mkdir(parents=True)
            (legacy / "data" / "CURRENT").write_text(
                "MANIFEST-chain\n",
                encoding="utf-8",
            )
            (legacy / "CHECKPOINT_INFO").write_text(
                "height=700000\nmode=storage-sample\nmpt_dir=missing-mpt\n",
                encoding="utf-8",
            )

            self.assertEqual(module.scan_checkpoints(checkpoints), [])

    def test_unverified_full_state_checkpoint_is_not_usable_for_recovery(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            checkpoints = root / "checkpoints"
            write_checkpoint(
                checkpoints,
                height=100,
                full_state=True,
                restore_verified=False,
            )
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

        self.assertEqual(plan["mode"], "clean-replay-required")
        self.assertEqual(plan["checkpoints"]["full_state"], [])
        self.assertFalse(plan["checkpoints"]["all"][0]["usable_for_state_validation"])
        self.assertIn(
            "restore verification",
            plan["checkpoints"]["all"][0]["restore_verification_reason"],
        )

    def test_full_state_checkpoint_without_verified_root_is_not_usable_for_recovery(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            checkpoints = root / "checkpoints"
            checkpoint = write_checkpoint(checkpoints, height=100, full_state=True)
            info = (checkpoint / "CHECKPOINT_INFO").read_text(encoding="utf-8")
            (checkpoint / "CHECKPOINT_INFO").write_text(
                info.replace("verified_stateroot_root=0xroot100\n", ""),
                encoding="utf-8",
            )
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

        self.assertEqual(plan["mode"], "clean-replay-required")
        self.assertEqual(plan["checkpoints"]["full_state"], [])
        self.assertIn(
            "verified_stateroot_root",
            plan["checkpoints"]["all"][0]["restore_verification_reason"],
        )

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
