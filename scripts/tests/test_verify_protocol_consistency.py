import copy
import importlib.util
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).resolve().parents[1] / "verify-protocol-consistency.py"


def load_module():
    spec = importlib.util.spec_from_file_location("verify_protocol_consistency", MODULE_PATH)
    if spec is None or spec.loader is None:
        raise ImportError(f"unable to load module from {MODULE_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


CSHARP_LOG = {
    "txid": "0xABCD",
    "executions": [
        {
            "trigger": "Application",
            "vmstate": "HALT",
            "exception": None,
            "gasconsumed": "9977780",
            "stack": [{"type": "Boolean", "value": True}],
            "notifications": [
                {
                    "contract": "0xD2A4CFF31913016155E38E474A2C06D08BE276CF",
                    "eventname": "Transfer",
                    "state": {"type": "Array", "value": [{"type": "Integer", "value": "1"}]},
                }
            ],
        }
    ],
}


class DiffApplogTests(unittest.TestCase):
    def setUp(self):
        self.module = load_module()

    def test_identical_logs_match_ignoring_hex_and_key_case(self):
        rust = copy.deepcopy(CSHARP_LOG)
        rust["txid"] = "0xabcd"
        rust["executions"][0]["notifications"][0]["contract"] = (
            "0xd2a4cff31913016155e38e474a2c06d08be276cf"
        )
        rust["executions"][0]["gasConsumed"] = rust["executions"][0].pop("gasconsumed")
        self.assertEqual(self.module.diff_applog(rust, CSHARP_LOG), [])

    def test_each_consensus_field_difference_is_reported(self):
        cases = {
            "vmstate": "FAULT",
            "exception": "boom",
            "gasconsumed": "9977781",
            "stack": [],
            "notifications": [],
            "trigger": "Verification",
        }
        for field, value in cases.items():
            with self.subTest(field=field):
                rust = copy.deepcopy(CSHARP_LOG)
                rust["executions"][0][field] = value
                diffs = self.module.diff_applog(rust, CSHARP_LOG)
                self.assertEqual(len(diffs), 1)
                self.assertIn(f"executions[0].{field}", diffs[0])

    def test_execution_count_and_missing_executions_fail(self):
        rust = copy.deepcopy(CSHARP_LOG)
        rust["executions"].append(copy.deepcopy(rust["executions"][0]))
        self.assertTrue(self.module.diff_applog(rust, CSHARP_LOG))
        with self.assertRaises(ValueError):
            self.module.diff_applog({"txid": "0x1"}, CSHARP_LOG)


if __name__ == "__main__":
    unittest.main()
