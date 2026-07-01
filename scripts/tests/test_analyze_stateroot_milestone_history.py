import importlib.util
import contextlib
import io
import json
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).resolve().parents[1] / "analyze-stateroot-milestone-history.py"


def load_module():
    spec = importlib.util.spec_from_file_location("analyze_stateroot_milestone_history", MODULE_PATH)
    if spec is None or spec.loader is None:
        raise ImportError(f"unable to load module from {MODULE_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def record(timestamp, milestones, **extra):
    payload = {
        "timestamp_utc": timestamp,
        "mode": "completed",
        "summary": {
            "milestones": milestones,
        },
    }
    payload.update(extra)
    return payload


def write_history(path: Path, records):
    path.write_text(
        "".join(json.dumps(item) + "\n" for item in records),
        encoding="utf-8",
    )


def create_restore_verified_checkpoint(checkpoint_root: Path, height: int) -> None:
    checkpoint = checkpoint_root / f"h{height}"
    (checkpoint / "mainnet").mkdir(parents=True)
    (checkpoint / "StateRoot").mkdir()
    (checkpoint / "CHECKPOINT_INFO").write_text(
        "\n".join(
            [
                f"height={height}",
                "state_root_included=true",
                "restore_verified=true",
                f"verified_height={height}",
                f"verified_stateroot_root=0x{height}",
                "verified_against_reference=true",
                "",
            ]
        ),
        encoding="utf-8",
    )


class AnalyzeStateRootMilestoneHistoryTests(unittest.TestCase):
    def test_load_history_reads_jsonl_records(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "history.jsonl"
            path.write_text(
                json.dumps(record("2026-01-01T00:00:00+00:00", [])) + "\n\n",
                encoding="utf-8",
            )

            records = module.load_history(path)

        self.assertEqual(len(records), 1)
        self.assertEqual(records[0]["mode"], "completed")

    def test_analyze_history_reports_perf_and_consistency_summary(self):
        module = load_module()
        records = [
            record(
                "2026-01-01T00:00:00+00:00",
                [
                    {
                        "height": 100,
                        "last_height": 100,
                        "blocks_per_second": 10.0,
                        "elapsed_seconds": 5.0,
                        "local_root": "0x100",
                        "reference_matches_local": True,
                        "stateroot_matches_chain": True,
                        "checkpoint_created": True,
                        "successful_reference_samples": 5,
                    },
                    {
                        "height": 200,
                        "last_height": 200,
                        "blocks_per_second": 2.0,
                        "elapsed_seconds": 6.0,
                        "local_root": "0x200",
                        "reference_matches_local": False,
                        "stateroot_matches_chain": True,
                        "checkpoint_created": False,
                        "successful_reference_samples": 4,
                    },
                ],
            ),
            record(
                "2026-01-01T00:10:00+00:00",
                [
                    {
                        "height": 500,
                        "last_height": 500,
                        "blocks_per_second": 20.0,
                        "elapsed_seconds": 5.0,
                        "local_root": "0x500",
                        "reference_matches_local": True,
                        "stateroot_matches_chain": True,
                        "checkpoint_created": True,
                        "successful_reference_samples": 5,
                        "metrics_sample_summary": {
                            "metrics": {
                                "neo_sync_avg_persist_us": {
                                    "last": 4406.0,
                                    "sample_count": 2,
                                }
                            }
                        },
                        "transaction_work_summary": {
                            "observed_transaction_work": True,
                            "max_avg_tx_count": 2.0,
                        },
                    }
                ],
            ),
        ]

        report = module.analyze_history(
            records,
            slowest_limit=2,
            fastest_limit=1,
        )

        self.assertEqual(report["run_count"], 2)
        self.assertEqual(report["milestone_count"], 3)
        self.assertEqual(report["completed_checkpoint_count"], 2)
        self.assertEqual(report["latest_height"], 500)
        self.assertEqual(report["latest_root"], "0x500")
        self.assertEqual(
            report["latest_metrics_sample_summary"]["metrics"]["neo_sync_avg_persist_us"]["last"],
            4406.0,
        )
        self.assertTrue(
            report["latest_transaction_work_summary"]["observed_transaction_work"]
        )
        self.assertEqual(report["average_blocks_per_second"], (10.0 + 2.0 + 20.0) / 3)
        self.assertEqual(report["slowest_milestones"][0]["height"], 200)
        self.assertEqual(report["fastest_milestones"][0]["height"], 500)
        self.assertEqual(report["reference_mismatch_count"], 1)
        self.assertEqual(report["state_mismatch_count"], 0)
        self.assertEqual(report["throughput_regression_count"], 0)

    def test_analyze_history_reports_adjacent_throughput_regressions(self):
        module = load_module()
        records = [
            record(
                "2026-01-01T00:00:00+00:00",
                [
                    {
                        "height": 100,
                        "last_height": 100,
                        "blocks_per_second": 100.0,
                        "checkpoint_created": True,
                        "local_root": "0x100",
                    },
                    {
                        "height": 200,
                        "last_height": 200,
                        "blocks_per_second": 60.0,
                        "checkpoint_created": True,
                        "local_root": "0x200",
                    },
                ],
            ),
            record(
                "2026-01-01T00:10:00+00:00",
                [
                    {
                        "height": 300,
                        "last_height": 300,
                        "blocks_per_second": 90.0,
                        "checkpoint_created": True,
                        "local_root": "0x300",
                    }
                ],
            ),
        ]

        report = module.analyze_history(
            records,
            slowest_limit=5,
            fastest_limit=5,
            regression_threshold_percent=25.0,
        )

        self.assertEqual([item["height"] for item in report["throughput_trend"]], [100, 200, 300])
        self.assertIsNone(report["throughput_trend"][0]["previous_height"])
        self.assertEqual(report["throughput_trend"][1]["previous_height"], 100)
        self.assertEqual(report["throughput_trend"][1]["height_delta"], 100)
        self.assertEqual(report["throughput_trend"][1]["change_percent"], -40.0)
        self.assertTrue(report["throughput_trend"][1]["regression"])
        self.assertEqual(report["throughput_regression_count"], 1)
        self.assertEqual(report["throughput_regressions"][0]["height"], 200)

    def test_analyze_history_reports_sync_speed_floor_shortfalls_against_default_floor(self):
        module = load_module()
        records = [
            record(
                "2026-01-01T00:00:00+00:00",
                [
                    {
                        "height": 100,
                        "last_height": 100,
                        "blocks_per_second": 1499.99,
                        "checkpoint_created": True,
                        "local_root": "0x100",
                    },
                    {
                        "height": 200,
                        "last_height": 200,
                        "blocks_per_second": 1500.0,
                        "checkpoint_created": True,
                        "local_root": "0x200",
                    },
                    {
                        "height": 300,
                        "last_height": 300,
                        "blocks_per_second": 2000.0,
                        "checkpoint_created": True,
                        "local_root": "0x300",
                    },
                ],
                node_bin="target/release/neo-node",
            )
        ]

        report = module.analyze_history(
            records,
            slowest_limit=5,
            fastest_limit=5,
        )

        self.assertEqual(report["sync_speed_floor_blocks_per_second"], 1500.0)
        self.assertEqual(report["throughput_floor_violation_count"], 1)
        self.assertEqual(report["throughput_floor_violations"][0]["height"], 100)
        self.assertEqual(
            report["throughput_floor_violations"][0]["blocks_per_second"],
            1499.99,
        )
        self.assertAlmostEqual(
            report["throughput_floor_violations"][0]["shortfall_blocks_per_second"],
            0.01,
            places=6,
        )

    def test_analyze_history_reports_transaction_import_speed_proof_floor_shortfalls(self):
        module = load_module()
        records = [
            record(
                "2026-01-01T00:00:00+00:00",
                [
                    {
                        "height": 100,
                        "last_height": 100,
                        "blocks_per_second": 10000.0,
                        "checkpoint_created": True,
                        "local_root": "0x100",
                        "speed_proof_source": "fast-sync-transaction-blocks",
                        "import_window_blocks_per_second": 1499.5,
                        "replay_window_blocks_per_second": 10.0,
                        "sync_proof": {
                            "fast_sync_import": {
                                "transaction_blocks": 2000,
                                "transactions": 5000,
                                "transaction_block_import_seconds": 1.333778,
                                "transaction_blocks_per_second": 1499.5,
                            }
                        },
                    },
                    {
                        "height": 200,
                        "last_height": 200,
                        "blocks_per_second": 10000.0,
                        "checkpoint_created": True,
                        "local_root": "0x200",
                        "speed_proof_source": "fast-sync-transaction-blocks",
                        "import_window_blocks_per_second": 1750.0,
                        "replay_window_blocks_per_second": 10.0,
                        "sync_proof": {
                            "fast_sync_import": {
                                "transaction_blocks": 3000,
                                "transactions": 6000,
                                "transaction_block_import_seconds": 1.714286,
                                "transaction_blocks_per_second": 1750.0,
                            }
                        },
                    },
                    {
                        "height": 300,
                        "last_height": 300,
                        "blocks_per_second": 50.0,
                        "checkpoint_created": True,
                        "local_root": "0x300",
                        "speed_proof_source": "height-samples",
                        "import_window_blocks_per_second": None,
                    },
                ],
                node_bin="target/release/neo-node",
            )
        ]

        report = module.analyze_history(
            records,
            slowest_limit=5,
            fastest_limit=5,
        )

        self.assertEqual(report["transaction_import_proof_count"], 2)
        self.assertEqual(report["transaction_import_speed_floor_blocks_per_second"], 1500.0)
        self.assertEqual(
            report["average_transaction_import_blocks_per_second"],
            (1499.5 + 1750.0) / 2,
        )
        self.assertEqual(report["min_transaction_import_blocks_per_second"], 1499.5)
        self.assertEqual(report["max_transaction_import_blocks_per_second"], 1750.0)
        self.assertEqual(report["transaction_import_floor_violation_count"], 1)
        violation = report["transaction_import_floor_violations"][0]
        self.assertEqual(violation["height"], 100)
        self.assertEqual(violation["transaction_import_blocks_per_second"], 1499.5)
        self.assertEqual(violation["transaction_blocks"], 2000)
        self.assertAlmostEqual(violation["shortfall_blocks_per_second"], 0.5)
        self.assertEqual(report["slowest_transaction_import_milestones"][0]["height"], 100)
        self.assertEqual(report["fastest_transaction_import_milestones"][0]["height"], 200)

    def test_analyze_history_reports_empty_block_fast_path_proofs_separately(self):
        module = load_module()
        records = [
            record(
                "2026-01-01T00:00:00+00:00",
                [
                    {
                        "height": 100,
                        "last_height": 100,
                        "blocks_per_second": 10000.0,
                        "checkpoint_created": True,
                        "local_root": "0x100",
                        "speed_proof_source": "fast-sync-transaction-blocks",
                        "import_window_blocks_per_second": 1600.0,
                        "empty_block_speed_proof_source": "fast-sync-empty-blocks",
                        "empty_block_blocks_per_second": 12000.0,
                        "empty_only_blocks": 96000,
                        "empty_block_import_seconds": 8.0,
                    },
                    {
                        "height": 200,
                        "last_height": 200,
                        "blocks_per_second": 9000.0,
                        "checkpoint_created": True,
                        "local_root": "0x200",
                        "speed_proof_source": "fast-sync-transaction-blocks",
                        "import_window_blocks_per_second": 1700.0,
                        "empty_block_speed_proof_source": "fast-sync-empty-blocks",
                        "empty_block_blocks_per_second": 9000.0,
                        "empty_only_blocks": 45000,
                        "empty_block_import_seconds": 5.0,
                        "empty_block_speed_proof_error": "fast-sync empty-block BPS does not match elapsed proof",
                    },
                ],
            )
        ]

        report = module.analyze_history(
            records,
            slowest_limit=5,
            fastest_limit=1,
        )

        self.assertEqual(report["empty_block_fast_path_proof_count"], 2)
        self.assertEqual(report["average_empty_block_blocks_per_second"], 10500.0)
        self.assertEqual(report["min_empty_block_blocks_per_second"], 9000.0)
        self.assertEqual(report["max_empty_block_blocks_per_second"], 12000.0)
        self.assertEqual(report["slowest_empty_block_milestones"][0]["height"], 200)
        self.assertEqual(report["fastest_empty_block_milestones"][0]["height"], 100)
        self.assertEqual(report["fastest_empty_block_milestones"][0]["empty_only_blocks"], 96000)
        self.assertEqual(report["empty_block_speed_proof_error_count"], 1)
        self.assertEqual(report["empty_block_speed_proof_errors"][0]["height"], 200)

    def test_analyze_history_treats_empty_block_fast_path_floor_as_optional(self):
        module = load_module()
        records = [
            record(
                "2026-01-01T00:00:00+00:00",
                [
                    {
                        "height": 100,
                        "last_height": 100,
                        "blocks_per_second": 14000.0,
                        "checkpoint_created": True,
                        "local_root": "0x100",
                        "speed_proof_source": "fast-sync-transaction-blocks",
                        "import_window_blocks_per_second": 1600.0,
                        "empty_block_speed_proof_source": "fast-sync-empty-blocks",
                        "empty_block_blocks_per_second": 12000.0,
                        "empty_only_blocks": 96000,
                        "empty_block_import_seconds": 8.0,
                    },
                    {
                        "height": 200,
                        "last_height": 200,
                        "blocks_per_second": 9000.0,
                        "checkpoint_created": True,
                        "local_root": "0x200",
                        "speed_proof_source": "fast-sync-transaction-blocks",
                        "import_window_blocks_per_second": 1700.0,
                        "empty_block_speed_proof_source": "fast-sync-empty-blocks",
                        "empty_block_blocks_per_second": 9500.0,
                        "empty_only_blocks": 47500,
                        "empty_block_import_seconds": 5.0,
                    },
                ],
            )
        ]

        report = module.analyze_history(
            records,
            slowest_limit=5,
            fastest_limit=5,
        )

        self.assertIsNone(report["empty_block_speed_floor_blocks_per_second"])
        self.assertEqual(report["empty_block_speed_floor_violation_count"], 0)
        self.assertEqual(report["empty_block_speed_floor_violations"], [])

        report = module.analyze_history(
            records,
            slowest_limit=5,
            fastest_limit=5,
            empty_block_speed_floor_bps=10000.0,
        )

        self.assertEqual(report["empty_block_speed_floor_blocks_per_second"], 10000.0)
        self.assertEqual(report["empty_block_speed_floor_violation_count"], 1)
        violation = report["empty_block_speed_floor_violations"][0]
        self.assertEqual(violation["height"], 200)
        self.assertEqual(violation["empty_block_blocks_per_second"], 9500.0)
        self.assertEqual(violation["shortfall_blocks_per_second"], 500.0)
        self.assertEqual(violation["transaction_import_blocks_per_second"], 1700.0)

    def test_analyze_history_groups_performance_by_node_binary(self):
        module = load_module()
        records = [
            record(
                "2026-01-01T00:00:00+00:00",
                [
                    {
                        "height": 100,
                        "last_height": 100,
                        "blocks_per_second": 50.0,
                        "checkpoint_created": True,
                        "local_root": "0x100",
                    }
                ],
            ),
            record(
                "2026-01-01T00:10:00+00:00",
                [
                    {
                        "height": 200,
                        "last_height": 200,
                        "blocks_per_second": 200.0,
                        "checkpoint_created": True,
                        "local_root": "0x200",
                    },
                    {
                        "height": 300,
                        "last_height": 300,
                        "blocks_per_second": 100.0,
                        "checkpoint_created": True,
                        "local_root": "0x300",
                    },
                ],
                node_bin="target/release/neo-node",
                probe_bin="target/release/neo-db-probe",
            ),
        ]

        report = module.analyze_history(
            records,
            slowest_limit=5,
            fastest_limit=5,
        )

        self.assertEqual(
            [item["node_bin"] for item in report["performance_by_node_bin"]],
            ["target/release/neo-node", "unknown"],
        )
        release = report["performance_by_node_bin"][0]
        self.assertEqual(release["probe_bins"], ["target/release/neo-db-probe"])
        self.assertEqual(release["milestone_count"], 2)
        self.assertEqual(release["height_min"], 200)
        self.assertEqual(release["height_max"], 300)
        self.assertEqual(release["latest_height"], 300)
        self.assertEqual(release["latest_root"], "0x300")
        self.assertEqual(release["average_blocks_per_second"], 150.0)
        self.assertEqual(release["min_blocks_per_second"], 100.0)
        self.assertEqual(release["max_blocks_per_second"], 200.0)
        self.assertEqual(report["throughput_trend"][1]["node_bin"], "target/release/neo-node")

    def test_analyze_history_reports_sample_interval_rankings(self):
        module = load_module()
        records = [
            record(
                "2026-01-01T00:00:00+00:00",
                [
                    {
                        "height": 100,
                        "last_height": 100,
                        "blocks_per_second": 50.0,
                        "checkpoint_created": True,
                        "local_root": "0x100",
                        "height_sample_rate_summary": {
                            "slowest_interval": {
                                "from_height": 10,
                                "to_height": 10,
                                "height_delta": 0,
                                "elapsed_seconds": 10.0,
                                "blocks_per_second": 0.0,
                            },
                            "fastest_interval": {
                                "from_height": 20,
                                "to_height": 80,
                                "height_delta": 60,
                                "elapsed_seconds": 10.0,
                                "blocks_per_second": 6.0,
                            },
                        },
                    }
                ],
                node_bin="target/release/neo-node",
            ),
            record(
                "2026-01-01T00:10:00+00:00",
                [
                    {
                        "height": 200,
                        "last_height": 200,
                        "blocks_per_second": 70.0,
                        "checkpoint_created": True,
                        "local_root": "0x200",
                        "height_sample_rate_summary": {
                            "slowest_interval": {
                                "from_height": 120,
                                "to_height": 122,
                                "height_delta": 2,
                                "elapsed_seconds": 10.0,
                                "blocks_per_second": 0.2,
                            },
                            "fastest_interval": {
                                "from_height": 122,
                                "to_height": 222,
                                "height_delta": 100,
                                "elapsed_seconds": 10.0,
                                "blocks_per_second": 10.0,
                            },
                        },
                    }
                ],
                node_bin="target/release/neo-node",
            ),
        ]

        report = module.analyze_history(
            records,
            slowest_limit=1,
            fastest_limit=1,
        )

        self.assertEqual(report["slowest_sample_intervals"][0]["height"], 200)
        self.assertEqual(report["slowest_sample_intervals"][0]["blocks_per_second"], 0.2)
        self.assertEqual(report["fastest_sample_intervals"][0]["height"], 200)
        self.assertEqual(report["fastest_sample_intervals"][0]["blocks_per_second"], 10.0)

    def test_analyze_history_ranks_recurring_hot_latency_metrics(self):
        module = load_module()
        queue_wait = 'neo_state_service_mpt_apply_stage_avg_us{stage="queue_wait"}'
        load_execute = 'neo_sync_native_persist_tx_stage_avg_us{stage="load_execute"}'
        ignored = 'neo_sync_native_persist_tx_stage_avg_us{stage="ignored"}'
        records = [
            record(
                "2026-01-01T00:00:00+00:00",
                [
                    {
                        "height": 100,
                        "last_height": 100,
                        "blocks_per_second": 50.0,
                        "checkpoint_created": True,
                        "local_root": "0x100",
                        "metrics_sample_summary": {
                            "hot_metrics_by_average_us": [
                                {
                                    "name": queue_wait,
                                    "average_us": 9000.0,
                                    "last_us": 9200.0,
                                    "max_us": 10000.0,
                                    "sample_count": 2,
                                },
                                {
                                    "name": load_execute,
                                    "average_us": 6000.0,
                                    "last_us": 6100.0,
                                    "max_us": 6200.0,
                                    "sample_count": 1,
                                },
                            ]
                        },
                    }
                ],
            ),
            record(
                "2026-01-01T00:10:00+00:00",
                [
                    {
                        "height": 200,
                        "last_height": 200,
                        "blocks_per_second": 60.0,
                        "checkpoint_created": True,
                        "local_root": "0x200",
                        "metrics_sample_summary": {
                            "hot_metrics_by_average_us": [
                                {
                                    "name": queue_wait,
                                    "average_us": 7000.0,
                                    "last_us": 7200.0,
                                    "max_us": 8000.0,
                                    "sample_count": 3,
                                }
                            ]
                        },
                    },
                    {
                        "height": 300,
                        "last_height": 300,
                        "blocks_per_second": 70.0,
                        "checkpoint_created": False,
                        "local_root": "0x300",
                        "metrics_sample_summary": {
                            "hot_metrics_by_average_us": [
                                {
                                    "name": ignored,
                                    "average_us": 100000.0,
                                    "last_us": 100000.0,
                                    "max_us": 100000.0,
                                    "sample_count": 1,
                                }
                            ]
                        },
                    },
                ],
            ),
        ]

        report = module.analyze_history(
            records,
            slowest_limit=5,
            fastest_limit=5,
        )

        hot = report["hot_metrics_by_average_us"]
        self.assertEqual([item["name"] for item in hot], [queue_wait, load_execute])
        self.assertEqual(hot[0]["milestone_count"], 2)
        self.assertEqual(hot[0]["sample_count"], 5)
        self.assertEqual(hot[0]["average_us"], 7800.0)
        self.assertEqual(hot[0]["max_us"], 10000.0)
        self.assertEqual(hot[0]["heights"], [100, 200])
        self.assertEqual(hot[1]["milestone_count"], 1)
        self.assertEqual(hot[1]["sample_count"], 1)

    def test_analyze_history_can_include_checkpoint_inventory(self):
        module = load_module()
        records = [
            record(
                "2026-01-01T00:00:00+00:00",
                [
                    {
                        "height": 100,
                        "last_height": 100,
                        "blocks_per_second": 10.0,
                        "checkpoint_created": True,
                    },
                    {
                        "height": 200,
                        "last_height": 200,
                        "blocks_per_second": 20.0,
                        "checkpoint_created": True,
                    },
                ],
            )
        ]

        with tempfile.TemporaryDirectory() as tmp:
            checkpoint_root = Path(tmp) / "checkpoints"
            (checkpoint_root / "h200" / "mainnet").mkdir(parents=True)
            (checkpoint_root / "h200" / "StateRoot").mkdir()
            (checkpoint_root / "h200" / "CHECKPOINT_INFO").write_text(
                "\n".join(
                    [
                        "height=200",
                        "state_root_included=true",
                        "restore_verified=true",
                        "verified_height=200",
                        "verified_stateroot_root=0x200",
                        "verified_against_reference=true",
                        "",
                    ]
                ),
                encoding="utf-8",
            )
            (checkpoint_root / "h300" / "mainnet").mkdir(parents=True)
            (checkpoint_root / "h400" / "StateRoot").mkdir(parents=True)
            legacy = checkpoint_root / "mainnet-bounded-700000-stable"
            (legacy / "data").mkdir(parents=True)
            (legacy / "CHECKPOINT_INFO").write_text(
                "height=700000\nmode=storage-sample\nmpt_dir=missing-mpt\n",
                encoding="utf-8",
            )

            report = module.analyze_history(
                records,
                slowest_limit=5,
                fastest_limit=5,
                checkpoint_root=checkpoint_root,
            )

        inventory = report["checkpoint_inventory"]
        self.assertEqual(inventory["total_count"], 3)
        self.assertEqual(inventory["full_state_count"], 1)
        self.assertEqual(inventory["chain_only_count"], 1)
        self.assertEqual(inventory["structural_not_restore_verified_count"], 0)
        self.assertEqual(inventory["latest_full_state_height"], 200)
        self.assertEqual(inventory["retained_heights"], [200, 300, 400])
        self.assertEqual(inventory["full_state_heights"], [200])
        self.assertEqual(inventory["chain_only_heights"], [300])
        self.assertEqual(inventory["structural_not_restore_verified_heights"], [])
        self.assertEqual(inventory["history_checkpoint_heights"], [100, 200])
        self.assertEqual(inventory["history_checkpoints_not_retained"], [100])
        self.assertEqual(inventory["retained_checkpoints_not_in_history"], [])
        self.assertEqual(inventory["minimum_full_state_checkpoints"], 3)
        self.assertFalse(inventory["minimum_full_state_checkpoints_met"])
        self.assertEqual(inventory["missing_full_state_checkpoint_count"], 2)

    def test_analyze_history_reports_production_proof_readiness_gates(self):
        module = load_module()
        records = [
            record(
                "2026-01-01T00:00:00+00:00",
                [
                    {
                        "height": 100,
                        "last_height": 100,
                        "blocks_per_second": 20000.0,
                        "checkpoint_created": True,
                        "local_root": "0x100",
                        "reference_matches_local": True,
                        "stateroot_matches_chain": True,
                        "speed_proof_source": "fast-sync-transaction-blocks",
                        "import_window_blocks_per_second": 1800.0,
                        "empty_block_speed_proof_source": "fast-sync-empty-blocks",
                        "empty_block_blocks_per_second": 50000.0,
                        "empty_only_blocks": 95000,
                        "empty_block_import_seconds": 1.9,
                        "sync_proof": {
                            "fast_sync_import": {
                                "transaction_blocks": 999,
                                "transactions": 3000,
                                "transaction_block_import_seconds": 0.555,
                                "transaction_blocks_per_second": 1800.0,
                            }
                        },
                    },
                    {
                        "height": 200,
                        "last_height": 200,
                        "blocks_per_second": 21000.0,
                        "checkpoint_created": True,
                        "local_root": "0x200",
                        "reference_matches_local": True,
                        "stateroot_matches_chain": True,
                        "speed_proof_source": "fast-sync-transaction-blocks",
                        "import_window_blocks_per_second": 1900.0,
                        "sync_proof": {
                            "fast_sync_import": {
                                "transaction_blocks": 1200,
                                "transactions": 3200,
                                "transaction_block_import_seconds": 0.632,
                                "transaction_blocks_per_second": 1900.0,
                            }
                        },
                    },
                    {
                        "height": 300,
                        "last_height": 300,
                        "blocks_per_second": 22000.0,
                        "checkpoint_created": True,
                        "local_root": "0x300",
                        "reference_matches_local": True,
                        "stateroot_matches_chain": True,
                        "speed_proof_source": "fast-sync-transaction-blocks",
                        "import_window_blocks_per_second": 1750.0,
                        "sync_proof": {
                            "fast_sync_import": {
                                "transaction_blocks": 1500,
                                "transactions": 3600,
                                "transaction_block_import_seconds": 0.857,
                                "transaction_blocks_per_second": 1750.0,
                            }
                        },
                    },
                ],
            )
        ]

        with tempfile.TemporaryDirectory() as tmp:
            checkpoint_root = Path(tmp) / "checkpoints"
            for height in (100, 200, 300):
                checkpoint = checkpoint_root / f"h{height}"
                (checkpoint / "mainnet").mkdir(parents=True)
                (checkpoint / "StateRoot").mkdir()
                (checkpoint / "CHECKPOINT_INFO").write_text(
                    "state_root_included=true\n",
                    encoding="utf-8",
                )

            report = module.analyze_history(
                records,
                slowest_limit=5,
                fastest_limit=5,
                checkpoint_root=checkpoint_root,
            )

        readiness = report["production_proof_readiness"]
        self.assertFalse(readiness["ready"])
        self.assertEqual(readiness["minimum_transaction_blocks"], 1000)
        self.assertEqual(readiness["minimum_full_state_checkpoints"], 3)
        self.assertTrue(readiness["state_roots_match_chain"])
        self.assertTrue(readiness["references_match_local"])
        self.assertTrue(readiness["transaction_import_speed_floor_met"])
        self.assertFalse(readiness["transaction_import_sample_size_met"])
        self.assertFalse(readiness["restore_verified_checkpoint_floor_met"])
        self.assertIn(
            "transaction import proof has fewer than 1000 transaction-bearing blocks",
            readiness["blocking_reasons"],
        )
        self.assertIn(
            "fewer than 3 restore-verified full-state checkpoints retained",
            readiness["blocking_reasons"],
        )

    def test_analyze_history_marks_production_proof_ready_when_all_gates_pass(self):
        module = load_module()
        milestones = []
        for height, bps, tx_blocks in (
            (100, 1800.0, 1100),
            (200, 1900.0, 1200),
            (300, 1750.0, 1300),
        ):
            milestones.append(
                {
                    "height": height,
                    "last_height": height,
                    "blocks_per_second": 21000.0,
                    "checkpoint_created": True,
                    "local_root": f"0x{height}",
                    "reference_matches_local": True,
                    "stateroot_matches_chain": True,
                    "speed_proof_source": "fast-sync-transaction-blocks",
                    "import_window_blocks_per_second": bps,
                    "empty_block_speed_proof_source": "fast-sync-empty-blocks",
                    "empty_block_blocks_per_second": 80000.0,
                    "empty_only_blocks": 90000,
                    "empty_block_import_seconds": 1.125,
                    "sync_proof": {
                        "fast_sync_import": {
                            "transaction_blocks": tx_blocks,
                            "transactions": tx_blocks * 3,
                            "transaction_block_import_seconds": tx_blocks / bps,
                            "transaction_blocks_per_second": bps,
                        }
                    },
                }
            )
        records = [record("2026-01-01T00:00:00+00:00", milestones)]

        with tempfile.TemporaryDirectory() as tmp:
            checkpoint_root = Path(tmp) / "checkpoints"
            for height in (100, 200, 300):
                create_restore_verified_checkpoint(checkpoint_root, height)

            report = module.analyze_history(
                records,
                slowest_limit=5,
                fastest_limit=5,
                checkpoint_root=checkpoint_root,
            )

        readiness = report["production_proof_readiness"]
        self.assertTrue(readiness["ready"])
        self.assertEqual(readiness["blocking_reasons"], [])
        self.assertTrue(readiness["state_roots_match_chain"])
        self.assertTrue(readiness["references_match_local"])
        self.assertTrue(readiness["transaction_import_speed_floor_met"])
        self.assertTrue(readiness["transaction_import_sample_size_met"])
        self.assertTrue(readiness["restore_verified_checkpoint_floor_met"])
        self.assertEqual(readiness["restore_verified_checkpoint_count"], 3)

    def test_main_require_production_proof_exits_nonzero_when_gate_is_not_ready(self):
        module = load_module()
        records = [
            record(
                "2026-01-01T00:00:00+00:00",
                [
                    {
                        "height": 100,
                        "last_height": 100,
                        "blocks_per_second": 60000.0,
                        "checkpoint_created": True,
                        "local_root": "0x100",
                        "reference_matches_local": True,
                        "stateroot_matches_chain": True,
                        "empty_block_speed_proof_source": "fast-sync-empty-blocks",
                        "empty_block_blocks_per_second": 60000.0,
                        "empty_only_blocks": 95000,
                    }
                ],
            )
        ]

        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            history = tmp_path / "history.jsonl"
            checkpoint_root = tmp_path / "checkpoints"
            write_history(history, records)
            original_argv = module.sys.argv
            try:
                module.sys.argv = [
                    "analyze-stateroot-milestone-history.py",
                    str(history),
                    "--checkpoint-root",
                    str(checkpoint_root),
                    "--require-production-proof",
                ]
                stdout = io.StringIO()
                stderr = io.StringIO()
                with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
                    result = module.main()
            finally:
                module.sys.argv = original_argv

        self.assertEqual(result, 2)
        self.assertIn('"production_proof_readiness"', stdout.getvalue())
        self.assertIn("production proof readiness gate failed", stderr.getvalue())
        self.assertIn("missing transaction-bearing import speed proof", stderr.getvalue())

    def test_main_require_production_proof_exits_zero_when_gate_is_ready(self):
        module = load_module()
        milestones = []
        for height, bps, tx_blocks in (
            (100, 1800.0, 1100),
            (200, 1900.0, 1200),
            (300, 1750.0, 1300),
        ):
            milestones.append(
                {
                    "height": height,
                    "last_height": height,
                    "blocks_per_second": 20000.0,
                    "checkpoint_created": True,
                    "local_root": f"0x{height}",
                    "reference_matches_local": True,
                    "stateroot_matches_chain": True,
                    "speed_proof_source": "fast-sync-transaction-blocks",
                    "import_window_blocks_per_second": bps,
                    "sync_proof": {
                        "fast_sync_import": {
                            "transaction_blocks": tx_blocks,
                            "transactions": tx_blocks * 3,
                            "transaction_block_import_seconds": tx_blocks / bps,
                            "transaction_blocks_per_second": bps,
                        }
                    },
                }
            )
        records = [record("2026-01-01T00:00:00+00:00", milestones)]

        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            history = tmp_path / "history.jsonl"
            checkpoint_root = tmp_path / "checkpoints"
            write_history(history, records)
            for height in (100, 200, 300):
                create_restore_verified_checkpoint(checkpoint_root, height)
            original_argv = module.sys.argv
            try:
                module.sys.argv = [
                    "analyze-stateroot-milestone-history.py",
                    str(history),
                    "--checkpoint-root",
                    str(checkpoint_root),
                    "--require-production-proof",
                ]
                stdout = io.StringIO()
                stderr = io.StringIO()
                with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
                    result = module.main()
            finally:
                module.sys.argv = original_argv

        self.assertEqual(result, 0)
        self.assertIn('"ready": true', stdout.getvalue())
        self.assertEqual(stderr.getvalue(), "")


if __name__ == "__main__":
    unittest.main()
