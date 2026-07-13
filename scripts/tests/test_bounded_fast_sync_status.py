import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).resolve().parents[1] / "run-bounded-mainnet-replay.py"


def load_module():
    spec = importlib.util.spec_from_file_location("run_bounded_mainnet_replay", MODULE_PATH)
    if spec is None or spec.loader is None:
        raise ImportError(f"unable to load module from {MODULE_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class BoundedFastSyncStatusTests(unittest.TestCase):
    def test_fast_sync_import_proof_reports_transaction_shortfall_when_below_floor(self):
        module = load_module()
        updated = module.attach_fast_sync_report(
            {
                "status": "sync-speed-too-slow",
                "sync_source": "fast-sync",
                "target_height": 100000,
                "last_height": 100000,
                "height_samples": [],
                "sync_speed_floor_blocks_per_second": 1500.0,
                "sync_speed_shortfall_blocks_per_second": 1375.607,
                "sync_speed_band_met": False,
            },
            self.write_fast_sync_report(
                transaction_blocks=4_910,
                transaction_blocks_per_second=747.8956,
                throughput_status="below-target",
            ),
        )

        self.assertEqual(updated["status"], "sync-speed-too-slow")
        self.assertEqual(
            updated["sync_speed_measurement_source"],
            "fast-sync-transaction-blocks",
        )
        self.assertEqual(updated["sync_speed_measured_blocks_per_second"], 747.8956)
        self.assertAlmostEqual(
            updated["sync_speed_shortfall_blocks_per_second"],
            752.1044,
            places=4,
        )
        self.assertEqual(
            updated["sync_proof"]["sync_speed_measurement_source"],
            "fast-sync-transaction-blocks",
        )
        self.assertEqual(
            updated["sync_proof"]["fast_sync_import"]["transaction_blocks_per_second"],
            747.8956,
        )
        self.assertAlmostEqual(
            updated["sync_proof"]["sync_speed_shortfall_blocks_per_second"],
            752.1044,
            places=4,
        )

    def test_fast_sync_import_proof_can_satisfy_speed_gate_after_reference_match(self):
        module = load_module()
        updated = module.attach_fast_sync_report(
            {
                "status": "sync-speed-too-slow",
                "sync_source": "fast-sync",
                "target_height": 5000,
                "last_height": 5000,
                "height_samples": [],
                "transaction_work_summary": {
                    "required_for_speed_proof": True,
                    "observed_transaction_work": False,
                    "metric_count": 0,
                    "metrics": [],
                },
                "sync_speed_floor_blocks_per_second": 1500.0,
                "sync_speed_shortfall_blocks_per_second": 598.15,
                "sync_speed_band_met": False,
            },
            self.write_fast_sync_report(),
        )

        original_collect_post_probe = module.collect_post_probe
        try:
            module.collect_post_probe = lambda **_kwargs: matching_post_probe()
            updated = module.attach_post_probe_report(
                updated,
                db=Path("chain"),
                probe_bin=Path("neo-db-probe"),
                require_stateroot_height_match=True,
                reference_urls=["http://seed1.neo.org:10332"],
                require_reference_stateroot_match=True,
            )
        finally:
            module.collect_post_probe = original_collect_post_probe

        self.assertEqual(updated["status"], "target-reached")
        self.assertEqual(updated["sync_proof"]["status"], "target-reached")
        self.assertTrue(updated["sync_speed_band_met"])
        self.assertFalse(updated["transaction_work_summary"]["observed_transaction_work"])
        self.assertEqual(
            updated["sync_proof"]["fast_sync_import"]["throughput_status"],
            "meets-floor",
        )
        self.assertTrue(updated["sync_proof"]["post_probe"]["reference_matches_local"])

    def write_fast_sync_report(
        self,
        *,
        transaction_blocks: int = 54,
        transaction_blocks_per_second: float = 1748.33,
        throughput_status: str = "meets-floor",
    ) -> Path:
        self.temp_dir = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp_dir.cleanup)
        proof_path = Path(self.temp_dir.name) / "fast-sync-report.json"
        proof_path.write_text(
            json.dumps(
                {
                    "import": {
                        "imported_blocks": 5000,
                        "final_height": 5000,
                        "elapsed_seconds": 2.18,
                        "average_blocks_per_second": 2289.88,
                        "transaction_blocks": transaction_blocks,
                        "transaction_block_import_seconds": 0.0308,
                        "transaction_blocks_per_second": transaction_blocks_per_second,
                        "transactions": transaction_blocks,
                        "throughput_status": throughput_status,
                    }
                }
            ),
            encoding="utf-8",
        )
        return proof_path


def matching_post_probe():
    return {
        "chain_height": {"ok": True, "height": 5000},
        "stateroot_matches_chain": True,
        "stateroot_height": {"ok": True, "height": 5000},
        "stateroot_root": {"ok": True, "height": 5000, "root": "0xroot5000"},
        "reference_stateroot": {
            "index": 5000,
            "matches_local": True,
            "successful_samples": 5,
            "sample_count": 5,
            "reference_roots": ["0xroot5000"],
        },
    }
