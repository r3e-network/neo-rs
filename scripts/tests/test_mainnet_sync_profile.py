import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


SCRIPTS_DIR = Path(__file__).resolve().parents[1]
PROFILE_PATH = SCRIPTS_DIR / "mainnet_sync_profile.py"
RUNNER_PATH = SCRIPTS_DIR / "run-bounded-mainnet-replay.py"


def load_module(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise ImportError(f"unable to load module from {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def structured_record(message: str, **fields) -> str:
    return json.dumps({"fields": {"message": message, **fields}})


class MainnetSyncProfileTests(unittest.TestCase):
    def test_progress_records_become_ordered_non_cumulative_windows(self):
        module = load_module(PROFILE_PATH, "mainnet_sync_profile_windows")
        text = "\n".join(
            [
                "not structured json",
                structured_record(
                    "chain.acc import progress",
                    imported=1000,
                    transaction_blocks=10,
                    transactions=12,
                    empty_blocks=990,
                    empty_only_blocks=950,
                    elapsed_seconds=2.0,
                    transaction_block_import_seconds=0.5,
                    empty_block_import_seconds=1.0,
                    native_persist_avg_total_us=3000,
                    native_persist_tx_hot_stage="execute",
                    state_service_mpt_trie_commit_avg_us=1200,
                ),
                structured_record(
                    "chain.acc import progress",
                    imported=2500,
                    transaction_blocks=25,
                    transactions=30,
                    empty_blocks=2475,
                    empty_only_blocks=2400,
                    elapsed_seconds=5.0,
                    transaction_block_import_seconds=1.1,
                    empty_block_import_seconds=2.5,
                    native_persist_avg_total_us=4500,
                    native_persist_tx_hot_stage="load_execute",
                    mdbx_commit_window_commit_avg_us=4100,
                    state_service_mpt_trie_commit_avg_us=5200,
                    state_service_mpt_window_end_to_end_total_us=9_000_000_000,
                    state_service_mpt_window_avg_end_to_end_us=900_000,
                    state_service_mpt_window_queue_wait_total_us=8_000_000_000,
                    state_service_mpt_window_queue_wait_avg_us=800_000,
                    state_service_mpt_avg_total_us=700_000,
                ),
                structured_record(
                    "chain.acc MPT mutation profile",
                    imported=2500,
                    state_service_mpt_window_overlay_puts_total=44_500,
                    state_service_mpt_window_overlay_deletes_total=17,
                    state_service_mpt_window_node_puts_total=44_000,
                    state_service_mpt_window_node_deletes_total=3,
                    state_service_mpt_window_node_value_size_0_64_total=11_000,
                    state_service_mpt_window_node_value_bytes_0_64_total=352_000,
                    state_service_mpt_window_put_node_cached_calls_total=45_000,
                    state_service_mpt_window_repeated_ancestor_finalizations_total=12_000,
                ),
                structured_record(
                    "chain.acc MDBX value-size profile",
                    imported=2500,
                    mdbx_commit_window_value_size_0_64_total=1,
                    mdbx_commit_window_value_size_65_128_total=2,
                    mdbx_commit_window_value_size_129_256_total=3,
                    mdbx_commit_window_value_size_257_512_total=4,
                    mdbx_commit_window_value_size_513_1024_total=5,
                    mdbx_commit_window_value_size_1025_4096_total=6,
                    mdbx_commit_window_value_size_4097_16384_total=7,
                    mdbx_commit_window_value_size_over_16384_total=8,
                ),
                structured_record(
                    "chain.acc MDBX cursor-resolve profile",
                    imported=2500,
                    mdbx_commit_window_cursor_resolve_present_total=900,
                    mdbx_commit_window_cursor_resolve_absent_total=44_000,
                    mdbx_commit_window_cursor_resolve_read_bytes_total=4096,
                    mdbx_commit_window_cursor_resolve_write_bytes_total=8192,
                    mdbx_commit_window_cursor_resolve_minor_faults_total=123,
                    mdbx_commit_window_cursor_resolve_major_faults_total=4,
                ),
                structured_record(
                    "chain.acc import complete",
                    imported=2500,
                    final_height=2499,
                    transaction_blocks=25,
                    transactions=30,
                ),
            ]
        )

        profile = module.parse_chain_acc_import_profile(text)
        windows = profile["profile_windows"]

        self.assertEqual([window["index"] for window in windows], [1, 2])
        self.assertEqual([window["imported"] for window in windows], [1000, 1500])
        self.assertEqual(
            [window["transaction_blocks"] for window in windows], [10, 15]
        )
        self.assertEqual([window["transactions"] for window in windows], [12, 18])
        self.assertEqual([window["empty_blocks"] for window in windows], [990, 1485])
        self.assertEqual([window["elapsed_seconds"] for window in windows], [2.0, 3.0])
        self.assertEqual([window["blocks_per_second"] for window in windows], [500.0, 500.0])
        self.assertEqual(
            [window["transaction_blocks_per_second"] for window in windows],
            [20.0, 25.0],
        )
        self.assertEqual(
            [window["empty_blocks_per_second"] for window in windows],
            [990.0, 990.0],
        )
        self.assertEqual(
            windows[1]["hotspots"]["native_persist_tx_hot_stage"],
            "load_execute",
        )
        self.assertNotIn("average_blocks_per_second", windows[1]["hotspots"])
        self.assertEqual(
            windows[1]["hotspots"]["mdbx_commit_window_commit_avg_us"], 4100
        )
        self.assertEqual(
            windows[1]["hotspots"]["state_service_mpt_window_overlay_puts_total"],
            44_500,
        )
        self.assertEqual(
            windows[1]["hotspots"]["state_service_mpt_window_overlay_deletes_total"],
            17,
        )
        self.assertEqual(
            windows[1]["hotspots"]["state_service_mpt_window_node_puts_total"],
            44_000,
        )
        self.assertEqual(
            windows[1]["hotspots"]["state_service_mpt_window_node_deletes_total"],
            3,
        )
        self.assertEqual(
            windows[1]["hotspots"][
                "state_service_mpt_window_node_value_size_0_64_total"
            ],
            11_000,
        )
        self.assertEqual(
            windows[1]["hotspots"][
                "state_service_mpt_window_node_value_bytes_0_64_total"
            ],
            352_000,
        )
        self.assertEqual(
            windows[1]["hotspots"][
                "state_service_mpt_window_put_node_cached_calls_total"
            ],
            45_000,
        )
        self.assertEqual(
            windows[1]["hotspots"][
                "state_service_mpt_window_repeated_ancestor_finalizations_total"
            ],
            12_000,
        )
        self.assertEqual(
            [
                windows[1]["hotspots"][name]
                for name in (
                    "mdbx_commit_window_value_size_0_64_total",
                    "mdbx_commit_window_value_size_65_128_total",
                    "mdbx_commit_window_value_size_129_256_total",
                    "mdbx_commit_window_value_size_257_512_total",
                    "mdbx_commit_window_value_size_513_1024_total",
                    "mdbx_commit_window_value_size_1025_4096_total",
                    "mdbx_commit_window_value_size_4097_16384_total",
                    "mdbx_commit_window_value_size_over_16384_total",
                )
            ],
            list(range(1, 9)),
        )
        self.assertEqual(
            windows[1]["hotspots"][
                "mdbx_commit_window_cursor_resolve_present_total"
            ],
            900,
        )
        self.assertEqual(
            windows[1]["hotspots"][
                "mdbx_commit_window_cursor_resolve_absent_total"
            ],
            44_000,
        )
        self.assertEqual(
            windows[1]["hotspots"][
                "mdbx_commit_window_cursor_resolve_major_faults_total"
            ],
            4,
        )
        self.assertEqual(profile["import_report"]["final_height"], 2499)

        hotspots = profile["profile_hotspots"]
        self.assertEqual(hotspots["window_count"], 2)
        self.assertEqual(hotspots["slowest_window"]["index"], 1)
        self.assertEqual(
            hotspots["top_native_mpt_by_max_us"][0]["name"],
            "state_service_mpt_trie_commit_avg_us",
        )
        ranked_names = {
            timing["name"] for timing in hotspots["top_native_mpt_by_max_us"]
        }
        self.assertNotIn("state_service_mpt_window_end_to_end_total_us", ranked_names)
        self.assertNotIn("state_service_mpt_window_avg_end_to_end_us", ranked_names)
        self.assertNotIn("state_service_mpt_window_queue_wait_total_us", ranked_names)
        self.assertNotIn("state_service_mpt_window_queue_wait_avg_us", ranked_names)
        self.assertNotIn("state_service_mpt_avg_total_us", ranked_names)
        self.assertEqual(
            hotspots["latest_labels"]["native_persist_tx_hot_stage"],
            "load_execute",
        )

    def test_latest_appended_attempt_does_not_cross_cumulative_counters(self):
        module = load_module(PROFILE_PATH, "mainnet_sync_profile_attempts")
        text = "\n".join(
            [
                structured_record(
                    "chain.acc import progress",
                    imported=1000,
                    transaction_blocks=10,
                    transactions=12,
                    empty_blocks=990,
                    elapsed_seconds=2.0,
                    transaction_block_import_seconds=0.5,
                    empty_block_import_seconds=1.0,
                ),
                structured_record(
                    "chain.acc import complete", imported=1000, final_height=999
                ),
                structured_record(
                    "chain.acc import progress",
                    imported=20,
                    transaction_blocks=2,
                    transactions=3,
                    empty_blocks=18,
                    elapsed_seconds=0.2,
                    transaction_block_import_seconds=0.05,
                    empty_block_import_seconds=0.1,
                ),
            ]
        )

        profile = module.parse_chain_acc_import_profile(text)

        self.assertIsNone(profile["import_report"])
        self.assertEqual(len(profile["profile_windows"]), 1)
        self.assertEqual(profile["profile_windows"][0]["from_imported"], 0)
        self.assertEqual(profile["profile_windows"][0]["imported"], 20)
        self.assertEqual(profile["profile_windows"][0]["transactions"], 3)

    def test_extension_record_must_match_latest_progress_sample(self):
        module = load_module(PROFILE_PATH, "mainnet_sync_profile_extensions")
        text = "\n".join(
            [
                structured_record(
                    "chain.acc import progress",
                    imported=100,
                    transaction_blocks=1,
                    transactions=1,
                    empty_blocks=99,
                    elapsed_seconds=1.0,
                    transaction_block_import_seconds=0.1,
                    empty_block_import_seconds=0.8,
                ),
                structured_record(
                    "chain.acc MDBX value-size profile",
                    imported=99,
                    mdbx_commit_window_value_size_0_64_total=123,
                ),
            ]
        )

        profile = module.parse_chain_acc_import_profile(text)

        self.assertNotIn(
            "mdbx_commit_window_value_size_0_64_total",
            profile["profile_windows"][0]["hotspots"],
        )

    def test_targeted_vm_profiles_produce_bounded_script_ranking(self):
        module = load_module(PROFILE_PATH, "mainnet_sync_profile_vm_scripts")
        script_a = "0x" + "11" * 20
        script_b = "0x" + "22" * 20
        text = "\n".join(
            [
                structured_record("importing blocks from chain.acc", count=10),
                structured_record(
                    "targeted VM execution profile",
                    execute_us=500,
                    profiled_instructions=110,
                    protocol="neo-n3-v3.10.1",
                    network_magic=860833102,
                    hardfork_context="HF_Basilisk@1:active",
                    hottest_scripts=(
                        f"{script_a}:bytes=4:instructions=80:contexts=2:"
                        "entries=0x1+3x1:other_entries=0;"
                        f"{script_b}:bytes=8:instructions=20:contexts=1:"
                        "entries=1x1:other_entries=0"
                    ),
                    other_script_instructions=5,
                    other_script_context_loads=1,
                    application_contexts=json.dumps(
                        {
                            "context_capacity": 128,
                            "contexts": [
                                {
                                    "raw_script_hash": script_a,
                                    "raw_script_bytes": 4,
                                    "entry_offset": 0,
                                    "logical_script_hash": "0x" + "44" * 20,
                                    "contract_id": 17,
                                    "contract_update_counter": 2,
                                    "nef_checksum": 1234,
                                    "manifest_name": "HotContract",
                                    "method": "transfer",
                                    "argument_count": 4,
                                    "parameter_types": ["Hash160", "Integer"],
                                    "return_type": "Boolean",
                                    "call_flags": 15,
                                    "dynamic_call": True,
                                    "context_loads": 2,
                                }
                            ],
                            "other_context_loads": 3,
                        }
                    ),
                ),
                structured_record(
                    "targeted VM execution profile",
                    execute_us=700,
                    profiled_instructions=100,
                    protocol="neo-n3-v3.10.1",
                    network_magic=860833102,
                    hardfork_context="HF_Basilisk@1:active",
                    hottest_scripts=(
                        f"{script_a}:bytes=4:instructions=90:contexts=1:"
                        "entries=0x1:other_entries=2"
                    ),
                    other_script_instructions=0,
                    other_script_context_loads=0,
                    application_contexts=json.dumps(
                        {
                            "context_capacity": 128,
                            "contexts": [
                                {
                                    "raw_script_hash": script_a,
                                    "raw_script_bytes": 4,
                                    "entry_offset": 0,
                                    "logical_script_hash": "0x" + "44" * 20,
                                    "contract_id": 17,
                                    "contract_update_counter": 2,
                                    "nef_checksum": 1234,
                                    "manifest_name": "HotContract",
                                    "method": "transfer",
                                    "argument_count": 4,
                                    "parameter_types": ["Hash160", "Integer"],
                                    "return_type": "Boolean",
                                    "call_flags": 15,
                                    "dynamic_call": True,
                                    "context_loads": 1,
                                }
                            ],
                            "other_context_loads": 0,
                        }
                    ),
                ),
                structured_record(
                    "targeted VM execution profile",
                    execute_us=1,
                    profiled_instructions=1,
                    hottest_scripts="malformed",
                    other_script_instructions=0,
                    other_script_context_loads=0,
                ),
                structured_record(
                    "chain.acc import progress",
                    imported=10,
                    transaction_blocks=2,
                    transactions=3,
                    empty_blocks=8,
                    elapsed_seconds=1.0,
                    transaction_block_import_seconds=0.2,
                    empty_block_import_seconds=0.1,
                ),
                structured_record("chain.acc import complete", imported=10),
            ]
        )

        result = module.parse_chain_acc_import_profile(text)["vm_script_profile"]

        self.assertEqual(result["transaction_count"], 3)
        self.assertEqual(result["execute_us_total"], 1201)
        self.assertEqual(result["profiled_instructions_total"], 211)
        self.assertEqual(result["collector_overflow_instructions"], 5)
        self.assertEqual(result["unreported_retained_script_instructions"], 16)
        self.assertEqual(result["malformed_script_records"], 1)
        self.assertEqual(result["malformed_application_context_profiles"], 0)
        self.assertEqual(result["application_context_other_loads"], 3)
        self.assertEqual(result["ranked_script_count"], 2)
        self.assertEqual(result["scripts"][0]["script_hash"], script_a)
        self.assertEqual(result["scripts"][0]["instructions"], 170)
        self.assertEqual(result["scripts"][0]["contexts"], 3)
        self.assertEqual(result["scripts"][0]["transaction_appearances"], 2)
        self.assertEqual(result["scripts"][0]["inclusive_execute_us"], 1200)
        self.assertEqual(result["scripts"][0]["other_entry_context_loads"], 2)
        self.assertEqual(
            result["scripts"][0]["logical_contexts"][0]["logical_script_hash"],
            "0x" + "44" * 20,
        )
        self.assertEqual(
            result["scripts"][0]["logical_contexts"][0]["context_loads"], 3
        )
        self.assertEqual(
            result["scripts"][0]["logical_contexts"][0]["method"], "transfer"
        )
        self.assertEqual(
            result["scripts"][0]["entry_points"],
            [
                {"entry_offset": 0, "context_loads": 2},
                {"entry_offset": 3, "context_loads": 1},
            ],
        )
        self.assertEqual(result["protocols"][0]["count"], 2)

    def test_bounded_report_attaches_profile_windows_and_hotspot_summary(self):
        runner = load_module(RUNNER_PATH, "bounded_replay_profile_wiring")
        report = {
            "status": "target-reached",
            "sync_source": "import-chain",
            "target_height": 99,
            "last_height": 99,
            "height_samples": [],
        }
        log = "\n".join(
            [
                structured_record(
                    "targeted VM execution profile",
                    execute_us=250,
                    profiled_instructions=3,
                    protocol="neo-n3-v3.10.1",
                    network_magic=860833102,
                    hardfork_context="HF_Basilisk@1:active",
                    hottest_scripts=(
                        f"{'0x' + '33' * 20}:bytes=4:instructions=3:contexts=1:"
                        "entries=0x1:other_entries=0"
                    ),
                    other_script_instructions=0,
                    other_script_context_loads=0,
                ),
                structured_record(
                    "chain.acc import progress",
                    imported=100,
                    transaction_blocks=4,
                    transactions=5,
                    empty_blocks=96,
                    elapsed_seconds=0.5,
                    transaction_block_import_seconds=0.1,
                    empty_block_import_seconds=0.3,
                    native_persist_avg_total_us=2400,
                    state_service_mpt_avg_total_us=1800,
                ),
                structured_record(
                    "chain.acc import complete",
                    imported=100,
                    final_height=99,
                    transaction_blocks=4,
                    transactions=5,
                    transaction_blocks_per_second=40.0,
                ),
            ]
        )

        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "neo-node.log"
            path.write_text(log + "\n", encoding="utf-8")
            updated = runner.attach_chain_acc_import_report(report, path)

        self.assertEqual(updated["profile_windows"][0]["imported"], 100)
        self.assertEqual(updated["profile_hotspots"]["window_count"], 1)
        self.assertEqual(
            updated["profile_hotspots"]["top_native_mpt_by_max_us"][0]["name"],
            "native_persist_avg_total_us",
        )
        self.assertEqual(updated["chain_acc_import_report"]["final_height"], 99)
        self.assertEqual(updated["vm_script_profile"]["transaction_count"], 1)
        self.assertEqual(updated["vm_script_profile"]["scripts"][0]["instructions"], 3)
        self.assertEqual(
            updated["transaction_work_summary"]["transactions"], 5
        )

    def test_bounded_report_uses_transaction_bps_for_speed_gate(self):
        runner = load_module(RUNNER_PATH, "bounded_replay_transaction_speed")
        report = {
            "status": "transaction-work-unproven",
            "sync_source": "import-chain",
            "target_height": 20000,
            "last_height": 20000,
            "elapsed_seconds": 11.0,
            "blocks_per_second": 1800.0,
            "height_samples": [
                {"elapsed_seconds": 0.0, "height": 0},
                {"elapsed_seconds": 11.0, "height": 20000},
            ],
            "sync_speed_floor_blocks_per_second": 1500.0,
            "sync_speed_ceiling_blocks_per_second": None,
            "sync_speed_band_met": True,
            "sync_speed_shortfall_blocks_per_second": 0.0,
            "sync_speed_overage_blocks_per_second": 0.0,
            "transaction_work_summary": {
                "required_for_speed_proof": True,
                "observed_transaction_work": False,
                "metric_count": 0,
                "metrics": [],
            },
        }
        log = structured_record(
            "chain.acc import complete",
            imported=20001,
            final_height=20000,
            average_blocks_per_second=1946.3,
            transaction_blocks=141,
            transactions=145,
            transaction_blocks_per_second=978.9,
        )

        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "neo-node.log"
            path.write_text(log + "\n", encoding="utf-8")
            updated = runner.attach_chain_acc_import_report(report, path)

        self.assertEqual(updated["status"], "sync-speed-too-slow")
        self.assertEqual(
            updated["sync_speed_measurement_source"],
            "import-chain-transaction-blocks",
        )
        self.assertEqual(updated["sync_speed_measured_blocks_per_second"], 978.9)
        self.assertGreater(updated["sync_speed_shortfall_blocks_per_second"], 0.0)
        self.assertFalse(updated["sync_speed_band_met"])
        self.assertTrue(updated["transaction_work_summary"]["observed_transaction_work"])
        self.assertEqual(
            updated["sync_proof"]["chain_acc_import"]["transaction_blocks"],
            141,
        )

    def test_bounded_report_derives_final_height_from_imported(self):
        runner = load_module(RUNNER_PATH, "bounded_replay_derived_final_height")
        report = {
            "status": "transaction-work-unproven",
            "sync_source": "import-chain",
            "target_height": 30000,
            "last_height": 30000,
            "elapsed_seconds": 3.0,
            "blocks_per_second": 10000.0,
            "height_samples": [
                {"elapsed_seconds": 0.0, "height": 1000},
                {"elapsed_seconds": 3.0, "height": 30000},
            ],
            "sync_speed_floor_blocks_per_second": 1500.0,
            "sync_speed_ceiling_blocks_per_second": None,
            "sync_speed_band_met": True,
            "sync_speed_shortfall_blocks_per_second": 0.0,
            "sync_speed_overage_blocks_per_second": 0.0,
        }
        log = structured_record(
            "chain.acc import complete",
            imported=29000,
            transaction_blocks=160,
            transactions=166,
            transaction_blocks_per_second=6800.0,
        )

        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "neo-node.log"
            path.write_text(log + "\n", encoding="utf-8")
            updated = runner.attach_chain_acc_import_report(report, path)

        self.assertEqual(updated["status"], "target-reached")
        self.assertEqual(
            updated["sync_speed_measurement_source"],
            "import-chain-transaction-blocks",
        )
        self.assertEqual(updated["sync_speed_measured_blocks_per_second"], 6800.0)
        self.assertTrue(updated["transaction_work_summary"]["observed_transaction_work"])


if __name__ == "__main__":
    unittest.main()
