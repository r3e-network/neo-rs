import importlib.util
import json
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace


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
    def create_full_state_checkpoint(
        self,
        checkpoint_root: Path,
        height: int,
        *,
        restore_verified: bool = True,
        verified_stateroot_root: str | None = None,
        verified_against_reference: bool = True,
        storage_provider: str | None = None,
    ) -> None:
        checkpoint = checkpoint_root / f"h{height}"
        (checkpoint / "mainnet").mkdir(parents=True)
        (checkpoint / "StateRoot").mkdir()
        info = f"height={height}\n"
        if storage_provider is not None:
            info += f"storage_provider={storage_provider}\n"
        if restore_verified:
            root = verified_stateroot_root or f"0xroot{height}"
            info += (
                "restore_verified=true\n"
                f"verified_height={height}\n"
                f"verified_stateroot_root={root}\n"
                f"verified_against_reference={str(verified_against_reference).lower()}\n"
            )
        (checkpoint / "CHECKPOINT_INFO").write_text(info, encoding="utf-8")

    def fake_probe_runner(self, command):
        db = Path(command[command.index("--db") + 1])
        height_text = next(
            (part[1:] for part in db.parts if part.startswith("h") and part[1:].isdigit()),
            None,
        )
        height = int(height_text or 0)
        if "--mpt-state-height" in command:
            return SimpleNamespace(
                stdout=json.dumps(
                    {"height": {"decoded": {"current_local_root_index": height}}}
                )
            )
        if "--mpt-state-root" in command:
            return SimpleNamespace(
                stdout=json.dumps(
                    {"state_root": {"decoded": {"roothash": f"0xroot{height}"}}}
                )
            )
        return SimpleNamespace(
            stdout=json.dumps({"decoded": {"format": "hash-index", "index": height}})
        )

    def test_checkpoint_verification_uses_checkpoint_storage_provider_metadata(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            checkpoint_root = Path(tmp) / "checkpoints"
            self.create_full_state_checkpoint(
                checkpoint_root,
                10,
                storage_provider="rocksdb",
            )
            providers = []

            def fake_runner(command):
                providers.append(command[command.index("--storage-provider") + 1])
                return self.fake_probe_runner(command)

            reason = module.checkpoint_verification_reason(
                checkpoint_root / "h10",
                probe_bin=Path("target/debug/neo-db-probe"),
                inventory_runner=fake_runner,
            )

        self.assertIsNone(reason)
        self.assertEqual(providers, ["rocksdb", "rocksdb", "rocksdb"])

    def checkpoint_status(
        self,
        *,
        base: int = 10,
        mid: int = 20,
        latest: int = 30,
        durable_height: int | None = 30,
        local_state_height: int | None = 30,
        local_validated_height: int | None = 30,
        last_validated_block: int | None = 30,
    ) -> dict:
        status = {
            "checkpoint_stages": [
                {"stage": "base", "height": base, "label": f"base-h{base}"},
                {"stage": "mid", "height": mid, "label": f"mid-h{mid}"},
                {"stage": "latest", "height": latest, "label": f"latest-h{latest}"},
            ],
        }
        if durable_height is not None:
            status["local_block_count"] = durable_height + 1
        if local_state_height is not None:
            status["local_state_height"] = local_state_height
        if local_validated_height is not None:
            status["local_validated_height"] = local_validated_height
        if last_validated_block is not None:
            status["last_validated_block"] = last_validated_block
        return status

    def test_plan_skips_existing_checkpoint_and_creates_missing_stages(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "checkpoints"
            self.create_full_state_checkpoint(root, 0)

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
                    ],
                    "last_validated_block": 120_000,
                    "local_block_count": 120_001,
                    "local_state_height": 120_000,
                    "local_validated_height": 120_000,
                },
                checkpoint_root=root,
                data_dir=Path(tmp) / "data",
                writer_pid="1234",
                script_path=Path("scripts/checkpoint-on-height.sh"),
                chain_db=None,
                stateroot_db=None,
                inventory_runner=self.fake_probe_runner,
            )

            self.assertEqual(plan["status"], "waiting")
            self.assertEqual(
                [(item["stage"], item["height"], item["action"]) for item in plan["actions"]],
                [
                    ("base", 0, "skip"),
                    ("mid", 60_000, "defer"),
                    ("latest", 120_000, "create"),
                ],
            )
            latest = plan["actions"][2]
            self.assertIn("--once", latest["command"])
            self.assertIn("--height", latest["command"])
            self.assertIn("120000", latest["command"])
            self.assertIn("--data-dir", latest["command"])
            self.assertIn("--root", latest["command"])
            self.assertEqual(
                latest["command"][latest["command"].index("--storage-provider") + 1],
                "rocksdb",
            )

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

    def test_plan_blocks_partial_checkpoint_stage_payload(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            plan = module.build_checkpoint_plan(
                {
                    "checkpoint_stages": [
                        {"stage": "latest", "height": 120_000, "label": "latest-h120000"}
                    ],
                    "last_validated_block": 120_000,
                    "local_block_count": 120_001,
                    "local_state_height": 120_000,
                    "local_validated_height": 120_000,
                },
                checkpoint_root=Path(tmp) / "checkpoints",
                data_dir=Path(tmp) / "data",
                writer_pid="none",
                script_path=Path("scripts/checkpoint-on-height.sh"),
                chain_db=None,
                stateroot_db=None,
            )

            self.assertEqual(plan["status"], "blocked")
            self.assertEqual(plan["actions"], [])
            self.assertIn("base", plan["reason"])
            self.assertIn("mid", plan["reason"])

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
            self.create_full_state_checkpoint(root, 0)
            self.create_full_state_checkpoint(root, 120_000)
            incomplete = root / "h60000"
            incomplete.mkdir(parents=True)
            (incomplete / "CHECKPOINT_IN_PROGRESS").write_text(
                "height=60000\n",
                encoding="utf-8",
            )

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
                writer_pid="none",
                script_path=Path("scripts/checkpoint-on-height.sh"),
                chain_db=None,
                stateroot_db=None,
            )

            self.assertEqual(plan["status"], "blocked")
            mid = next(item for item in plan["actions"] if item["stage"] == "mid")
            self.assertEqual(mid["action"], "blocked")
            self.assertIn("incomplete", mid["reason"])

    def test_plan_creates_duplicate_checkpoint_height_only_once(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            plan = module.build_checkpoint_plan(
                {
                    "checkpoint_stages": [
                        {"stage": "base", "height": 0, "label": "base-h0"},
                        {"stage": "mid", "height": 0, "label": "mid-h0"},
                        {"stage": "latest", "height": 0, "label": "latest-h0"},
                    ],
                    "last_validated_block": 0,
                    "local_block_count": 1,
                    "local_state_height": 0,
                    "local_validated_height": 0,
                },
                checkpoint_root=Path(tmp) / "checkpoints",
                data_dir=Path(tmp) / "data",
                writer_pid="none",
                script_path=Path("scripts/checkpoint-on-height.sh"),
                chain_db=None,
                stateroot_db=None,
                inventory_runner=self.fake_probe_runner,
            )

            self.assertEqual(
                [(item["stage"], item["action"]) for item in plan["actions"]],
                [
                    ("base", "create"),
                    ("mid", "skip"),
                    ("latest", "skip"),
                ],
            )
            self.assertEqual(plan["status"], "waiting")
            self.assertEqual(plan["usable_checkpoint_count"], 0)
            self.assertFalse(plan["minimum_usable_checkpoint_count_met"])
            self.assertEqual(plan["missing_usable_checkpoint_count"], 3)
            self.assertEqual(plan["projected_usable_checkpoint_count"], 1)
            self.assertFalse(plan["projected_minimum_usable_checkpoint_count_met"])
            self.assertEqual(plan["projected_missing_usable_checkpoint_count"], 2)
            self.assertIn(
                "current checkpoint stages can produce at most 1 usable full-state checkpoint",
                plan["reason"],
            )
            self.assertEqual(plan["actions"][1]["reason"], "checkpoint height already planned")

    def test_plan_ready_only_after_three_usable_full_state_checkpoints_exist(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "checkpoints"
            for height in [10, 20, 30]:
                self.create_full_state_checkpoint(root, height)

            status = self.checkpoint_status(base=10, mid=20, latest=30)
            status["checkpoint_stages"][2]["verified_stateroot_root"] = "0xroot30"
            status["checkpoint_stages"][2]["verified_against_reference"] = True

            plan = module.build_checkpoint_plan(
                status,
                checkpoint_root=root,
                data_dir=Path(tmp) / "data",
                writer_pid="none",
                script_path=Path("scripts/checkpoint-on-height.sh"),
                chain_db=None,
                stateroot_db=None,
                inventory_runner=self.fake_probe_runner,
            )

            self.assertEqual(plan["status"], "ready")
            self.assertEqual(plan["usable_checkpoint_count"], 3)
            self.assertTrue(plan["minimum_usable_checkpoint_count_met"])
            self.assertEqual(plan["missing_usable_checkpoint_count"], 0)
            self.assertEqual([item["action"] for item in plan["actions"]], ["skip", "skip", "skip"])

    def test_plan_does_not_count_metadata_only_checkpoints_as_usable(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "checkpoints"
            for height in [10, 20, 30]:
                self.create_full_state_checkpoint(root, height)

            plan = module.build_checkpoint_plan(
                self.checkpoint_status(base=10, mid=20, latest=30),
                checkpoint_root=root,
                data_dir=Path(tmp) / "data",
                writer_pid="none",
                script_path=Path("scripts/checkpoint-on-height.sh"),
                chain_db=None,
                stateroot_db=None,
            )

            self.assertEqual(plan["status"], "blocked")
            self.assertEqual(plan["usable_checkpoint_count"], 0)
            self.assertFalse(plan["minimum_usable_checkpoint_count_met"])
            self.assertIn("probe", plan["reason"])

    def test_structural_checkpoint_without_restore_verification_is_not_usable(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "checkpoints"
            for height in [10, 20, 30]:
                self.create_full_state_checkpoint(root, height, restore_verified=False)

            status = self.checkpoint_status(base=10, mid=20, latest=30)
            status["checkpoint_stages"][2]["verified_stateroot_root"] = "0xroot30"
            status["checkpoint_stages"][2]["verified_against_reference"] = True

            plan = module.build_checkpoint_plan(
                status,
                checkpoint_root=root,
                data_dir=Path(tmp) / "data",
                writer_pid="none",
                script_path=Path("scripts/checkpoint-on-height.sh"),
                chain_db=None,
                stateroot_db=None,
                inventory_runner=self.fake_probe_runner,
            )

            self.assertEqual(plan["status"], "blocked")
            self.assertEqual(plan["usable_checkpoint_count"], 0)
            self.assertIn("restore verification", plan["reason"])
            self.assertEqual(
                [item["action"] for item in plan["actions"]],
                ["blocked", "blocked", "blocked"],
            )

    def test_plan_waits_when_only_two_usable_full_state_checkpoints_exist(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "checkpoints"
            for height in [10, 20]:
                self.create_full_state_checkpoint(root, height)

            status = self.checkpoint_status(base=10, mid=20, latest=30)
            status["checkpoint_stages"][2]["verified_stateroot_root"] = "0xroot30"
            status["checkpoint_stages"][2]["verified_against_reference"] = True

            plan = module.build_checkpoint_plan(
                status,
                checkpoint_root=root,
                data_dir=Path(tmp) / "data",
                writer_pid="none",
                script_path=Path("scripts/checkpoint-on-height.sh"),
                chain_db=None,
                stateroot_db=None,
                inventory_runner=self.fake_probe_runner,
            )

            self.assertEqual(plan["status"], "waiting")
            self.assertEqual(plan["reason"], "need at least 3 usable full-state checkpoints; currently have 2")
            self.assertEqual(plan["usable_checkpoint_count"], 2)
            self.assertFalse(plan["minimum_usable_checkpoint_count_met"])
            self.assertEqual(plan["missing_usable_checkpoint_count"], 1)
            self.assertEqual([item["action"] for item in plan["actions"]], ["skip", "skip", "create"])

    def test_execute_plan_rechecks_inventory_after_creating_third_checkpoint(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "checkpoints"
            for height in [10, 20]:
                self.create_full_state_checkpoint(root, height)

            status = self.checkpoint_status(base=10, mid=20, latest=30)
            status["checkpoint_stages"][2]["verified_stateroot_root"] = "0xroot30"
            status["checkpoint_stages"][2]["verified_against_reference"] = True

            plan = module.build_checkpoint_plan(
                status,
                checkpoint_root=root,
                data_dir=Path(tmp) / "data",
                writer_pid="none",
                script_path=Path("scripts/checkpoint-on-height.sh"),
                chain_db=None,
                stateroot_db=None,
                inventory_runner=self.fake_probe_runner,
            )
            self.assertEqual(plan["status"], "waiting")

            def create_checkpoint(command):
                if Path(command[0]).name == "checkpoint-on-height.sh":
                    height = int(command[command.index("--height") + 1])
                    self.create_full_state_checkpoint(root, height, restore_verified=False)
                    return SimpleNamespace(stdout="")
                elif Path(command[0]).name == "restore-checkpoint.sh":
                    chain_db = Path(command[command.index("--chain-db") + 1])
                    stateroot_db = Path(command[command.index("--stateroot-db") + 1])
                    chain_db.mkdir(parents=True)
                    stateroot_db.mkdir(parents=True)
                    return SimpleNamespace(stdout="")
                elif Path(command[0]).name == "neo-db-probe":
                    return self.fake_probe_runner(command)
                raise AssertionError(f"unexpected command: {command}")

            result = module.execute_plan(plan, execute=True, runner=create_checkpoint)

            self.assertEqual(result["status"], "ready")
            self.assertEqual(result["executed"], 1)
            self.assertEqual(result["restore_probed"], 1)
            self.assertEqual(result["usable_checkpoint_count"], 3)
            self.assertTrue(result["minimum_usable_checkpoint_count_met"])
            self.assertEqual(result["missing_usable_checkpoint_count"], 0)

    def test_plan_defers_historical_stage_when_live_chain_is_already_ahead(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            plan = module.build_checkpoint_plan(
                {
                    "checkpoint_stages": [
                        {"stage": "base", "height": 0, "label": "base-h0"},
                        {"stage": "mid", "height": 0, "label": "mid-h0"},
                        {"stage": "latest", "height": 0, "label": "latest-h0"},
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

    def test_plan_defers_checkpoint_when_durable_height_fields_are_missing(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            plan = module.build_checkpoint_plan(
                {
                    "checkpoint_stages": [
                        {"stage": "base", "height": 120_000, "label": "base-h120000"},
                        {"stage": "mid", "height": 120_000, "label": "mid-h120000"},
                        {"stage": "latest", "height": 120_000, "label": "latest-h120000"},
                    ],
                    "last_validated_block": 120_000,
                },
                checkpoint_root=Path(tmp) / "checkpoints",
                data_dir=Path(tmp) / "data",
                writer_pid="none",
                script_path=Path("scripts/checkpoint-on-height.sh"),
                chain_db=None,
                stateroot_db=None,
            )

            self.assertEqual(plan["status"], "waiting")
            self.assertEqual(plan["actions"][0]["action"], "defer")
            self.assertIn("durable height fields", plan["actions"][0]["reason"])

    def test_plan_defers_checkpoint_when_validated_height_field_is_missing(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            plan = module.build_checkpoint_plan(
                {
                    "checkpoint_stages": [
                        {"stage": "base", "height": 120_000, "label": "base-h120000"},
                        {"stage": "mid", "height": 120_000, "label": "mid-h120000"},
                        {"stage": "latest", "height": 120_000, "label": "latest-h120000"},
                    ],
                    "last_validated_block": 120_000,
                    "local_block_count": 120_001,
                    "local_state_height": 120_000,
                },
                checkpoint_root=Path(tmp) / "checkpoints",
                data_dir=Path(tmp) / "data",
                writer_pid="none",
                script_path=Path("scripts/checkpoint-on-height.sh"),
                chain_db=None,
                stateroot_db=None,
            )

            self.assertEqual(plan["status"], "waiting")
            self.assertEqual(plan["actions"][0]["action"], "defer")
            self.assertIn("validated height", plan["actions"][0]["reason"])

    def test_plan_passes_explicit_validation_data_dirs_to_checkpoint_script(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            chain_db = root / "data" / "mainnet-validate"
            stateroot_db = root / "Data_MPT_validate_334F454E"
            plan = module.build_checkpoint_plan(
                {
                    "checkpoint_stages": [
                        {"stage": "base", "height": 50_900, "label": "base-h50900"},
                        {"stage": "mid", "height": 50_900, "label": "mid-h50900"},
                        {"stage": "latest", "height": 50_900, "label": "latest-h50900"},
                    ],
                    "last_validated_block": 50_900,
                    "local_block_count": 50_901,
                    "local_state_height": 50_900,
                    "local_validated_height": 50_900,
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

    def test_plan_passes_configured_probe_binary_to_restore_verification(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            plan = module.build_checkpoint_plan(
                self.checkpoint_status(base=30, mid=30, latest=30),
                checkpoint_root=root / "checkpoints",
                data_dir=root / "data",
                writer_pid="none",
                script_path=Path("scripts/checkpoint-on-height.sh"),
                chain_db=root / "data" / "mainnet-validate",
                stateroot_db=root / "Data_MPT_validate_334F454E",
                probe_bin=Path("target/release/neo-db-probe"),
            )

            action = plan["actions"][0]

            self.assertEqual(
                action["restore_probe_chain_height_command"][0],
                "target/release/neo-db-probe",
            )
            self.assertEqual(
                action["restore_probe_stateroot_height_command"][0],
                "target/release/neo-db-probe",
            )

    def test_create_action_defers_restore_verified_metadata_until_probe(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            plan = module.build_checkpoint_plan(
                self.checkpoint_status(base=30, mid=30, latest=30),
                checkpoint_root=root / "checkpoints",
                data_dir=root / "data",
                writer_pid="none",
                script_path=Path("scripts/checkpoint-on-height.sh"),
                chain_db=root / "data" / "mainnet-validate",
                stateroot_db=root / "Data_MPT_validate_334F454E",
            )

            command = plan["actions"][0]["command"]

            self.assertNotIn("--restore-verified", command)
            self.assertNotIn("--verified-height", command)
            self.assertNotIn("--verified-against-reference", command)
            self.assertTrue(plan["actions"][0]["requires_restore_probe"])
            self.assertIn(
                "--allow-unverified",
                plan["actions"][0]["restore_probe_command"],
            )

    def test_execute_plan_marks_checkpoint_verified_after_restore_probe(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            checkpoint_root = root / "checkpoints"
            commands = []
            plan = module.build_checkpoint_plan(
                self.checkpoint_status(base=30, mid=30, latest=30),
                checkpoint_root=checkpoint_root,
                data_dir=root / "data",
                writer_pid="none",
                script_path=Path("scripts/checkpoint-on-height.sh"),
                chain_db=root / "data" / "mainnet-validate",
                stateroot_db=root / "Data_MPT_validate_334F454E",
            )

            def fake_runner(command):
                commands.append(command)
                if command[0] == "scripts/checkpoint-on-height.sh":
                    height = int(command[command.index("--height") + 1])
                    checkpoint = checkpoint_root / f"h{height}"
                    (checkpoint / "mainnet").mkdir(parents=True)
                    (checkpoint / "StateRoot").mkdir()
                    (checkpoint / "CHECKPOINT_INFO").write_text(
                        f"height={height}\n",
                        encoding="utf-8",
                    )
                    return SimpleNamespace(stdout="")
                elif command[0] == "scripts/restore-checkpoint.sh":
                    chain_db = Path(command[command.index("--chain-db") + 1])
                    stateroot_db = Path(command[command.index("--stateroot-db") + 1])
                    chain_db.mkdir(parents=True)
                    stateroot_db.mkdir(parents=True)
                    return SimpleNamespace(stdout="")
                elif command[0] == "target/debug/neo-db-probe":
                    if "--mpt-state-height" in command:
                        return SimpleNamespace(
                            stdout=json.dumps(
                                {
                                    "height": {
                                        "decoded": {"current_local_root_index": 30}
                                    }
                                }
                            )
                        )
                    if "--mpt-state-root" in command:
                        return SimpleNamespace(
                            stdout=json.dumps(
                                {
                                    "state_root": {
                                        "decoded": {"roothash": "0xroot30"}
                                    }
                                }
                            )
                        )
                    return SimpleNamespace(
                        stdout=json.dumps({"decoded": {"format": "hash-index", "index": 30}})
                    )
                raise AssertionError(f"unexpected command: {command}")

            result = module.execute_plan(plan, execute=True, runner=fake_runner)

            self.assertEqual(result["executed"], 1)
            self.assertEqual(result["restore_probed"], 1)
            self.assertEqual(
                [Path(command[0]).name for command in commands],
                [
                    "checkpoint-on-height.sh",
                    "restore-checkpoint.sh",
                    "neo-db-probe",
                    "neo-db-probe",
                    "neo-db-probe",
                ],
            )
            info = (checkpoint_root / "h30" / "CHECKPOINT_INFO").read_text(
                encoding="utf-8"
            )
            self.assertIn("restore_verified=true", info)
            self.assertIn("verified_height=30", info)
            self.assertIn("verified_stateroot_root=0xroot30", info)
            self.assertIn("verified_against_reference=false", info)

    def test_execute_plan_marks_reference_verified_only_with_reference_backed_stage_root(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            checkpoint_root = root / "checkpoints"
            plan = module.build_checkpoint_plan(
                {
                    "checkpoint_stages": [
                        {
                            "stage": "base",
                            "height": 30,
                            "verified_stateroot_root": "0xroot30",
                            "verified_against_reference": True,
                        },
                        {
                            "stage": "mid",
                            "height": 30,
                            "verified_stateroot_root": "0xroot30",
                            "verified_against_reference": True,
                        },
                        {
                            "stage": "latest",
                            "height": 30,
                            "verified_stateroot_root": "0xroot30",
                            "verified_against_reference": True,
                        },
                    ],
                    "last_validated_block": 30,
                    "local_block_count": 31,
                    "local_state_height": 30,
                    "local_validated_height": 30,
                },
                checkpoint_root=checkpoint_root,
                data_dir=root / "data",
                writer_pid="none",
                script_path=Path("scripts/checkpoint-on-height.sh"),
                chain_db=root / "data" / "mainnet-validate",
                stateroot_db=root / "Data_MPT_validate_334F454E",
            )

            def fake_runner(command):
                if command[0] == "scripts/checkpoint-on-height.sh":
                    height = int(command[command.index("--height") + 1])
                    checkpoint = checkpoint_root / f"h{height}"
                    (checkpoint / "mainnet").mkdir(parents=True)
                    (checkpoint / "StateRoot").mkdir()
                    (checkpoint / "CHECKPOINT_INFO").write_text(
                        f"height={height}\n",
                        encoding="utf-8",
                    )
                    return SimpleNamespace(stdout="")
                if command[0] == "scripts/restore-checkpoint.sh":
                    Path(command[command.index("--chain-db") + 1]).mkdir(parents=True)
                    Path(command[command.index("--stateroot-db") + 1]).mkdir(parents=True)
                    return SimpleNamespace(stdout="")
                if command[0] == "target/debug/neo-db-probe":
                    return self.fake_probe_runner(command)
                raise AssertionError(f"unexpected command: {command}")

            module.execute_plan(plan, execute=True, runner=fake_runner)

            info = (checkpoint_root / "h30" / "CHECKPOINT_INFO").read_text(
                encoding="utf-8"
            )
            self.assertIn("verified_against_reference=true", info)

    def test_execute_plan_refuses_restore_verified_marker_when_restored_stateroot_root_mismatches(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            checkpoint_root = root / "checkpoints"
            plan = module.build_checkpoint_plan(
                self.checkpoint_status(base=30, mid=30, latest=30),
                checkpoint_root=checkpoint_root,
                data_dir=root / "data",
                writer_pid="none",
                script_path=Path("scripts/checkpoint-on-height.sh"),
                chain_db=root / "data" / "mainnet-validate",
                stateroot_db=root / "Data_MPT_validate_334F454E",
            )

            def fake_runner(command):
                if command[0] == "scripts/checkpoint-on-height.sh":
                    height = int(command[command.index("--height") + 1])
                    checkpoint = checkpoint_root / f"h{height}"
                    (checkpoint / "mainnet").mkdir(parents=True)
                    (checkpoint / "StateRoot").mkdir()
                    (checkpoint / "CHECKPOINT_INFO").write_text(
                        f"height={height}\nexpected_stateroot_root=0xroot{height}\n",
                        encoding="utf-8",
                    )
                    return SimpleNamespace(stdout="")
                if command[0] == "scripts/restore-checkpoint.sh":
                    chain_db = Path(command[command.index("--chain-db") + 1])
                    stateroot_db = Path(command[command.index("--stateroot-db") + 1])
                    chain_db.mkdir(parents=True)
                    stateroot_db.mkdir(parents=True)
                    return SimpleNamespace(stdout="")
                if command[0] == "target/debug/neo-db-probe":
                    if "--mpt-state-height" in command:
                        return SimpleNamespace(
                            stdout=json.dumps(
                                {
                                    "height": {
                                        "decoded": {"current_local_root_index": 30}
                                    }
                                }
                            )
                        )
                    if "--mpt-state-root" in command:
                        return SimpleNamespace(
                            stdout=json.dumps(
                                {
                                    "state_root": {
                                        "decoded": {"roothash": "0xdifferent"}
                                    }
                                }
                            )
                        )
                    return SimpleNamespace(
                        stdout=json.dumps({"decoded": {"format": "hash-index", "index": 30}})
                    )
                raise AssertionError(f"unexpected command: {command}")

            with self.assertRaisesRegex(RuntimeError, "restored StateRoot root"):
                module.execute_plan(plan, execute=True, runner=fake_runner)

            info = (checkpoint_root / "h30" / "CHECKPOINT_INFO").read_text(
                encoding="utf-8"
            )
            self.assertNotIn("restore_verified=true", info)

    def test_execute_plan_refuses_restore_verified_marker_when_restored_chain_height_mismatches(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            checkpoint_root = root / "checkpoints"
            plan = module.build_checkpoint_plan(
                self.checkpoint_status(base=30, mid=30, latest=30),
                checkpoint_root=checkpoint_root,
                data_dir=root / "data",
                writer_pid="none",
                script_path=Path("scripts/checkpoint-on-height.sh"),
                chain_db=root / "data" / "mainnet-validate",
                stateroot_db=root / "Data_MPT_validate_334F454E",
            )
            commands = []

            def fake_runner(command):
                commands.append(command)
                if command[0] == "scripts/checkpoint-on-height.sh":
                    height = int(command[command.index("--height") + 1])
                    checkpoint = checkpoint_root / f"h{height}"
                    (checkpoint / "mainnet").mkdir(parents=True)
                    (checkpoint / "StateRoot").mkdir()
                    (checkpoint / "CHECKPOINT_INFO").write_text(
                        f"height={height}\n",
                        encoding="utf-8",
                    )
                    return SimpleNamespace(stdout="")
                if command[0] == "scripts/restore-checkpoint.sh":
                    chain_db = Path(command[command.index("--chain-db") + 1])
                    stateroot_db = Path(command[command.index("--stateroot-db") + 1])
                    chain_db.mkdir(parents=True)
                    stateroot_db.mkdir(parents=True)
                    return SimpleNamespace(stdout="")
                if command[0] == "target/debug/neo-db-probe":
                    if "--mpt-state-height" in command:
                        return SimpleNamespace(
                            stdout=json.dumps(
                                {
                                    "height": {
                                        "decoded": {"current_local_root_index": 30}
                                    }
                                }
                            )
                        )
                    if "--mpt-state-root" in command:
                        return SimpleNamespace(
                            stdout=json.dumps(
                                {
                                    "state_root": {
                                        "decoded": {"roothash": "0xroot30"}
                                    }
                                }
                            )
                        )
                    return SimpleNamespace(
                        stdout=json.dumps({"decoded": {"format": "hash-index", "index": 29}})
                    )
                raise AssertionError(f"unexpected command: {command}")

            with self.assertRaisesRegex(RuntimeError, "restored chain height"):
                module.execute_plan(plan, execute=True, runner=fake_runner)

            info = (checkpoint_root / "h30" / "CHECKPOINT_INFO").read_text(
                encoding="utf-8"
            )
            self.assertNotIn("restore_verified=true", info)
            self.assertTrue(
                any(command[0] == "target/debug/neo-db-probe" for command in commands),
                "restore verification must probe the restored scratch DB before marking metadata",
            )

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
                Path("./data/mainnet-validate"),
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
            {
                "checkpoint_stages": [
                    {"stage": "base", "height": 10},
                    {"stage": "mid", "height": 10},
                    {"stage": "latest", "height": 10},
                ],
                "last_validated_block": 10,
                "local_block_count": 11,
                "local_state_height": 10,
                "local_validated_height": 10,
            },
            {
                "checkpoint_stages": [
                    {"stage": "base", "height": 20},
                    {"stage": "mid", "height": 20},
                    {"stage": "latest", "height": 20},
                ],
                "last_validated_block": 20,
                "local_block_count": 21,
                "local_state_height": 20,
                "local_validated_height": 20,
            },
        ]
        commands = []
        sleeps = []
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            checkpoint_root = root / "checkpoints"
            data_dir = root / "data"

            def fake_runner(command):
                commands.append(command)
                if Path(command[0]).name == "checkpoint-on-height.sh":
                    height = int(command[command.index("--height") + 1])
                    self.create_full_state_checkpoint(
                        checkpoint_root,
                        height,
                        restore_verified=False,
                    )
                    return SimpleNamespace(stdout="")
                elif Path(command[0]).name == "restore-checkpoint.sh":
                    chain_db = Path(command[command.index("--chain-db") + 1])
                    stateroot_db = Path(command[command.index("--stateroot-db") + 1])
                    chain_db.mkdir(parents=True)
                    stateroot_db.mkdir(parents=True)
                    return SimpleNamespace(stdout="")
                elif Path(command[0]).name == "neo-db-probe":
                    height = int(Path(command[command.index("--db") + 1]).parent.name[1:])
                    if "--mpt-state-height" in command:
                        return SimpleNamespace(
                            stdout=json.dumps(
                                {
                                    "height": {
                                        "decoded": {
                                            "current_local_root_index": height
                                        }
                                    }
                                }
                            )
                        )
                    if "--mpt-state-root" in command:
                        return SimpleNamespace(
                            stdout=json.dumps(
                                {
                                    "state_root": {
                                        "decoded": {"roothash": f"0xroot{height}"}
                                    }
                                }
                            )
                        )
                    return SimpleNamespace(
                        stdout=json.dumps(
                            {"decoded": {"format": "hash-index", "index": height}}
                        )
                    )
                raise AssertionError(f"unexpected command: {command}")

            result = module.run_watch_loop(
                status_loader=lambda: statuses.pop(0),
                plan_builder=lambda status: module.build_checkpoint_plan(
                    status,
                    checkpoint_root=checkpoint_root,
                    data_dir=data_dir,
                    writer_pid="1234",
                    script_path=Path("scripts/checkpoint-on-height.sh"),
                    chain_db=data_dir / "mainnet-validate",
                    stateroot_db=root / "Data_MPT_validate_334F454E",
                ),
                execute=True,
                runner=fake_runner,
                interval_seconds=30,
                max_iterations=2,
                sleep_fn=sleeps.append,
            )

        self.assertEqual(result["mode"], "watch")
        self.assertEqual(result["iterations"], 2)
        checkpoint_commands = [
            command for command in commands if Path(command[0]).name == "checkpoint-on-height.sh"
        ]
        restore_commands = [
            command for command in commands if Path(command[0]).name == "restore-checkpoint.sh"
        ]
        self.assertEqual(
            [command[command.index("--height") + 1] for command in checkpoint_commands],
            ["10", "20"],
        )
        self.assertEqual(
            [command[1] for command in restore_commands],
            ["10", "20"],
        )
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
        self.assertIn("restore_verified=true", text)
        self.assertIn("structurally present, not restore-verified", text)


if __name__ == "__main__":
    unittest.main()
