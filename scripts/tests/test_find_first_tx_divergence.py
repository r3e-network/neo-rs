import importlib.util
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock


REPO_ROOT = Path(__file__).resolve().parents[2]
MODULE_PATH = REPO_ROOT / "scripts" / "find-first-tx-divergence.py"


def load_module():
    spec = importlib.util.spec_from_file_location("find_first_tx_divergence", MODULE_PATH)
    if spec is None or spec.loader is None:
        raise ImportError(f"unable to load module from {MODULE_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def application_log():
    return {
        "txid": "0xabc",
        "executions": [
            {
                "trigger": "Application",
                "vmstate": "HALT",
                "exception": None,
                "gasconsumed": "42",
                "stack": [{"type": "Boolean", "value": True}],
                "notifications": [
                    {
                        "contract": "0x123",
                        "eventname": "Transfer",
                        "state": {
                            "type": "Array",
                            "value": [{"type": "Integer", "value": "7"}],
                        },
                    }
                ],
            }
        ],
    }


class FindFirstTxDivergenceTests(unittest.TestCase):
    def test_parses_rpc_shaped_json_artifact(self):
        module = load_module()
        artifact = {"block_index": 12, **application_log()}

        with tempfile.TemporaryDirectory() as tmp:
            log_path = Path(tmp) / "trace.log"
            log_path.write_text(
                "2026-07-14 WARN NEO_TX_ARTIFACT "
                + json.dumps(artifact, separators=(",", ":"))
                + "\n",
                encoding="utf-8",
            )
            parsed = module.parse_rust_log(str(log_path), {12})

        self.assertEqual(parsed[12]["0xabc"]["artifact"], artifact)

    def test_exact_rpc_artifact_match_covers_stack_and_notifications(self):
        module = load_module()
        expected = application_log()
        artifact = {"block_index": 12, **expected}

        self.assertEqual(module.compare_rpc_artifact(artifact, expected), [])

        changed = json.loads(json.dumps(expected))
        changed["executions"][0]["notifications"][0]["state"]["value"][0][
            "value"
        ] = "8"
        diffs = module.compare_rpc_artifact(artifact, changed)
        self.assertTrue(any("notifications" in diff for diff in diffs), diffs)

    def test_malformed_json_artifact_fails_closed(self):
        module = load_module()

        with tempfile.TemporaryDirectory() as tmp:
            log_path = Path(tmp) / "trace.log"
            log_path.write_text(
                "NEO_TX_ARTIFACT {not-json}\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(ValueError, "malformed NEO_TX_ARTIFACT"):
                module.parse_rust_log(str(log_path), {12})

    def test_compare_tx_uses_complete_artifact_values(self):
        module = load_module()
        artifact = {"block_index": 12, **application_log()}
        expected = application_log()
        expected["executions"][0]["stack"][0]["value"] = False

        diffs = module.compare_tx("0xabc", {"artifact": artifact}, expected)

        self.assertTrue(any("artifact.executions[0].stack" in diff for diff in diffs), diffs)

    def test_block_fetch_error_is_counted_as_divergence(self):
        module = load_module()
        with mock.patch.object(
            module,
            "get_block_tx_hashes",
            return_value=(None, "reference timeout"),
        ):
            total, divergent, lines = module.process_block(12, {}, "http://reference")

        self.assertEqual(total, 0)
        self.assertEqual(divergent, 1)
        self.assertTrue(any("reference timeout" in line for line in lines))

    def test_strict_mode_rejects_legacy_summary_without_full_artifact(self):
        module = load_module()
        rust_summary = {
            "0xabc": {
                "vm_state": "HALT",
                "gas_consumed": "42",
                "notif_count": 0,
                "notifications": [],
            }
        }
        with mock.patch.object(
            module,
            "get_block_tx_hashes",
            return_value=(["0xabc"], None),
        ):
            total, divergent, lines = module.process_block(
                12,
                rust_summary,
                "http://reference",
                require_artifact=True,
            )

        self.assertEqual(total, 1)
        self.assertEqual(divergent, 1)
        self.assertTrue(any("NEO_TX_ARTIFACT missing" in line for line in lines))

    def test_reversed_or_negative_block_ranges_are_rejected(self):
        module = load_module()
        for value in ("10-5", "-1", "abc"):
            with self.subTest(value=value):
                with self.assertRaises(ValueError):
                    module.parse_block_range(value)


if __name__ == "__main__":
    unittest.main()
