import importlib.util
import unittest
from pathlib import Path
from unittest import mock


MODULE_PATH = Path(__file__).resolve().parents[1] / "validate-state-roots.py"


def load_module():
    spec = importlib.util.spec_from_file_location("validate_state_roots", MODULE_PATH)
    if spec is None or spec.loader is None:
        raise ImportError(f"unable to load module from {MODULE_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class ValidateStateRootsTests(unittest.TestCase):
    def test_incomplete_interval_is_not_a_pass(self):
        module = load_module()
        result, reasons = module.validation_result(10, 19, 18, 9, 0, [])
        self.assertEqual(result, "FAIL")
        self.assertTrue(any("before requested end" in reason for reason in reasons))
        self.assertTrue(any("expected 10" in reason for reason in reasons))

    def test_rpc_error_is_not_a_pass_even_when_every_other_root_matches(self):
        module = load_module()
        result, reasons = module.validation_result(
            0, 2, 2, 2, 0, [{"index": 1, "error": "timeout"}]
        )
        self.assertEqual(result, "FAIL")
        self.assertTrue(any("query" in reason for reason in reasons))

    def test_complete_exact_match_passes(self):
        module = load_module()
        self.assertEqual(module.validation_result(0, 2, 2, 3, 0, []), ("PASS", []))

    def test_diagnostic_override_only_allows_an_incomplete_interval(self):
        module = load_module()
        result, reasons = module.validation_result(
            10,
            19,
            14,
            5,
            0,
            [],
            allow_incomplete=True,
        )
        self.assertEqual(result, "INCOMPLETE")
        self.assertTrue(any("before requested end" in reason for reason in reasons))

    def test_diagnostic_override_never_hides_query_failures(self):
        module = load_module()
        result, _ = module.validation_result(
            0,
            2,
            2,
            2,
            0,
            [{"index": 1, "error": "timeout"}],
            allow_incomplete=True,
        )
        self.assertEqual(result, "FAIL")

    def test_diagnostic_override_never_hides_root_mismatches(self):
        module = load_module()
        result, _ = module.validation_result(
            0,
            2,
            2,
            3,
            1,
            [],
            allow_incomplete=True,
        )
        self.assertEqual(result, "FAIL")

    def test_state_root_requires_canonical_32_byte_hex(self):
        module = load_module()
        with mock.patch.object(
            module,
            "rpc_call",
            return_value=({"roothash": "0xabc"}, None),
        ):
            root, error = module.get_state_root("http://reference", 1)
        self.assertIsNone(root)
        self.assertIn("malformed", error)

    def test_state_root_normalizes_hex_case(self):
        module = load_module()
        expected = "0x" + "ab" * 32
        with mock.patch.object(
            module,
            "rpc_call",
            return_value=({"roothash": expected.upper().replace("X", "x")}, None),
        ):
            root, error = module.get_state_root("http://reference", 1)
        self.assertEqual(root, expected)
        self.assertIsNone(error)


if __name__ == "__main__":
    unittest.main()
