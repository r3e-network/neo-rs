import importlib.util
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).resolve().parents[1] / "mainnet_replay_proofs.py"


def load_module():
    spec = importlib.util.spec_from_file_location("mainnet_replay_proofs", MODULE_PATH)
    if spec is None or spec.loader is None:
        raise ImportError(f"unable to load module from {MODULE_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class MainnetReplayProofTests(unittest.TestCase):
    def test_transaction_work_prefers_exact_import_stage_totals(self):
        module = load_module()
        report = {
            "import": {
                "transaction_blocks": 2,
                "transactions": 4,
                "transaction_blocks_per_second": 1600.0,
                "native_persist_tx": {
                    "stages": [
                        {
                            "stage": "execute",
                            "calls": 4,
                            "total_us": 800,
                            "avg_us": 200,
                        }
                    ]
                },
            },
            "hot_metrics": {
                "native_persist_tx_stages": [
                    {
                        "stage": "execute",
                        "calls": 999,
                        "total_us": 999_000,
                        "avg_us": 999,
                    }
                ]
            },
        }

        summary = module.transaction_work_summary_from_fast_sync_report(report)

        self.assertEqual(summary["source"], "fast-sync-import-native-tx-stages")
        self.assertEqual(summary["native_execution_stage_calls"], 4)
        self.assertTrue(summary["observed_transaction_work"])
        self.assertEqual(summary["metrics"][0]["total_us"], 800)
        self.assertEqual(summary["metrics"][0]["average_us"], 200)

    def test_transaction_work_falls_back_to_legacy_hot_stage_totals(self):
        module = load_module()
        report = {
            "import": {"transaction_blocks": 1, "transactions": 2},
            "hot_metrics": {
                "native_persist_tx_stages": [
                    {
                        "stage": "execute",
                        "calls": 2,
                        "total_us": 500,
                        "avg_us": 250,
                    }
                ]
            },
        }

        summary = module.transaction_work_summary_from_fast_sync_report(report)

        self.assertEqual(summary["source"], "fast-sync-native-tx-stages")
        self.assertEqual(summary["native_execution_stage_calls"], 2)
        self.assertEqual(summary["metrics"][0]["total_us"], 500)

    def test_height_sample_rate_uses_first_sample_of_each_atomic_plateau(self):
        module = load_module()
        samples = [
            {"elapsed_seconds": float(second), "height": 0}
            for second in range(10)
        ]
        samples.extend(
            {"elapsed_seconds": float(second), "height": 10000}
            for second in range(10, 20)
        )
        samples.append({"elapsed_seconds": 20.0, "height": 20000})

        summary = module.height_sample_rate_summary({"height_samples": samples})

        self.assertEqual(summary["interval_count"], 2)
        self.assertEqual(summary["min_blocks_per_second"], 1000.0)
        self.assertEqual(summary["max_blocks_per_second"], 1000.0)
        self.assertEqual(summary["slowest_interval"]["elapsed_seconds"], 10.0)
        self.assertEqual(summary["fastest_interval"]["elapsed_seconds"], 10.0)


if __name__ == "__main__":
    unittest.main()
