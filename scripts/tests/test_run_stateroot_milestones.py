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
    def create_full_state_checkpoint(
        self,
        checkpoint_root: Path,
        height: int,
        *,
        restore_verified: bool = True,
        verified_stateroot_root: str | None = None,
        storage_provider: str | None = None,
    ) -> None:
        checkpoint = checkpoint_root / f"h{height}"
        (checkpoint / "mainnet").mkdir(parents=True)
        (checkpoint / "StateRoot").mkdir()
        info = f"height={height}\nstate_root_included=true\n"
        if storage_provider is not None:
            info += f"storage_provider={storage_provider}\n"
        if restore_verified:
            root = verified_stateroot_root or f"0xroot{height}"
            info += (
                "restore_verified=true\n"
                f"verified_height={height}\n"
                f"verified_stateroot_root={root}\n"
                "verified_against_reference=true\n"
            )
        (checkpoint / "CHECKPOINT_INFO").write_text(info, encoding="utf-8")

    def fake_probe_result(self, command: list[str]) -> SimpleNamespace | None:
        if "neo-db-probe" not in str(command[0]):
            return None
        db_path = Path(command[command.index("--db") + 1])
        height_text = next(
            (part[1:] for part in db_path.parts if part.startswith("h") and part[1:].isdigit()),
            None,
        )
        height = int(height_text or 0)
        if ".restore-probe" in db_path.parts and height == 0:
            checkpoint_root = db_path.parents[2]
            checkpoint_height = db_path.parents[0].name[1:]
            source_db = checkpoint_root / f"h{checkpoint_height}" / db_path.name
            return self.fake_probe_result(
                [
                    command[0],
                    "--db",
                    str(source_db),
                    *command[command.index("--db") + 2 :],
                ]
            )
        if "--contract-id" in command:
            return SimpleNamespace(
                returncode=0,
                stdout=json.dumps({"decoded": {"index": height}}),
                stderr="",
            )
        if "--mpt-state-height" in command:
            return SimpleNamespace(
                returncode=0,
                stdout=json.dumps(
                    {"height": {"decoded": {"current_local_root_index": height}}}
                ),
                stderr="",
            )
        if "--mpt-state-root" in command:
            return SimpleNamespace(
                returncode=0,
                stdout=json.dumps(
                    {"state_root": {"decoded": {"roothash": f"0xroot{height}"}}}
                ),
                stderr="",
            )
        raise AssertionError(f"unexpected probe command: {command}")

    def fast_sync_speed_report(
        self,
        *,
        transaction_blocks: int,
        transaction_elapsed: float | None,
        transaction_bps: float,
        empty_only_blocks: int | None = None,
        empty_elapsed: float | None = None,
        empty_bps: float | None = None,
    ) -> dict:
        import_report = {
            "imported_blocks": 100001,
            "final_height": 100000,
            "elapsed_seconds": 100.0,
            "average_blocks_per_second": 1000.0,
            "empty_blocks": 100001 - transaction_blocks,
            "transaction_blocks": transaction_blocks,
            "transactions": transaction_blocks * 2,
            "transaction_blocks_per_second": transaction_bps,
            "throughput_status": "within-target",
        }
        if transaction_elapsed is not None:
            import_report["transaction_block_import_seconds"] = transaction_elapsed
        if empty_only_blocks is not None:
            import_report["empty_only_blocks"] = empty_only_blocks
        if empty_elapsed is not None:
            import_report["empty_block_import_seconds"] = empty_elapsed
        if empty_bps is not None:
            import_report["empty_blocks_per_second"] = empty_bps
        return {
            "status": "target-reached",
            "sync_source": "fast-sync",
            "target_height": 100000,
            "last_height": 100000,
            "blocks_per_second": 10.0,
            "height_samples": [
                {
                    "elapsed_seconds": 0.0,
                    "height": 0,
                    "metrics": {"neo_sync_native_persist_avg_tx_count": 0.0},
                },
                {
                    "elapsed_seconds": 10000.0,
                    "height": 100000,
                    "metrics": {"neo_sync_native_persist_avg_tx_count": 1.0},
                },
            ],
            "sync_proof": {
                "sync_source": "fast-sync",
                "fast_sync_import": import_report,
                "fast_sync_hot_metrics": {
                    "native_persist_avg_total_us": 3000,
                    "state_service_mpt_avg_total_us": 2000,
                },
                "fast_sync_reference": {
                    "endpoint": "http://seed1.neo.org:10332",
                    "block_height": 100000,
                    "block_hash": "0xblock100000",
                    "state_root_height": 100000,
                    "state_root_hash": "0xroot100000",
                },
            },
            "post_probe": {
                "chain_height": {"ok": True, "height": 100000},
                "stateroot_matches_chain": True,
                "stateroot_height": {"ok": True, "height": 100000},
                "stateroot_root": {
                    "ok": True,
                    "height": 100000,
                    "root": "0xroot100000",
                },
                "reference_stateroot": {
                    "index": 100000,
                    "matches_local": True,
                    "successful_samples": 1,
                    "sample_count": 1,
                },
            },
        }

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
            minimum_checkpoint_count=2,
        )

        self.assertEqual(plan["mode"], "dry-run")
        self.assertEqual(plan["node_bin"], "target/debug/neo-node")
        self.assertEqual(plan["probe_bin"], "target/debug/neo-db-probe")
        self.assertEqual(plan["storage_provider"], "mdbx")
        self.assertIsNone(plan["metrics_url"])
        self.assertEqual(plan["sync_speed_floor_blocks_per_second"], 1500.0)
        self.assertEqual(plan["sync_speed_ceiling_blocks_per_second"], 2000.0)
        self.assertEqual(plan["minimum_transaction_blocks_for_speed_proof"], 1000)
        self.assertEqual(plan["milestones"], [10, 20])
        self.assertEqual(
            plan["checkpoint_plan"],
            {
                "planned_checkpoint_count": 2,
                "minimum_checkpoint_count": 2,
                "minimum_checkpoint_count_met": True,
                "missing_checkpoint_count": 0,
            },
        )
        bounded = plan["steps"][0]["bounded_command"]
        self.assertIn("scripts/run-bounded-mainnet-replay.py", bounded)
        self.assertEqual(
            bounded[bounded.index("--storage-provider") + 1],
            "mdbx",
        )
        self.assertIn("--require-stateroot-height-match", bounded)
        self.assertIn("--require-reference-stateroot-match", bounded)
        self.assertIn("--sync-speed-floor-bps", bounded)
        self.assertIn("1500.0", bounded)
        self.assertIn("--sync-speed-ceiling-bps", bounded)
        self.assertIn("2000.0", bounded)
        self.assertIn("clean/logs/neo-node-milestone-h10.log", bounded)
        checkpoint = plan["steps"][1]["checkpoint_command"]
        self.assertEqual(checkpoint[0], "scripts/checkpoint-on-height.sh")
        self.assertIn("--height", checkpoint)
        self.assertIn("20", checkpoint)
        self.assertEqual(checkpoint[checkpoint.index("--storage-provider") + 1], "mdbx")

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
        self.assertIn("--require-metrics-samples", bounded)

    def test_build_plan_uses_builtin_fast_sync_only_for_first_package_validation(self):
        module = load_module()

        plan = module.build_plan(
            config=Path("clean/neo_mainnet_validate.toml"),
            node_bin=Path("target/debug/neo-node"),
            rpc_url="http://127.0.0.1:21332",
            milestones=[100000, 200000],
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
            fast_sync=True,
            fast_sync_cache=Path("clean/fast-sync-cache"),
            initial_height=0,
            metrics_url="http://127.0.0.1:21990/metrics",
        )

        bounded = plan["steps"][0]["bounded_command"]
        later = plan["steps"][1]["bounded_command"]
        self.assertIn("--fast-sync", bounded)
        self.assertIn("--fast-sync-cache", bounded)
        self.assertIn("clean/fast-sync-cache", bounded)
        self.assertIn("--initial-height", bounded)
        self.assertIn("0", bounded)
        self.assertEqual(bounded[bounded.index("--poll-interval") + 1], "1.0")
        self.assertIn("--metrics-url", bounded)
        self.assertIn("--require-metrics-samples", bounded)
        self.assertNotIn("--fast-sync", later)
        self.assertNotIn("--fast-sync-cache", later)
        self.assertNotIn("--initial-height", later)
        self.assertEqual(later[later.index("--poll-interval") + 1], "5.0")
        self.assertIn("--metrics-url", later)
        self.assertIn("--require-metrics-samples", later)
        self.assertTrue(plan["fast_sync"])
        self.assertEqual(plan["fast_sync_cache"], "clean/fast-sync-cache")
        self.assertEqual(plan["initial_height"], 0)

    def test_fast_sync_first_step_skips_metrics_requirement_when_metrics_are_disabled(self):
        module = load_module()

        plan = module.build_plan(
            config=Path("clean/neo_mainnet_validate.toml"),
            node_bin=Path("target/debug/neo-node"),
            rpc_url="http://127.0.0.1:21332",
            milestones=[100000],
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
            fast_sync=True,
            metrics_url=None,
        )

        bounded = plan["steps"][0]["bounded_command"]
        self.assertIn("--fast-sync", bounded)
        self.assertNotIn("--metrics-url", bounded)
        self.assertNotIn("--require-metrics-samples", bounded)

    def test_build_plan_flags_insufficient_checkpoint_milestones_before_execution(self):
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
            minimum_checkpoint_count=3,
        )

        self.assertEqual(plan["checkpoint_plan"]["planned_checkpoint_count"], 2)
        self.assertEqual(plan["checkpoint_plan"]["minimum_checkpoint_count"], 3)
        self.assertFalse(plan["checkpoint_plan"]["minimum_checkpoint_count_met"])
        self.assertEqual(plan["checkpoint_plan"]["missing_checkpoint_count"], 1)

    def test_main_rejects_fewer_milestones_than_minimum_checkpoint_count(self):
        module = load_module()
        original_argv = module.sys.argv
        try:
            module.sys.argv = [
                "run-stateroot-milestones.py",
                "--config",
                "clean/neo_mainnet_validate.toml",
                "--milestone",
                "10",
                "--milestone",
                "20",
                "--chain-db",
                "clean/chain",
                "--stateroot-db",
                "clean/state-root-334F454E",
                "--checkpoint-root",
                "clean/checkpoints",
                "--log-dir",
                "clean/logs",
            ]

            self.assertEqual(module.main(), 2)
        finally:
            module.sys.argv = original_argv

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

    def test_main_rejects_checkpoint_floor_below_three(self):
        module = load_module()
        original_argv = module.sys.argv
        try:
            module.sys.argv = [
                "run-stateroot-milestones.py",
                "--config",
                "clean/neo_mainnet_validate.toml",
                "--milestone",
                "10",
                "--milestone",
                "20",
                "--milestone",
                "30",
                "--chain-db",
                "clean/chain",
                "--stateroot-db",
                "clean/state-root-334F454E",
                "--checkpoint-root",
                "clean/checkpoints",
                "--log-dir",
                "clean/logs",
                "--minimum-checkpoint-count",
                "2",
            ]
            self.assertEqual(module.main(), 2)
        finally:
            module.sys.argv = original_argv

    def test_main_rejects_sync_speed_floor_below_proof_target(self):
        module = load_module()
        original_argv = module.sys.argv
        try:
            module.sys.argv = [
                "run-stateroot-milestones.py",
                "--config",
                "clean/neo_mainnet_validate.toml",
                "--milestone",
                "10",
                "--milestone",
                "20",
                "--milestone",
                "30",
                "--chain-db",
                "clean/chain",
                "--stateroot-db",
                "clean/state-root-334F454E",
                "--checkpoint-root",
                "clean/checkpoints",
                "--log-dir",
                "clean/logs",
                "--sync-speed-floor-bps",
                "1499.99",
            ]
            self.assertEqual(module.main(), 2)
        finally:
            module.sys.argv = original_argv

    def test_main_rejects_minimum_transaction_blocks_below_proof_target(self):
        module = load_module()
        original_argv = module.sys.argv
        try:
            module.sys.argv = [
                "run-stateroot-milestones.py",
                "--config",
                "clean/neo_mainnet_validate.toml",
                "--milestone",
                "10",
                "--milestone",
                "20",
                "--milestone",
                "30",
                "--chain-db",
                "clean/chain",
                "--stateroot-db",
                "clean/state-root-334F454E",
                "--checkpoint-root",
                "clean/checkpoints",
                "--log-dir",
                "clean/logs",
                "--minimum-transaction-blocks",
                "999",
            ]
            self.assertEqual(module.main(), 2)
        finally:
            module.sys.argv = original_argv

    def test_checkpoint_inventory_requires_restore_verification_metadata(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            checkpoint_root = Path(tmp) / "checkpoints"
            self.create_full_state_checkpoint(checkpoint_root, 10, restore_verified=False)

            inventory = module.checkpoint_inventory(checkpoint_root / "h10")

            self.assertTrue(inventory["exists"])
            self.assertTrue(inventory["has_checkpoint_info"])
            self.assertTrue(inventory["has_chain"])
            self.assertTrue(inventory["has_stateroot"])
            self.assertFalse(inventory["usable_for_state_validation"])
            self.assertIn("restore verification", inventory["reason"])

    def test_checkpoint_inventory_requires_verified_stateroot_root_metadata(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            checkpoint_root = Path(tmp) / "checkpoints"
            checkpoint = checkpoint_root / "h10"
            (checkpoint / "mainnet").mkdir(parents=True)
            (checkpoint / "StateRoot").mkdir()
            (checkpoint / "CHECKPOINT_INFO").write_text(
                "\n".join(
                    [
                        "height=10",
                        "state_root_included=true",
                        "restore_verified=true",
                        "verified_height=10",
                        "verified_against_reference=true",
                        "",
                    ]
                ),
                encoding="utf-8",
            )

            inventory = module.checkpoint_inventory(checkpoint)

            self.assertFalse(inventory["usable_for_state_validation"])
            self.assertIn("verified_stateroot_root", inventory["reason"])

    def test_checkpoint_inventory_uses_checkpoint_storage_provider_metadata(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            checkpoint_root = Path(tmp) / "checkpoints"
            self.create_full_state_checkpoint(
                checkpoint_root,
                10,
                storage_provider="rocksdb",
            )
            providers = []

            def fake_probe(command, **kwargs):
                providers.append(command[command.index("--storage-provider") + 1])
                return self.fake_probe_result(command)

            inventory = module.checkpoint_inventory(
                checkpoint_root / "h10",
                expected_verified_height=10,
                expected_verified_stateroot_root="0xroot10",
                expected_verified_against_reference=True,
                probe_bin=Path("target/debug/neo-db-probe"),
                storage_provider="mdbx",
                probe_runner=fake_probe,
            )

        self.assertTrue(inventory["usable_for_state_validation"])
        self.assertEqual(providers, ["rocksdb", "rocksdb", "rocksdb"])

    def test_checkpoint_inventory_probes_checkpoint_contents_before_counting_usable(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            checkpoint_root = Path(tmp) / "checkpoints"
            self.create_full_state_checkpoint(checkpoint_root, 10)
            calls = []

            def fake_probe(command, **kwargs):
                calls.append(command)
                if "--contract-id" in command:
                    return SimpleNamespace(
                        stdout=json.dumps({"decoded": {"index": 9}}),
                        stderr="",
                    )
                if "--mpt-state-height" in command:
                    return SimpleNamespace(
                        stdout=json.dumps(
                            {
                                "height": {
                                    "decoded": {"current_local_root_index": 10}
                                }
                            }
                        ),
                        stderr="",
                    )
                if "--mpt-state-root" in command:
                    return SimpleNamespace(
                        stdout=json.dumps(
                            {
                                "state_root": {
                                    "decoded": {"roothash": "0xroot10"}
                                }
                            }
                        ),
                        stderr="",
                    )
                raise AssertionError(f"unexpected probe command: {command}")

            inventory = module.checkpoint_inventory(
                checkpoint_root / "h10",
                expected_verified_height=10,
                expected_verified_stateroot_root="0xroot10",
                expected_verified_against_reference=True,
                probe_bin=Path("target/debug/neo-db-probe"),
                probe_runner=fake_probe,
            )

        self.assertFalse(inventory["usable_for_state_validation"])
        self.assertIn("chain database height", inventory["reason"])
        self.assertTrue(calls)

    def test_run_milestones_checkpoints_each_successful_height(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            checkpoint_root = Path(tmp) / "checkpoints"
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
                checkpoint_root=checkpoint_root,
                checkpoint_script=Path("scripts/checkpoint-on-height.sh"),
                log_dir=Path("clean/logs"),
                minimum_checkpoint_count=2,
                sync_speed_floor_bps=2.0,
                sync_speed_ceiling_bps=3.0,
            )
            calls = []

            def fake_run(command, **kwargs):
                calls.append(command)
                probe_result = self.fake_probe_result(command)
                if probe_result is not None:
                    return probe_result
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
                                            "neo_sync_native_persist_avg_tx_count": 0.0,
                                        },
                                    },
                                    {
                                        "elapsed_seconds": 2.0,
                                        "height": 4,
                                        "metrics": {
                                            "neo_sync_avg_persist_us": 200.0,
                                            "neo_state_service_mpt_apply_avg_total_us": 20.0,
                                            "neo_sync_native_persist_avg_tx_count": 1.0,
                                        },
                                    },
                                    {
                                        "elapsed_seconds": 5.0,
                                        "height": 10,
                                        "metrics_error": "metrics unavailable",
                                    },
                                ],
                                "post_probe": {
                                    "chain_height": {"ok": True, "height": int(height)},
                                    "stateroot_matches_chain": True,
                                    "stateroot_height": {"ok": True, "height": int(height)},
                                    "stateroot_root": {
                                        "ok": True,
                                        "height": int(height),
                                        "root": f"0xroot{height}",
                                    },
                                    "reference_stateroot": {
                                        "index": int(height),
                                        "matches_local": True,
                                        "successful_samples": 5,
                                        "sample_count": 5,
                                    },
                                },
                            }
                        ),
                        stderr="",
                    )
                if "scripts/restore-checkpoint.sh" in command:
                    return SimpleNamespace(returncode=0, stdout="restore complete", stderr="")
                height = int(command[command.index("--height") + 1])
                self.create_full_state_checkpoint(
                    checkpoint_root,
                    height,
                    verified_stateroot_root=command[
                        command.index("--verified-stateroot-root") + 1
                    ],
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
            bounded_calls = [
                command
                for command in calls
                if "scripts/run-bounded-mainnet-replay.py" in command
            ]
            checkpoint_calls = [
                command for command in calls if command[0] == "scripts/checkpoint-on-height.sh"
            ]
            probe_calls = [command for command in calls if "neo-db-probe" in str(command[0])]
            self.assertEqual(len(bounded_calls), 2)
            self.assertEqual(len(checkpoint_calls), 2)
            self.assertEqual(len(probe_calls), 18)

    def test_run_milestones_refuses_checkpoint_without_speed_floor_proof(self):
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
            sync_speed_floor_bps=1000.0,
        )
        calls = []

        def fake_run(command, **kwargs):
            calls.append(command)
            return SimpleNamespace(
                returncode=0,
                stdout=json.dumps(
                    {
                        "status": "target-reached",
                        "target_height": 10,
                        "last_height": 10,
                        "blocks_per_second": 999.0,
                        "height_samples": [
                            {"elapsed_seconds": 0.0, "height": 0},
                            {"elapsed_seconds": 1.0, "height": 10},
                        ],
                        "post_probe": {
                            "stateroot_matches_chain": True,
                            "reference_stateroot": {
                                "matches_local": True,
                                "successful_samples": 1,
                                "sample_count": 1,
                            },
                        },
                    }
                ),
                stderr="",
            )

        result = module.run_milestones(plan, runner=fake_run)

        self.assertEqual(result["mode"], "failed")
        self.assertEqual(result["failure"], "speed-proof")
        self.assertEqual(result["failed_height"], 10)
        self.assertIn("below configured floor", result["results"][0]["speed_proof_error"])
        self.assertEqual(len(calls), 1)

    def test_run_milestones_refuses_checkpoint_without_speed_ceiling_proof(self):
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
            sync_speed_floor_bps=1000.0,
            sync_speed_ceiling_bps=2000.0,
        )
        calls = []

        def fake_run(command, **kwargs):
            calls.append(command)
            return SimpleNamespace(
                returncode=0,
                stdout=json.dumps(
                    {
                        "status": "target-reached",
                        "target_height": 10,
                        "last_height": 10,
                        "blocks_per_second": 2500.0,
                        "height_samples": [
                            {"elapsed_seconds": 0.0, "height": 0},
                            {"elapsed_seconds": 0.004, "height": 10},
                        ],
                        "post_probe": {
                            "stateroot_matches_chain": True,
                            "reference_stateroot": {
                                "matches_local": True,
                                "successful_samples": 1,
                                "sample_count": 1,
                            },
                        },
                    }
                ),
                stderr="",
            )

        result = module.run_milestones(plan, runner=fake_run)

        self.assertEqual(result["mode"], "failed")
        self.assertEqual(result["failure"], "speed-proof")
        self.assertEqual(result["failed_height"], 10)
        self.assertIn("above configured ceiling", result["results"][0]["speed_proof_error"])
        self.assertEqual(len(calls), 1)

    def test_run_milestones_rejects_reported_bps_without_matching_height_sample_proof(self):
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
            sync_speed_floor_bps=500.0,
            sync_speed_ceiling_bps=2000.0,
        )
        calls = []

        def fake_run(command, **kwargs):
            calls.append(command)
            return SimpleNamespace(
                returncode=0,
                stdout=json.dumps(
                    {
                        "status": "target-reached",
                        "target_height": 10,
                        "last_height": 10,
                        "blocks_per_second": 1500.0,
                        "height_samples": [
                            {"elapsed_seconds": 0.0, "height": 0},
                            {"elapsed_seconds": 10.0, "height": 10},
                        ],
                        "post_probe": {
                            "chain_height": {"ok": True, "height": 10},
                            "stateroot_matches_chain": True,
                            "stateroot_height": {"ok": True, "height": 10},
                            "stateroot_root": {"root": "0xroot10"},
                            "reference_stateroot": {
                                "index": 10,
                                "matches_local": True,
                                "successful_samples": 1,
                                "sample_count": 1,
                            },
                        },
                    }
                ),
                stderr="",
            )

        result = module.run_milestones(plan, runner=fake_run)

        self.assertEqual(result["mode"], "failed")
        self.assertEqual(result["failure"], "speed-proof")
        self.assertIn("height sample speed below", result["results"][0]["speed_proof_error"])
        self.assertEqual(len(calls), 1)

    def test_run_milestones_requires_height_samples_for_speed_claims(self):
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
            sync_speed_floor_bps=500.0,
        )
        calls = []

        def fake_run(command, **kwargs):
            calls.append(command)
            return SimpleNamespace(
                returncode=0,
                stdout=json.dumps(
                    {
                        "status": "target-reached",
                        "target_height": 10,
                        "last_height": 10,
                        "blocks_per_second": 1500.0,
                        "height_samples": [],
                        "post_probe": {
                            "stateroot_matches_chain": True,
                            "stateroot_root": {"root": "0xroot10"},
                            "reference_stateroot": {
                                "matches_local": True,
                                "successful_samples": 1,
                                "sample_count": 1,
                            },
                        },
                    }
                ),
                stderr="",
            )

        result = module.run_milestones(plan, runner=fake_run)

        self.assertEqual(result["mode"], "failed")
        self.assertEqual(result["failure"], "speed-proof")
        self.assertIn("missing height-sample speed proof", result["results"][0]["speed_proof_error"])
        self.assertEqual(len(calls), 1)

    def test_run_milestones_rejects_speed_claim_without_node_metrics_proof(self):
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
            sync_speed_floor_bps=500.0,
            sync_speed_ceiling_bps=2000.0,
        )
        calls = []

        def fake_run(command, **kwargs):
            calls.append(command)
            return SimpleNamespace(
                returncode=0,
                stdout=json.dumps(
                    {
                        "status": "target-reached",
                        "target_height": 10,
                        "last_height": 10,
                        "blocks_per_second": 1000.0,
                        "height_samples": [
                            {"elapsed_seconds": 0.0, "height": 0},
                            {"elapsed_seconds": 0.01, "height": 10},
                        ],
                        "post_probe": {
                            "chain_height": {"ok": True, "height": 10},
                            "stateroot_matches_chain": True,
                            "stateroot_height": {"ok": True, "height": 10},
                            "stateroot_root": {
                                "ok": True,
                                "height": 10,
                                "root": "0xroot10",
                            },
                            "reference_stateroot": {
                                "index": 10,
                                "matches_local": True,
                                "successful_samples": 1,
                                "sample_count": 1,
                            },
                        },
                    }
                ),
                stderr="",
            )

        result = module.run_milestones(plan, runner=fake_run)

        self.assertEqual(result["mode"], "failed")
        self.assertEqual(result["failure"], "speed-proof")
        self.assertIn("missing node metrics proof", result["results"][0]["speed_proof_error"])
        self.assertEqual(len(calls), 1)

    def test_run_milestones_rejects_empty_only_speed_claim_without_transaction_work(self):
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
            sync_speed_floor_bps=500.0,
        )

        def fake_run(command, **kwargs):
            return SimpleNamespace(
                returncode=0,
                stdout=json.dumps(
                    {
                        "status": "target-reached",
                        "target_height": 10,
                        "last_height": 10,
                        "height_samples": [
                            {
                                "elapsed_seconds": 0.0,
                                "height": 0,
                                "metrics": {
                                    "neo_sync_native_persist_avg_tx_count": 0.0,
                                },
                            },
                            {
                                "elapsed_seconds": 0.01,
                                "height": 10,
                                "metrics": {
                                    "neo_sync_native_persist_avg_tx_count": 0.0,
                                },
                            },
                        ],
                        "post_probe": {
                            "chain_height": {"ok": True, "height": 10},
                            "stateroot_matches_chain": True,
                            "stateroot_height": {"ok": True, "height": 10},
                            "stateroot_root": {"root": "0xroot10"},
                            "reference_stateroot": {
                                "index": 10,
                                "matches_local": True,
                                "successful_samples": 1,
                                "sample_count": 1,
                            },
                        },
                    }
                ),
                stderr="",
            )

        result = module.run_milestones(plan, runner=fake_run)

        self.assertEqual(result["mode"], "failed")
        self.assertEqual(result["failure"], "speed-proof")
        self.assertIn(
            "missing transaction-bearing replay proof",
            result["results"][0]["speed_proof_error"],
        )

    def test_run_milestones_rejects_fast_sync_speed_claim_without_hot_metrics(self):
        module = load_module()
        plan = module.build_plan(
            config=Path("clean/neo_mainnet_validate.toml"),
            node_bin=Path("target/debug/neo-node"),
            rpc_url="http://127.0.0.1:21332",
            milestones=[100000],
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
            fast_sync=True,
            initial_height=0,
            minimum_checkpoint_count=1,
            sync_speed_floor_bps=500.0,
            sync_speed_ceiling_bps=2000.0,
        )

        with tempfile.TemporaryDirectory() as tmp:
            checkpoint_root = Path(tmp) / "checkpoints"
            plan["checkpoint_root"] = str(checkpoint_root)
            for step in plan["steps"]:
                step["checkpoint_command"][
                    step["checkpoint_command"].index("--root") + 1
                ] = str(checkpoint_root)
                step["checkpoint_restore_command"][
                    step["checkpoint_restore_command"].index("--root") + 1
                ] = str(checkpoint_root)

        def fake_run(command, **kwargs):
            probe_result = self.fake_probe_result(command)
            if probe_result is not None:
                return probe_result
            if "scripts/restore-checkpoint.sh" in command:
                return SimpleNamespace(returncode=0, stdout="restore complete", stderr="")
            if command[0] == "scripts/checkpoint-on-height.sh":
                height = int(command[command.index("--height") + 1])
                self.create_full_state_checkpoint(
                    checkpoint_root,
                    height,
                    verified_stateroot_root=command[
                        command.index("--verified-stateroot-root") + 1
                    ],
                )
                return SimpleNamespace(returncode=0, stdout="checkpoint created", stderr="")
            return SimpleNamespace(
                returncode=0,
                stdout=json.dumps(
                    {
                        "status": "target-reached",
                        "sync_source": "fast-sync",
                        "target_height": 100000,
                        "last_height": 100000,
                        "blocks_per_second": 10.0,
                        "height_samples": [
                            {"elapsed_seconds": 0.0, "height": 0},
                            {"elapsed_seconds": 10000.0, "height": 100000},
                        ],
                        "sync_proof": {
                            "sync_source": "fast-sync",
                            "fast_sync_import": {
                                "imported_blocks": 100001,
                                "final_height": 100000,
                                "elapsed_seconds": 100.0,
                                "average_blocks_per_second": 1000.0,
                                "empty_blocks": 98001,
                                "transaction_blocks": 2000,
                                "transactions": 5000,
                                "transaction_block_import_seconds": 4.0,
                                "transaction_blocks_per_second": 500.0,
                                "throughput_status": "within-target",
                            },
                        },
                        "post_probe": {
                            "chain_height": {"ok": True, "height": 100000},
                            "stateroot_matches_chain": True,
                            "stateroot_height": {"ok": True, "height": 100000},
                            "stateroot_root": {
                                "ok": True,
                                "height": 100000,
                                "root": "0xroot100000",
                            },
                            "reference_stateroot": {
                                "index": 100000,
                                "matches_local": True,
                                "successful_samples": 1,
                                "sample_count": 1,
                            },
                        },
                    }
                ),
                stderr="",
            )

        result = module.run_milestones(plan, runner=fake_run)

        self.assertEqual(result["mode"], "failed")
        self.assertEqual(result["failure"], "speed-proof")
        self.assertIn("missing node metrics proof", result["results"][0]["speed_proof_error"])

    def test_run_milestones_rejects_fast_sync_speed_claim_without_reference_provenance(self):
        module = load_module()
        plan = module.build_plan(
            config=Path("clean/neo_mainnet_validate.toml"),
            node_bin=Path("target/debug/neo-node"),
            rpc_url="http://127.0.0.1:21332",
            milestones=[100000],
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
            fast_sync=True,
            initial_height=0,
            minimum_checkpoint_count=1,
            sync_speed_floor_bps=500.0,
            sync_speed_ceiling_bps=2000.0,
        )

        def fake_run(command, **kwargs):
            return SimpleNamespace(
                returncode=0,
                stdout=json.dumps(
                    {
                        "status": "target-reached",
                        "sync_source": "fast-sync",
                        "target_height": 100000,
                        "last_height": 100000,
                        "height_samples": [
                            {"elapsed_seconds": 0.0, "height": 0},
                            {"elapsed_seconds": 10000.0, "height": 100000},
                        ],
                        "sync_proof": {
                            "sync_source": "fast-sync",
                            "fast_sync_import": {
                                "imported_blocks": 100001,
                                "final_height": 100000,
                                "elapsed_seconds": 100.0,
                                "average_blocks_per_second": 1000.0,
                                "empty_blocks": 98001,
                                "transaction_blocks": 2000,
                                "transactions": 5000,
                                "transaction_block_import_seconds": 4.0,
                                "transaction_blocks_per_second": 500.0,
                                "throughput_status": "within-target",
                            },
                            "fast_sync_hot_metrics": {
                                "native_persist_avg_total_us": 3000,
                            },
                        },
                        "post_probe": {
                            "chain_height": {"ok": True, "height": 100000},
                            "stateroot_matches_chain": True,
                            "stateroot_height": {"ok": True, "height": 100000},
                            "stateroot_root": {
                                "ok": True,
                                "height": 100000,
                                "root": "0xroot100000",
                            },
                            "reference_stateroot": {
                                "index": 100000,
                                "matches_local": True,
                                "successful_samples": 1,
                                "sample_count": 1,
                            },
                        },
                    }
                ),
                stderr="",
            )

        result = module.run_milestones(plan, runner=fake_run)

        self.assertEqual(result["mode"], "failed")
        self.assertEqual(result["failure"], "speed-proof")
        self.assertIn(
            "missing fast-sync reference proof",
            result["results"][0]["speed_proof_error"],
        )

    def test_speed_proof_rejects_transaction_bps_without_elapsed_denominator(self):
        module = load_module()
        report = self.fast_sync_speed_report(
            transaction_blocks=2000,
            transaction_elapsed=None,
            transaction_bps=500.0,
        )

        err = module.speed_proof_error(
            report,
            floor_bps=100.0,
            ceiling_bps=2000.0,
            minimum_transaction_blocks=1000,
        )

        self.assertIn("missing transaction-bearing import elapsed", err)

    def test_speed_proof_rejects_transaction_bps_that_does_not_match_elapsed_denominator(self):
        module = load_module()
        report = self.fast_sync_speed_report(
            transaction_blocks=2000,
            transaction_elapsed=4.0,
            transaction_bps=1500.0,
        )

        err = module.speed_proof_error(
            report,
            floor_bps=100.0,
            ceiling_bps=2000.0,
            minimum_transaction_blocks=1000,
        )

        self.assertIn("does not match elapsed proof", err)

    def test_speed_summary_reports_empty_block_bps_separately_from_transaction_proof(self):
        module = load_module()
        report = self.fast_sync_speed_report(
            transaction_blocks=2000,
            transaction_elapsed=2.0,
            transaction_bps=1000.0,
            empty_only_blocks=96000,
            empty_elapsed=8.0,
            empty_bps=12000.0,
        )

        err = module.speed_proof_error(
            report,
            floor_bps=500.0,
            ceiling_bps=2000.0,
            minimum_transaction_blocks=1000,
        )
        summary = module.milestone_summary({"height": 100000, "bounded_report": report})

        self.assertIsNone(err)
        self.assertEqual(summary["speed_proof_source"], "fast-sync-transaction-blocks")
        self.assertEqual(summary["import_window_blocks_per_second"], 1000.0)
        self.assertEqual(summary["empty_block_speed_proof_source"], "fast-sync-empty-blocks")
        self.assertEqual(summary["empty_block_blocks_per_second"], 12000.0)
        self.assertEqual(summary["empty_only_blocks"], 96000)
        self.assertEqual(summary["empty_block_import_seconds"], 8.0)
        self.assertIsNone(summary["empty_block_speed_proof_error"])

    def test_empty_block_bps_does_not_satisfy_transaction_speed_gate(self):
        module = load_module()
        report = self.fast_sync_speed_report(
            transaction_blocks=0,
            transaction_elapsed=0.0,
            transaction_bps=0.0,
            empty_only_blocks=100001,
            empty_elapsed=10.0,
            empty_bps=10000.1,
        )

        err = module.speed_proof_error(
            report,
            floor_bps=500.0,
            ceiling_bps=2000.0,
            minimum_transaction_blocks=1000,
        )
        summary = module.milestone_summary({"height": 100000, "bounded_report": report})

        self.assertIn("no transaction-bearing blocks", err)
        self.assertEqual(summary["empty_block_blocks_per_second"], 10000.1)
        self.assertEqual(summary["import_window_blocks_per_second"], 0.0)

    def test_empty_block_speed_proof_rejects_bps_that_does_not_match_elapsed_denominator(self):
        module = load_module()
        report = self.fast_sync_speed_report(
            transaction_blocks=2000,
            transaction_elapsed=2.0,
            transaction_bps=1000.0,
            empty_only_blocks=96000,
            empty_elapsed=8.0,
            empty_bps=20000.0,
        )

        err = module.empty_block_speed_proof_error(report)
        summary = module.milestone_summary({"height": 100000, "bounded_report": report})

        self.assertIn("empty-block BPS does not match elapsed proof", err)
        self.assertIn(
            "empty-block BPS does not match elapsed proof",
            summary["empty_block_speed_proof_error"],
        )

    def test_run_milestones_uses_fast_sync_import_window_for_speed_proof(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            checkpoint_root = Path(tmp) / "checkpoints"
            plan = module.build_plan(
                config=Path("clean/neo_mainnet_validate.toml"),
                node_bin=Path("target/debug/neo-node"),
                rpc_url="http://127.0.0.1:21332",
                milestones=[100000],
                poll_interval=5.0,
                max_seconds=120.0,
                chain_db=Path("clean/chain"),
                stateroot_db=Path("clean/state-root-334F454E"),
                probe_bin=Path("target/debug/neo-db-probe"),
                references=["http://seed1.neo.org:10332"],
                data_dir=Path("clean"),
                checkpoint_root=checkpoint_root,
                checkpoint_script=Path("scripts/checkpoint-on-height.sh"),
                log_dir=Path("clean/logs"),
                fast_sync=True,
                initial_height=0,
                minimum_checkpoint_count=1,
                sync_speed_floor_bps=100.0,
                sync_speed_ceiling_bps=2000.0,
            )

            def fake_run(command, **kwargs):
                probe_result = self.fake_probe_result(command)
                if probe_result is not None:
                    return probe_result
                if "scripts/run-bounded-mainnet-replay.py" in command:
                    return SimpleNamespace(
                        returncode=0,
                        stdout=json.dumps(
                            {
                                "status": "target-reached",
                                "sync_source": "fast-sync",
                                "target_height": 100000,
                                "last_height": 100000,
                                "blocks_per_second": 10.0,
                                "height_samples": [
                                    {
                                        "elapsed_seconds": 0.0,
                                        "height": 0,
                                        "metrics": {
                                            "neo_sync_native_persist_avg_tx_count": 0.0,
                                        },
                                    },
                                    {
                                        "elapsed_seconds": 10000.0,
                                        "height": 100000,
                                        "metrics": {
                                            "neo_sync_native_persist_avg_tx_count": 1.0,
                                        },
                                    },
                                ],
                                "sync_proof": {
                                    "sync_source": "fast-sync",
                                    "fast_sync_import": {
                                        "imported_blocks": 100001,
                                        "final_height": 100000,
                                        "elapsed_seconds": 100.0,
                                        "average_blocks_per_second": 1000.0,
                                        "empty_blocks": 90001,
                                        "transaction_blocks": 10000,
                                        "transactions": 25000,
                                        "transaction_block_import_seconds": 100.0,
                                        "transaction_blocks_per_second": 100.0,
                                        "throughput_status": "within-target",
                                    },
                                    "fast_sync_hot_metrics": {
                                        "native_persist_avg_total_us": 3000,
                                        "state_service_mpt_avg_total_us": 2000,
                                    },
                                    "fast_sync_reference": {
                                        "endpoint": "http://seed1.neo.org:10332",
                                        "block_height": 100000,
                                        "block_hash": "0xblock100000",
                                        "state_root_height": 100000,
                                        "state_root_hash": "0xroot100000",
                                    },
                                },
                                "post_probe": {
                                    "chain_height": {"ok": True, "height": 100000},
                                    "stateroot_matches_chain": True,
                                    "stateroot_height": {"ok": True, "height": 100000},
                                    "stateroot_root": {
                                        "ok": True,
                                        "height": 100000,
                                        "root": "0xroot100000",
                                    },
                                    "reference_stateroot": {
                                        "index": 100000,
                                        "matches_local": True,
                                        "successful_samples": 1,
                                        "sample_count": 1,
                                    },
                                },
                            }
                        ),
                        stderr="",
                    )
                if "scripts/restore-checkpoint.sh" in command:
                    return SimpleNamespace(returncode=0, stdout="restore complete", stderr="")
                height = int(command[command.index("--height") + 1])
                self.create_full_state_checkpoint(
                    checkpoint_root,
                    height,
                    verified_stateroot_root=command[
                        command.index("--verified-stateroot-root") + 1
                    ],
                )
                return SimpleNamespace(returncode=0, stdout="checkpoint created", stderr="")

            result = module.run_milestones(plan, runner=fake_run)

        self.assertEqual(result["mode"], "completed")
        summary = result["summary"]["milestones"][0]
        self.assertEqual(summary["speed_proof_source"], "fast-sync-transaction-blocks")
        self.assertEqual(summary["import_window_blocks_per_second"], 100.0)
        self.assertNotIn("speed_proof_error", result["results"][0])

    def test_run_milestones_rejects_empty_only_fast_sync_speed_proof(self):
        module = load_module()
        plan = module.build_plan(
            config=Path("clean/neo_mainnet_validate.toml"),
            node_bin=Path("target/debug/neo-node"),
            rpc_url="http://127.0.0.1:21332",
            milestones=[100000],
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
            fast_sync=True,
            initial_height=0,
            minimum_checkpoint_count=1,
            sync_speed_floor_bps=100.0,
            sync_speed_ceiling_bps=2000.0,
        )

        def fake_run(command, **kwargs):
            probe_result = self.fake_probe_result(command)
            if probe_result is not None:
                return probe_result
            return SimpleNamespace(
                returncode=0,
                stdout=json.dumps(
                    {
                        "status": "target-reached",
                        "sync_source": "fast-sync",
                        "target_height": 100000,
                        "last_height": 100000,
                        "blocks_per_second": 10000.0,
                        "height_samples": [
                            {
                                "elapsed_seconds": 0.0,
                                "height": 0,
                                "metrics": {
                                    "neo_sync_native_persist_avg_tx_count": 0.0,
                                },
                            },
                            {
                                "elapsed_seconds": 10.0,
                                "height": 100000,
                                "metrics": {
                                    "neo_sync_native_persist_avg_tx_count": 1.0,
                                },
                            },
                        ],
                        "sync_proof": {
                            "sync_source": "fast-sync",
                            "fast_sync_import": {
                                "imported_blocks": 100001,
                                "final_height": 100000,
                                "elapsed_seconds": 10.0,
                                "average_blocks_per_second": 10000.1,
                                "empty_blocks": 100001,
                                "transaction_blocks": 0,
                                "transactions": 0,
                                "transaction_block_import_seconds": 0.0,
                                "transaction_blocks_per_second": 0.0,
                                "throughput_status": "above-target",
                            },
                            "fast_sync_hot_metrics": {
                                "native_persist_avg_total_us": 3000,
                                "state_service_mpt_avg_total_us": 2000,
                            },
                            "fast_sync_reference": {
                                "endpoint": "http://seed1.neo.org:10332",
                                "block_height": 100000,
                                "block_hash": "0xblock100000",
                                "state_root_height": 100000,
                                "state_root_hash": "0xroot100000",
                            },
                        },
                        "post_probe": {
                            "chain_height": {"ok": True, "height": 100000},
                            "stateroot_matches_chain": True,
                            "stateroot_height": {"ok": True, "height": 100000},
                            "stateroot_root": {
                                "ok": True,
                                "height": 100000,
                                "root": "0xroot100000",
                            },
                            "reference_stateroot": {
                                "index": 100000,
                                "matches_local": True,
                                "successful_samples": 1,
                                "sample_count": 1,
                            },
                        },
                    }
                ),
                stderr="",
            )

        result = module.run_milestones(plan, runner=fake_run)

        self.assertEqual(result["mode"], "failed")
        self.assertEqual(result["failure"], "speed-proof")
        self.assertIn(
            "no transaction-bearing blocks",
            result["results"][0]["speed_proof_error"],
        )

    def test_run_milestones_rejects_tiny_transaction_bearing_speed_sample(self):
        module = load_module()
        plan = module.build_plan(
            config=Path("clean/neo_mainnet_validate.toml"),
            node_bin=Path("target/debug/neo-node"),
            rpc_url="http://127.0.0.1:21332",
            milestones=[100000],
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
            fast_sync=True,
            initial_height=0,
            minimum_checkpoint_count=1,
            sync_speed_floor_bps=100.0,
            sync_speed_ceiling_bps=2000.0,
        )

        def fake_run(command, **kwargs):
            probe_result = self.fake_probe_result(command)
            if probe_result is not None:
                return probe_result
            return SimpleNamespace(
                returncode=0,
                stdout=json.dumps(
                    {
                        "status": "target-reached",
                        "sync_source": "fast-sync",
                        "target_height": 100000,
                        "last_height": 100000,
                        "blocks_per_second": 10000.0,
                        "height_samples": [
                            {
                                "elapsed_seconds": 0.0,
                                "height": 0,
                                "metrics": {
                                    "neo_sync_native_persist_avg_tx_count": 0.0,
                                },
                            },
                            {
                                "elapsed_seconds": 10.0,
                                "height": 100000,
                                "metrics": {
                                    "neo_sync_native_persist_avg_tx_count": 1.0,
                                },
                            },
                        ],
                        "sync_proof": {
                            "sync_source": "fast-sync",
                            "fast_sync_import": {
                                "imported_blocks": 100001,
                                "final_height": 100000,
                                "elapsed_seconds": 10.0,
                                "average_blocks_per_second": 10000.1,
                                "empty_blocks": 99902,
                                "transaction_blocks": 99,
                                "transactions": 99,
                                "transaction_block_import_seconds": 10.0,
                                "transaction_blocks_per_second": 9.9,
                                "throughput_status": "within-target",
                            },
                            "fast_sync_hot_metrics": {
                                "native_persist_avg_total_us": 3000,
                                "state_service_mpt_avg_total_us": 2000,
                            },
                            "fast_sync_reference": {
                                "endpoint": "http://seed1.neo.org:10332",
                                "block_height": 100000,
                                "block_hash": "0xblock100000",
                                "state_root_height": 100000,
                                "state_root_hash": "0xroot100000",
                            },
                        },
                        "post_probe": {
                            "chain_height": {"ok": True, "height": 100000},
                            "stateroot_matches_chain": True,
                            "stateroot_height": {"ok": True, "height": 100000},
                            "stateroot_root": {
                                "ok": True,
                                "height": 100000,
                                "root": "0xroot100000",
                            },
                            "reference_stateroot": {
                                "index": 100000,
                                "matches_local": True,
                                "successful_samples": 1,
                                "sample_count": 1,
                            },
                        },
                    }
                ),
                stderr="",
            )

        result = module.run_milestones(plan, runner=fake_run)

        self.assertEqual(result["mode"], "failed")
        self.assertEqual(result["failure"], "speed-proof")
        self.assertIn(
            "too few transaction-bearing blocks",
            result["results"][0]["speed_proof_error"],
        )

    def test_run_milestones_marks_checkpoint_restore_verified_after_reference_proof(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            checkpoint_root = Path(tmp) / "checkpoints"
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
                checkpoint_root=checkpoint_root,
                checkpoint_script=Path("scripts/checkpoint-on-height.sh"),
                log_dir=Path("clean/logs"),
                minimum_checkpoint_count=1,
                sync_speed_floor_bps=None,
                sync_speed_ceiling_bps=None,
            )
            checkpoint_commands = []

            def fake_run(command, **kwargs):
                probe_result = self.fake_probe_result(command)
                if probe_result is not None:
                    return probe_result
                if "scripts/run-bounded-mainnet-replay.py" in command:
                    return SimpleNamespace(
                        returncode=0,
                        stdout=json.dumps(
                            {
                                "status": "target-reached",
                                "target_height": 10,
                                "last_height": 10,
                                "post_probe": {
                                    "chain_height": {"ok": True, "height": 10},
                                    "stateroot_matches_chain": True,
                                    "stateroot_height": {
                                        "ok": True,
                                        "height": 10,
                                    },
                                    "stateroot_root": {
                                        "ok": True,
                                        "height": 10,
                                        "root": "0xroot10",
                                    },
                                    "reference_stateroot": {
                                        "index": 10,
                                        "matches_local": True,
                                        "successful_samples": 3,
                                        "sample_count": 3,
                                    },
                                },
                            }
                        ),
                        stderr="",
                    )
                if "scripts/restore-checkpoint.sh" in command:
                    return SimpleNamespace(returncode=0, stdout="restore complete", stderr="")

                checkpoint_commands.append(command)
                self.assertIn("--restore-verified", command)
                self.assertEqual(
                    command[command.index("--verified-height") + 1],
                    "10",
                )
                self.assertEqual(
                    command[command.index("--verified-stateroot-root") + 1],
                    "0xroot10",
                )
                self.assertIn("--verified-against-reference", command)
                height = int(command[command.index("--height") + 1])
                self.create_full_state_checkpoint(
                    checkpoint_root,
                    height,
                    verified_stateroot_root=command[
                        command.index("--verified-stateroot-root") + 1
                    ],
                )
                return SimpleNamespace(returncode=0, stdout="checkpoint created", stderr="")

            result = module.run_milestones(plan, runner=fake_run)

            self.assertEqual(result["mode"], "completed")
            self.assertEqual(len(checkpoint_commands), 1)
            self.assertEqual(
                result["results"][0]["checkpoint_command"],
                checkpoint_commands[0],
            )

    def test_run_milestones_restore_probes_checkpoint_before_counting_completed(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            checkpoint_root = Path(tmp) / "checkpoints"
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
                checkpoint_root=checkpoint_root,
                checkpoint_script=Path("scripts/checkpoint-on-height.sh"),
                log_dir=Path("clean/logs"),
                minimum_checkpoint_count=1,
                sync_speed_floor_bps=None,
                sync_speed_ceiling_bps=None,
            )
            restore_commands = []
            scratch_probe_commands = []

            def fake_run(command, **kwargs):
                if "neo-db-probe" in str(command[0]):
                    if ".restore-probe" in str(command[command.index("--db") + 1]):
                        scratch_probe_commands.append(command)
                    return self.fake_probe_result(command)
                if "scripts/run-bounded-mainnet-replay.py" in command:
                    return SimpleNamespace(
                        returncode=0,
                        stdout=json.dumps(
                            {
                                "status": "target-reached",
                                "target_height": 10,
                                "last_height": 10,
                                "post_probe": {
                                    "chain_height": {"ok": True, "height": 10},
                                    "stateroot_matches_chain": True,
                                    "stateroot_height": {"ok": True, "height": 10},
                                    "stateroot_root": {
                                        "ok": True,
                                        "height": 10,
                                        "root": "0xroot10",
                                    },
                                    "reference_stateroot": {
                                        "index": 10,
                                        "matches_local": True,
                                        "successful_samples": 1,
                                        "sample_count": 1,
                                    },
                                },
                            }
                        ),
                        stderr="",
                    )
                if "scripts/restore-checkpoint.sh" in command:
                    restore_commands.append(command)
                    self.assertEqual(command[1], "10")
                    self.assertIn("--root", command)
                    self.assertEqual(
                        command[command.index("--root") + 1],
                        str(checkpoint_root),
                    )
                    self.assertIn("--yes", command)
                    return SimpleNamespace(returncode=0, stdout="restore complete", stderr="")

                height = int(command[command.index("--height") + 1])
                self.create_full_state_checkpoint(
                    checkpoint_root,
                    height,
                    verified_stateroot_root=command[
                        command.index("--verified-stateroot-root") + 1
                    ],
                )
                return SimpleNamespace(returncode=0, stdout="checkpoint created", stderr="")

            result = module.run_milestones(plan, runner=fake_run)

        self.assertEqual(result["mode"], "completed")
        self.assertEqual(result["summary"]["completed_checkpoint_count"], 1)
        self.assertEqual(len(restore_commands), 1)
        self.assertEqual(len(scratch_probe_commands), 3)
        self.assertTrue(result["results"][0]["checkpoint_restore_probe"]["verified"])
        self.assertTrue(
            result["results"][0]["checkpoint_inventory"]["restore_roundtrip_verified"]
        )

    def test_run_milestones_rejects_post_probe_height_above_milestone(self):
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
            minimum_checkpoint_count=1,
            sync_speed_floor_bps=None,
            sync_speed_ceiling_bps=None,
        )
        calls = []

        def fake_run(command, **kwargs):
            calls.append(command)
            return SimpleNamespace(
                returncode=0,
                stdout=json.dumps(
                    {
                        "status": "target-reached",
                        "target_height": 10,
                        "last_height": 11,
                        "post_probe": {
                            "chain_height": {"ok": True, "height": 11},
                            "stateroot_matches_chain": True,
                            "stateroot_height": {"ok": True, "height": 11},
                            "stateroot_root": {
                                "ok": True,
                                "height": 11,
                                "root": "0xroot11",
                            },
                            "reference_stateroot": {
                                "index": 11,
                                "matches_local": True,
                                "successful_samples": 1,
                                "sample_count": 1,
                            },
                        },
                    }
                ),
                stderr="",
            )

        result = module.run_milestones(plan, runner=fake_run)

        self.assertEqual(result["mode"], "failed")
        self.assertEqual(result["failure"], "post-probe")
        self.assertEqual(result["failed_height"], 10)
        self.assertIn("height", result["results"][0]["post_probe_error"])
        self.assertEqual(len(calls), 1)

    def test_run_milestones_rejects_existing_checkpoint_with_wrong_verified_root(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            checkpoint_root = Path(tmp) / "checkpoints"
            self.create_full_state_checkpoint(
                checkpoint_root,
                10,
                verified_stateroot_root="0xstale",
            )
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
                checkpoint_root=checkpoint_root,
                checkpoint_script=Path("scripts/checkpoint-on-height.sh"),
                log_dir=Path("clean/logs"),
                minimum_checkpoint_count=1,
                sync_speed_floor_bps=None,
                sync_speed_ceiling_bps=None,
            )

            def fake_run(command, **kwargs):
                if "scripts/run-bounded-mainnet-replay.py" in command:
                    return SimpleNamespace(
                        returncode=0,
                        stdout=json.dumps(
                            {
                                "status": "target-reached",
                                "target_height": 10,
                                "last_height": 10,
                                "post_probe": {
                                    "chain_height": {"ok": True, "height": 10},
                                    "stateroot_matches_chain": True,
                                    "stateroot_height": {"ok": True, "height": 10},
                                    "stateroot_root": {
                                        "ok": True,
                                        "height": 10,
                                        "root": "0xroot10",
                                    },
                                    "reference_stateroot": {
                                        "index": 10,
                                        "matches_local": True,
                                        "successful_samples": 1,
                                        "sample_count": 1,
                                    },
                                },
                            }
                        ),
                        stderr="",
                    )
                return SimpleNamespace(
                    returncode=0,
                    stdout="checkpoint h10 already exists, skipping",
                    stderr="",
                )

            result = module.run_milestones(plan, runner=fake_run)

        self.assertEqual(result["mode"], "failed")
        self.assertEqual(result["failure"], "checkpoint-inventory")
        self.assertIn("verified_stateroot_root", result["results"][0]["checkpoint_inventory"]["reason"])

    def test_run_milestones_requires_all_reference_samples(self):
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
            references=[
                "http://seed1.neo.org:10332",
                "http://seed2.neo.org:10332",
                "http://seed3.neo.org:10332",
                "http://seed4.neo.org:10332",
                "http://seed5.neo.org:10332",
            ],
            data_dir=Path("clean"),
            checkpoint_root=Path("clean/checkpoints"),
            checkpoint_script=Path("scripts/checkpoint-on-height.sh"),
            log_dir=Path("clean/logs"),
            minimum_checkpoint_count=1,
            sync_speed_floor_bps=None,
            sync_speed_ceiling_bps=None,
        )

        def fake_run(command, **kwargs):
            return SimpleNamespace(
                returncode=0,
                stdout=json.dumps(
                    {
                        "status": "target-reached",
                        "target_height": 10,
                        "last_height": 10,
                        "post_probe": {
                            "chain_height": {"ok": True, "height": 10},
                            "stateroot_matches_chain": True,
                            "stateroot_height": {"ok": True, "height": 10},
                            "stateroot_root": {
                                "ok": True,
                                "height": 10,
                                "root": "0xroot10",
                            },
                            "reference_stateroot": {
                                "index": 10,
                                "matches_local": True,
                                "successful_samples": 1,
                                "sample_count": 5,
                            },
                        },
                    }
                ),
                stderr="",
            )

        result = module.run_milestones(plan, runner=fake_run)

        self.assertEqual(result["mode"], "failed")
        self.assertEqual(result["failure"], "post-probe")
        self.assertIn("reference", result["results"][0]["post_probe_error"])

    def test_run_milestones_fails_when_minimum_checkpoint_count_is_not_met(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            checkpoint_root = Path(tmp) / "checkpoints"
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
                checkpoint_root=checkpoint_root,
                checkpoint_script=Path("scripts/checkpoint-on-height.sh"),
                log_dir=Path("clean/logs"),
                sync_speed_floor_bps=None,
                sync_speed_ceiling_bps=None,
            )

            def fake_run(command, **kwargs):
                probe_result = self.fake_probe_result(command)
                if probe_result is not None:
                    return probe_result
                if "scripts/run-bounded-mainnet-replay.py" in command:
                    height = int(command[command.index("--target-height") + 1])
                    return SimpleNamespace(
                        returncode=0,
                        stdout=json.dumps(
                            {
                                "status": "target-reached",
                                "target_height": height,
                                "last_height": height,
                                "post_probe": {
                                    "chain_height": {"ok": True, "height": height},
                                    "stateroot_matches_chain": True,
                                    "stateroot_height": {"ok": True, "height": height},
                                    "stateroot_root": {
                                        "ok": True,
                                        "height": height,
                                        "root": f"0xroot{height}",
                                    },
                                    "reference_stateroot": {
                                        "index": height,
                                        "matches_local": True,
                                        "successful_samples": 1,
                                        "sample_count": 1,
                                    },
                                },
                            }
                        ),
                        stderr="",
                    )
                if "scripts/restore-checkpoint.sh" in command:
                    return SimpleNamespace(returncode=0, stdout="restore complete", stderr="")
                height = int(command[command.index("--height") + 1])
                self.create_full_state_checkpoint(checkpoint_root, height)
                return SimpleNamespace(returncode=0, stdout="checkpoint created", stderr="")

            result = module.run_milestones(plan, runner=fake_run)

        self.assertEqual(result["mode"], "failed")
        self.assertEqual(result["failure"], "minimum-checkpoints")
        self.assertEqual(result["summary"]["completed_checkpoint_count"], 2)
        self.assertEqual(result["summary"]["minimum_checkpoint_count"], 3)
        self.assertEqual(result["summary"]["missing_checkpoint_count"], 1)

    def test_run_milestones_fails_when_final_retained_inventory_drops_below_minimum(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            checkpoint_root = Path(tmp) / "checkpoints"
            plan = module.build_plan(
                config=Path("clean/neo_mainnet_validate.toml"),
                node_bin=Path("target/debug/neo-node"),
                rpc_url="http://127.0.0.1:21332",
                milestones=[10, 20, 30],
                poll_interval=5.0,
                max_seconds=120.0,
                chain_db=Path("clean/chain"),
                stateroot_db=Path("clean/state-root-334F454E"),
                probe_bin=Path("target/debug/neo-db-probe"),
                references=["http://seed1.neo.org:10332"],
                data_dir=Path("clean"),
                checkpoint_root=checkpoint_root,
                checkpoint_script=Path("scripts/checkpoint-on-height.sh"),
                log_dir=Path("clean/logs"),
                minimum_checkpoint_count=3,
                sync_speed_floor_bps=None,
                sync_speed_ceiling_bps=None,
            )

            def fake_run(command, **kwargs):
                probe_result = self.fake_probe_result(command)
                if probe_result is not None:
                    return probe_result
                if "scripts/run-bounded-mainnet-replay.py" in command:
                    height = int(command[command.index("--target-height") + 1])
                    return SimpleNamespace(
                        returncode=0,
                        stdout=json.dumps(
                            {
                                "status": "target-reached",
                                "target_height": height,
                                "last_height": height,
                                "post_probe": {
                                    "chain_height": {"ok": True, "height": height},
                                    "stateroot_matches_chain": True,
                                    "stateroot_height": {"ok": True, "height": height},
                                    "stateroot_root": {
                                        "ok": True,
                                        "height": height,
                                        "root": f"0xroot{height}",
                                    },
                                    "reference_stateroot": {
                                        "index": height,
                                        "matches_local": True,
                                        "successful_samples": 1,
                                        "sample_count": 1,
                                    },
                                },
                            }
                        ),
                        stderr="",
                    )
                if "scripts/restore-checkpoint.sh" in command:
                    return SimpleNamespace(returncode=0, stdout="restore complete", stderr="")
                height = int(command[command.index("--height") + 1])
                self.create_full_state_checkpoint(checkpoint_root, height)
                if height == 30:
                    import shutil

                    shutil.rmtree(checkpoint_root / "h10")
                return SimpleNamespace(returncode=0, stdout="checkpoint created", stderr="")

            result = module.run_milestones(plan, runner=fake_run)

        self.assertEqual(result["mode"], "failed")
        self.assertEqual(result["failure"], "retained-minimum-checkpoints")
        self.assertEqual(result["summary"]["completed_checkpoint_count"], 3)
        self.assertEqual(result["summary"]["retained_usable_checkpoint_count"], 2)
        self.assertEqual(result["summary"]["retained_missing_checkpoint_count"], 1)
        self.assertFalse(result["summary"]["retained_minimum_checkpoint_count_met"])

    def test_run_milestones_summary_lists_retained_usable_checkpoints(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            checkpoint_root = Path(tmp) / "checkpoints"
            plan = module.build_plan(
                config=Path("clean/neo_mainnet_validate.toml"),
                node_bin=Path("target/debug/neo-node"),
                rpc_url="http://127.0.0.1:21332",
                milestones=[10, 20, 30],
                poll_interval=5.0,
                max_seconds=120.0,
                chain_db=Path("clean/chain"),
                stateroot_db=Path("clean/state-root-334F454E"),
                probe_bin=Path("target/debug/neo-db-probe"),
                references=["http://seed1.neo.org:10332"],
                data_dir=Path("clean"),
                checkpoint_root=checkpoint_root,
                checkpoint_script=Path("scripts/checkpoint-on-height.sh"),
                log_dir=Path("clean/logs"),
                minimum_checkpoint_count=3,
                sync_speed_floor_bps=None,
                sync_speed_ceiling_bps=None,
            )

            def fake_run(command, **kwargs):
                probe_result = self.fake_probe_result(command)
                if probe_result is not None:
                    return probe_result
                if "scripts/run-bounded-mainnet-replay.py" in command:
                    height = int(command[command.index("--target-height") + 1])
                    return SimpleNamespace(
                        returncode=0,
                        stdout=json.dumps(
                            {
                                "status": "target-reached",
                                "target_height": height,
                                "last_height": height,
                                "post_probe": {
                                    "chain_height": {"ok": True, "height": height},
                                    "stateroot_matches_chain": True,
                                    "stateroot_height": {"ok": True, "height": height},
                                    "stateroot_root": {
                                        "ok": True,
                                        "height": height,
                                        "root": f"0xroot{height}",
                                    },
                                    "reference_stateroot": {
                                        "index": height,
                                        "matches_local": True,
                                        "successful_samples": 1,
                                        "sample_count": 1,
                                    },
                                },
                            }
                        ),
                        stderr="",
                    )
                if "scripts/restore-checkpoint.sh" in command:
                    return SimpleNamespace(returncode=0, stdout="restore complete", stderr="")
                height = int(command[command.index("--height") + 1])
                self.create_full_state_checkpoint(
                    checkpoint_root,
                    height,
                    verified_stateroot_root=command[
                        command.index("--verified-stateroot-root") + 1
                    ],
                )
                return SimpleNamespace(returncode=0, stdout="checkpoint created", stderr="")

            result = module.run_milestones(plan, runner=fake_run)

        retained = result["summary"]["retained_usable_checkpoints"]
        self.assertEqual(result["mode"], "completed")
        self.assertEqual(result["summary"]["retained_usable_checkpoint_count"], 3)
        self.assertTrue(result["summary"]["retained_minimum_checkpoint_count_met"])
        self.assertEqual(len(retained), 3)
        self.assertEqual(
            [Path(item["path"]).name for item in retained],
            ["h10", "h20", "h30"],
        )
        self.assertTrue(all(item["usable_for_state_validation"] for item in retained))

    def test_run_milestones_fails_when_final_retained_checkpoint_content_probe_fails(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            checkpoint_root = Path(tmp) / "checkpoints"
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
                checkpoint_root=checkpoint_root,
                checkpoint_script=Path("scripts/checkpoint-on-height.sh"),
                log_dir=Path("clean/logs"),
                minimum_checkpoint_count=1,
                sync_speed_floor_bps=None,
                sync_speed_ceiling_bps=None,
            )
            probe_counts = {"chain": 0}

            def fake_run(command, **kwargs):
                if "neo-db-probe" in str(command[0]):
                    if "--contract-id" in command:
                        probe_counts["chain"] += 1
                        height = 10 if probe_counts["chain"] == 1 else 9
                        return SimpleNamespace(
                            stdout=json.dumps({"decoded": {"format": "hash-index", "index": height}}),
                            stderr="",
                        )
                    if "--mpt-state-height" in command:
                        return SimpleNamespace(
                            stdout=json.dumps(
                                {
                                    "height": {
                                        "decoded": {"current_local_root_index": 10}
                                    }
                                }
                            ),
                            stderr="",
                        )
                    if "--mpt-state-root" in command:
                        return SimpleNamespace(
                            stdout=json.dumps(
                                {
                                    "state_root": {
                                        "decoded": {"roothash": "0xroot10"}
                                    }
                                }
                            ),
                            stderr="",
                        )
                    raise AssertionError(f"unexpected probe command: {command}")
                if "scripts/run-bounded-mainnet-replay.py" in command:
                    return SimpleNamespace(
                        returncode=0,
                        stdout=json.dumps(
                            {
                                "status": "target-reached",
                                "target_height": 10,
                                "last_height": 10,
                                "post_probe": {
                                    "chain_height": {"ok": True, "height": 10},
                                    "stateroot_matches_chain": True,
                                    "stateroot_height": {"ok": True, "height": 10},
                                    "stateroot_root": {
                                        "ok": True,
                                        "height": 10,
                                        "root": "0xroot10",
                                    },
                                    "reference_stateroot": {
                                        "index": 10,
                                        "matches_local": True,
                                        "successful_samples": 1,
                                        "sample_count": 1,
                                    },
                                },
                            }
                        ),
                        stderr="",
                    )
                if "scripts/restore-checkpoint.sh" in command:
                    return SimpleNamespace(returncode=0, stdout="restore complete", stderr="")
                height = int(command[command.index("--height") + 1])
                self.create_full_state_checkpoint(
                    checkpoint_root,
                    height,
                    verified_stateroot_root="0xroot10",
                )
                return SimpleNamespace(returncode=0, stdout="checkpoint created", stderr="")

            result = module.run_milestones(plan, runner=fake_run)

        self.assertEqual(result["mode"], "failed")
        self.assertEqual(result["failure"], "checkpoint-restore-probe")
        self.assertEqual(result["summary"]["completed_checkpoint_count"], 0)
        self.assertEqual(result["summary"]["retained_usable_checkpoint_count"], 0)
        self.assertEqual(result["summary"]["retained_missing_checkpoint_count"], 1)
        self.assertGreaterEqual(probe_counts["chain"], 2)

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

    def test_metrics_sample_summary_ranks_hot_latency_metrics(self):
        module = load_module()

        summary = module.metrics_sample_summary(
            {
                "height_samples": [
                    {
                        "metrics": {
                            "neo_sync_avg_total_us": 4000,
                            'neo_sync_neotoken_committee_compute_stage_avg_us{stage="candidate_state_decode"}': 2100,
                            "neo_state_service_mpt_apply_avg_changes": 17,
                            "neo_state_service_mpt_apply_avg_items": 700,
                        }
                    },
                    {
                        "metrics": {
                            "neo_sync_avg_total_us": 6000,
                            'neo_sync_neotoken_committee_compute_stage_avg_us{stage="candidate_state_decode"}': 3100,
                            "neo_state_service_mpt_apply_avg_changes": 19,
                            "neo_state_service_mpt_apply_avg_items": 900,
                        }
                    },
                ]
            }
        )

        hot = summary["hot_metrics_by_average_us"]
        self.assertEqual(hot[0]["name"], "neo_sync_avg_total_us")
        self.assertEqual(hot[0]["average_us"], 5000.0)
        self.assertEqual(
            hot[1]["name"],
            'neo_sync_neotoken_committee_compute_stage_avg_us{stage="candidate_state_decode"}',
        )
        self.assertEqual(hot[1]["average_us"], 2600.0)
        self.assertNotIn(
            "neo_state_service_mpt_apply_avg_changes",
            [item["name"] for item in hot],
        )
        hot_counts = summary["hot_count_metrics_by_average"]
        self.assertEqual(hot_counts[0]["name"], "neo_state_service_mpt_apply_avg_items")
        self.assertEqual(hot_counts[0]["average"], 800.0)
        self.assertEqual(hot_counts[1]["name"], "neo_state_service_mpt_apply_avg_changes")
        self.assertEqual(hot_counts[1]["average"], 18.0)

    def test_milestone_summary_carries_structured_sync_proof(self):
        module = load_module()

        summary = module.milestone_summary(
            {
                "height": 100000,
                "bounded_report": {
                    "status": "target-reached",
                    "target_height": 100000,
                    "last_height": 100000,
                    "blocks_per_second": 5000.0,
                    "elapsed_seconds": 20.0,
                    "sync_proof": {
                        "sync_source": "fast-sync",
                        "status": "target-reached",
                        "target_height": 100000,
                        "initial_height": 0,
                        "final_height": 100000,
                        "advanced_blocks": 100000,
                        "elapsed_seconds": 20.0,
                        "average_blocks_per_second": 5000.0,
                        "sync_speed_band_met": True,
                        "fast_sync_cache": {
                            "stage": "extracted",
                            "package_path": "chain.0.acc.zip",
                            "package_bytes": 100,
                            "chain_path": "chain.0.acc/chain.0.acc",
                            "chain_bytes": 1000,
                        },
                        "post_probe": {
                            "status_after_post_probe": "target-reached",
                            "stateroot_matches_chain": True,
                            "chain_height": 100000,
                            "stateroot_height": 100000,
                            "local_root": "0xroot100000",
                        },
                    },
                    "post_probe": {
                        "stateroot_matches_chain": True,
                        "stateroot_root": {"root": "0xroot100000"},
                        "reference_stateroot": {
                            "matches_local": True,
                            "successful_samples": 5,
                        },
                    },
                },
                "checkpoint_inventory": {
                    "usable_for_state_validation": True,
                    "path": "clean/checkpoints/h100000",
                },
            }
        )

        proof = summary["sync_proof"]
        self.assertEqual(proof["sync_source"], "fast-sync")
        self.assertEqual(proof["advanced_blocks"], 100000)
        self.assertTrue(proof["sync_speed_band_met"])
        self.assertEqual(proof["fast_sync_cache"]["stage"], "extracted")
        self.assertTrue(proof["post_probe"]["stateroot_matches_chain"])

    def test_run_milestones_can_include_raw_child_output_on_success(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            checkpoint_root = Path(tmp) / "checkpoints"
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
                checkpoint_root=checkpoint_root,
                checkpoint_script=Path("scripts/checkpoint-on-height.sh"),
                log_dir=Path("clean/logs"),
                minimum_checkpoint_count=1,
                sync_speed_floor_bps=None,
                sync_speed_ceiling_bps=None,
            )

            def fake_run(command, **kwargs):
                probe_result = self.fake_probe_result(command)
                if probe_result is not None:
                    return probe_result
                if "scripts/run-bounded-mainnet-replay.py" in command:
                    return SimpleNamespace(
                        returncode=0,
                        stdout=json.dumps(
                            {
                                "status": "target-reached",
                                "target_height": 10,
                                "last_height": 10,
                                "post_probe": {
                                    "chain_height": {"ok": True, "height": 10},
                                    "stateroot_matches_chain": True,
                                    "stateroot_height": {"ok": True, "height": 10},
                                    "stateroot_root": {
                                        "ok": True,
                                        "height": 10,
                                        "root": "0xroot10",
                                    },
                                    "reference_stateroot": {
                                        "index": 10,
                                        "matches_local": True,
                                        "successful_samples": 1,
                                        "sample_count": 1,
                                    },
                                },
                            }
                        ),
                        stderr="",
                    )
                if "scripts/restore-checkpoint.sh" in command:
                    return SimpleNamespace(returncode=0, stdout="restore complete", stderr="")
                self.create_full_state_checkpoint(checkpoint_root, 10)
                return SimpleNamespace(returncode=0, stdout="checkpoint created", stderr="")

            result = module.run_milestones(
                plan,
                runner=fake_run,
                include_command_output=True,
            )

        self.assertEqual(result["mode"], "completed")
        self.assertIn("bounded_stdout", result["results"][0])
        self.assertIn("checkpoint_stdout", result["results"][0])

    def test_run_milestones_refuses_stale_or_wrong_height_bounded_report(self):
        module = load_module()
        plan = module.build_plan(
            config=Path("clean/neo_mainnet_validate.toml"),
            node_bin=Path("target/debug/neo-node"),
            rpc_url="http://127.0.0.1:21332",
            milestones=[100],
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
            minimum_checkpoint_count=1,
            sync_speed_floor_bps=None,
            sync_speed_ceiling_bps=None,
        )
        calls = []

        def fake_run(command, **kwargs):
            calls.append(command)
            return SimpleNamespace(
                returncode=0,
                stdout=json.dumps(
                    {
                        "status": "target-reached",
                        "target_height": 90,
                        "last_height": 90,
                        "post_probe": {
                            "stateroot_matches_chain": True,
                            "stateroot_root": {"root": "0xroot90"},
                            "reference_stateroot": {
                                "matches_local": True,
                                "successful_samples": 1,
                                "sample_count": 1,
                            },
                        },
                    }
                ),
                stderr="",
            )

        result = module.run_milestones(plan, runner=fake_run)

        self.assertEqual(result["mode"], "failed")
        self.assertEqual(result["failure"], "bounded-height-proof")
        self.assertEqual(result["failed_height"], 100)
        self.assertIn(
            "bounded target height mismatch",
            result["results"][0]["bounded_height_proof_error"],
        )
        self.assertEqual(len(calls), 1)

    def test_run_milestones_refuses_checkpoint_without_post_probe_proof(self):
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
            sync_speed_floor_bps=None,
            sync_speed_ceiling_bps=None,
        )
        calls = []

        def fake_run(command, **kwargs):
            calls.append(command)
            return SimpleNamespace(
                returncode=0,
                stdout=json.dumps(
                    {
                        "status": "target-reached",
                        "target_height": 10,
                        "last_height": 10,
                        "post_probe": {
                            "stateroot_matches_chain": False,
                            "reference_stateroot": {"matches_local": True},
                        },
                    }
                ),
                stderr="",
            )

        result = module.run_milestones(plan, runner=fake_run)

        self.assertEqual(result["mode"], "failed")
        self.assertEqual(result["failure"], "post-probe")
        self.assertEqual(result["failed_height"], 10)
        self.assertEqual(result["results"][0]["post_probe_error"], "stateroot-mismatch")
        self.assertEqual(len(calls), 1)

    def test_run_milestones_refuses_chain_only_checkpoint_as_completed(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            checkpoint_root = Path(tmp) / "checkpoints"
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
                checkpoint_root=checkpoint_root,
                checkpoint_script=Path("scripts/checkpoint-on-height.sh"),
                log_dir=Path("clean/logs"),
                minimum_checkpoint_count=1,
                sync_speed_floor_bps=None,
                sync_speed_ceiling_bps=None,
            )

            def fake_run(command, **kwargs):
                if "scripts/run-bounded-mainnet-replay.py" in command:
                    return SimpleNamespace(
                        returncode=0,
                        stdout=json.dumps(
                            {
                                "status": "target-reached",
                                "target_height": 10,
                                "last_height": 10,
                                "post_probe": {
                                    "chain_height": {"ok": True, "height": 10},
                                    "stateroot_matches_chain": True,
                                    "stateroot_height": {"ok": True, "height": 10},
                                    "stateroot_root": {
                                        "ok": True,
                                        "height": 10,
                                        "root": "0xroot10",
                                    },
                                    "reference_stateroot": {
                                        "index": 10,
                                        "matches_local": True,
                                        "successful_samples": 1,
                                        "sample_count": 1,
                                    },
                                },
                            }
                        ),
                        stderr="",
                    )

                checkpoint = checkpoint_root / "h10"
                (checkpoint / "mainnet").mkdir(parents=True)
                (checkpoint / "CHECKPOINT_INFO").write_text(
                    "height=10\nstate_root_included=false\n",
                    encoding="utf-8",
                )
                return SimpleNamespace(returncode=0, stdout="checkpoint created", stderr="")

            result = module.run_milestones(plan, runner=fake_run)

        self.assertEqual(result["mode"], "failed")
        self.assertEqual(result["failure"], "checkpoint-inventory")
        self.assertEqual(result["failed_height"], 10)
        checkpoint_result = result["results"][0]["checkpoint_inventory"]
        self.assertFalse(checkpoint_result["usable_for_state_validation"])
        self.assertTrue(checkpoint_result["has_chain"])
        self.assertFalse(checkpoint_result["has_stateroot"])
        self.assertEqual(result["summary"]["completed_checkpoint_count"], 0)
        self.assertFalse(result["summary"]["minimum_checkpoint_count_met"])

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
