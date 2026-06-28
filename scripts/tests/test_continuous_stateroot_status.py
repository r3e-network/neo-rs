import importlib.util
import unittest
from pathlib import Path
from types import SimpleNamespace


MODULE_PATH = Path(__file__).resolve().parents[1] / "continuous_stateroot_status.py"
REPO_ROOT = Path(__file__).resolve().parents[2]


def load_module():
    spec = importlib.util.spec_from_file_location("continuous_stateroot_status", MODULE_PATH)
    if spec is None or spec.loader is None:
        raise ImportError(f"unable to load module from {MODULE_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class ContinuousStateRootStatusTests(unittest.TestCase):
    def test_status_payload_recommends_three_recovery_checkpoint_stages(self):
        module = load_module()
        payload = module.build_status_payload(
            local_endpoint=SimpleNamespace(url="http://127.0.0.1:10332"),
            reference_endpoints=[
                SimpleNamespace(url="http://seed1.neo.org:10332"),
                SimpleNamespace(url="http://seed2.neo.org:10332"),
            ],
            start_block=0,
            next_block=120_001,
            last_validated_block=120_000,
            total_compared=120_001,
            total_matched=120_001,
            total_mismatched=0,
            total_errors=0,
            local_state_height=120_000,
            local_validated_height=120_000,
            local_block_count=120_001,
            mismatches=[],
            errors=[],
            started_at=0.0,
            status="running",
            target_stop_at=None,
        )

        self.assertEqual(
            [stage["stage"] for stage in payload["checkpoint_stages"]],
            ["base", "mid", "latest"],
        )
        self.assertEqual(
            [stage["height"] for stage in payload["checkpoint_stages"]],
            [0, 60_000, 120_000],
        )
        for stage in payload["checkpoint_stages"]:
            self.assertIn("scripts/checkpoint-on-height.sh", stage["command"])
            self.assertIn(f"--height {stage['height']}", stage["command"])

    def test_status_payload_waits_for_first_validated_block_before_checkpointing(self):
        module = load_module()
        payload = module.build_status_payload(
            local_endpoint=SimpleNamespace(url="http://127.0.0.1:10332"),
            reference_endpoints=[SimpleNamespace(url="http://seed1.neo.org:10332")],
            start_block=0,
            next_block=0,
            last_validated_block=-1,
            total_compared=0,
            total_matched=0,
            total_mismatched=0,
            total_errors=0,
            local_state_height=None,
            local_validated_height=None,
            local_block_count=None,
            mismatches=[],
            errors=[],
            started_at=0.0,
            status="waiting",
            target_stop_at=None,
        )

        self.assertEqual(payload["checkpoint_stages"], [])

    def test_operations_doc_explains_checkpoint_stage_status_field(self):
        text = (REPO_ROOT / "docs" / "operations.md").read_text(encoding="utf-8")

        self.assertIn("checkpoint_stages", text)
        for stage in ["base", "mid", "latest"]:
            self.assertIn(stage, text)


if __name__ == "__main__":
    unittest.main()
