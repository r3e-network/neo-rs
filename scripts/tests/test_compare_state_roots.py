import importlib.util
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).resolve().parents[1] / "compare-local-csharp-rust-stateroots.py"


def load_module():
    spec = importlib.util.spec_from_file_location("compare_state_roots", MODULE_PATH)
    if spec is None or spec.loader is None:
        raise ImportError(f"unable to load module from {MODULE_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class CompareStateRootsTests(unittest.TestCase):
    def test_build_auth_header_encodes_basic_credentials(self):
        module = load_module()
        self.assertEqual(
            module.build_auth_header("neo", "secret"),
            "Basic bmVvOnNlY3JldA==",
        )

    def test_chunk_ranges_cover_full_interval(self):
        module = load_module()
        self.assertEqual(
            list(module.chunk_ranges(0, 10, 4)),
            [(0, 3), (4, 7), (8, 10)],
        )

    def test_compare_records_returns_first_mismatch(self):
        module = load_module()
        local = [
            {"index": 0, "roothash": "0x01"},
            {"index": 1, "roothash": "0x02"},
            {"index": 2, "roothash": "0x03"},
        ]
        public = [
            {"index": 0, "roothash": "0x01"},
            {"index": 1, "roothash": "0x99"},
            {"index": 2, "roothash": "0x03"},
        ]

        mismatch = module.compare_records(local, public)
        self.assertEqual(
            mismatch,
            {
                "index": 1,
                "local": {"index": 1, "roothash": "0x02"},
                "public": {"index": 1, "roothash": "0x99"},
            },
        )

    def test_compare_records_returns_none_when_equal(self):
        module = load_module()
        records = [
            {"index": 0, "roothash": "0x01"},
            {"index": 1, "roothash": "0x02"},
        ]
        self.assertIsNone(module.compare_records(records, list(records)))

    def test_should_retry_rate_limit_error(self):
        module = load_module()
        self.assertTrue(module.should_retry_rpc_error({"code": -32001, "message": "Too many requests"}))
        self.assertFalse(module.should_retry_rpc_error({"code": -32601, "message": "Method not found"}))

    def test_should_retry_curl_timeout_exit_code(self):
        module = load_module()
        self.assertTrue(module.should_retry_curl_exit_code(28))
        self.assertFalse(module.should_retry_curl_exit_code(22))

    def test_build_batch_requests_uses_incrementing_ids(self):
        module = load_module()
        batch = module.build_batch_requests("getstateroot", [3, 4, 5])
        self.assertEqual(
            batch,
            [
                {"jsonrpc": "2.0", "id": 1, "method": "getstateroot", "params": [3]},
                {"jsonrpc": "2.0", "id": 2, "method": "getstateroot", "params": [4]},
                {"jsonrpc": "2.0", "id": 3, "method": "getstateroot", "params": [5]},
            ],
        )


if __name__ == "__main__":
    unittest.main()
