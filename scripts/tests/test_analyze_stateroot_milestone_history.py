import importlib.util
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
        self.assertEqual(inventory["latest_full_state_height"], 200)
        self.assertEqual(inventory["retained_heights"], [200, 300, 400])
        self.assertEqual(inventory["full_state_heights"], [200])
        self.assertEqual(inventory["chain_only_heights"], [300])
        self.assertEqual(inventory["history_checkpoint_heights"], [100, 200])
        self.assertEqual(inventory["history_checkpoints_not_retained"], [100])
        self.assertEqual(inventory["retained_checkpoints_not_in_history"], [])
        self.assertEqual(inventory["minimum_full_state_checkpoints"], 3)
        self.assertFalse(inventory["minimum_full_state_checkpoints_met"])
        self.assertEqual(inventory["missing_full_state_checkpoint_count"], 2)


if __name__ == "__main__":
    unittest.main()
