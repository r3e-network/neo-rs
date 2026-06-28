import importlib.util
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).resolve().parents[1] / "compare-offline-gas-storage.py"
REPO_ROOT = Path(__file__).resolve().parents[2]


def load_module():
    spec = importlib.util.spec_from_file_location("compare_offline_gas_storage", MODULE_PATH)
    if spec is None or spec.loader is None:
        raise ImportError(f"unable to load module from {MODULE_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class CompareOfflineGasStorageTests(unittest.TestCase):
    def test_compares_local_probe_balances_against_reference_state_root(self):
        module = load_module()
        probe_calls = []
        rpc_calls = []

        def fake_probe(db_path, args, *, probe_bin):
            probe_calls.append((str(db_path), list(args), str(probe_bin)))
            if "--contract-id" in args:
                return {
                    "found": True,
                    "decoded": {
                        "format": "hash-index",
                        "index": 665626,
                        "hash_hex_le": "e54eada3a64a8f23c5232eb57fcd51b94720b34bbd97531b14bd6694ec0a3b7a",
                    },
                }
            return {
                "found": True,
                "key_base64": "FFsXKfRyjg82/UCteNWE46qLP5K8",
                "decoded": {
                    "format": "nep17-account",
                    "balance": "50177089",
                },
            }

        def fake_rpc(url, method, params, timeout=20.0):
            rpc_calls.append((url, method, params, timeout))
            if method == "getblock":
                return {
                    "hash": "0x7a3b0aec9466bd141b5397bd4bb32047b951cd7fb52e23c5238f4aa6a3ad4ee5"
                }
            if method == "getstateroot":
                return {"roothash": "0xroot"}
            if method == "getstate":
                return "QQEhBEGk/QI="
            raise AssertionError(f"unexpected RPC method {method}")

        report = module.compare_gas_accounts(
            db_path=Path("data/mainnet-replay"),
            addresses=["NUDcRfftT99w4m2puzTxQToHxZPjQ9NN9n"],
            probe_bin=Path("target/debug/neo-db-probe"),
            reference_rpc="http://seed1.neo.org:10332",
            probe_runner=fake_probe,
            rpc=fake_rpc,
        )

        self.assertTrue(report["canonical_block_hash_match"])
        self.assertTrue(report["all_balances_match"])
        self.assertEqual(report["height"], 665626)
        self.assertEqual(
            report["local_block_hash"],
            "0x7a3b0aec9466bd141b5397bd4bb32047b951cd7fb52e23c5238f4aa6a3ad4ee5",
        )
        self.assertEqual(
            report["balances"],
            [
                {
                    "address": "NUDcRfftT99w4m2puzTxQToHxZPjQ9NN9n",
                    "gas_account_key_base64": "FFsXKfRyjg82/UCteNWE46qLP5K8",
                    "local_found": True,
                    "local_balance": 50177089,
                    "reference_balance": 50177089,
                    "delta": 0,
                    "matches": True,
                }
            ],
        )
        self.assertIn(
            (
                "data/mainnet-replay",
                ["--contract-id", "-4", "--key-hex", "0c", "--decode", "hash-index"],
                "target/debug/neo-db-probe",
            ),
            probe_calls,
        )
        self.assertIn(
            (
                "http://seed1.neo.org:10332",
                "getstate",
                ["0xroot", module.GAS_HASH, "FFsXKfRyjg82/UCteNWE46qLP5K8"],
                20.0,
            ),
            rpc_calls,
        )

    def test_reports_balance_delta_when_local_storage_diverges(self):
        module = load_module()

        def fake_probe(db_path, args, *, probe_bin):
            if "--contract-id" in args:
                return {
                    "found": True,
                    "decoded": {
                        "format": "hash-index",
                        "index": 474701,
                        "hash_hex_le": "7e066ec18435c7b35c0d5934fa61306639ac6d8ff92aa7093f9d40c83e62cfff",
                    },
                }
            return {
                "found": True,
                "key_base64": "FDu+GQEs6EYlA9ypVuHWa4TlphG1",
                "decoded": {
                    "format": "nep17-account",
                    "balance": "804511",
                },
            }

        def fake_rpc(url, method, params, timeout=20.0):
            if method == "getblock":
                return {
                    "hash": "0xffcf623ec8409d3f09a72af98f6dac39663061fa34590d5cb3c73584c16e067e"
                }
            if method == "getstateroot":
                return {"roothash": "0xroot"}
            if method == "getstate":
                return "QQEhBF8woDA="
            raise AssertionError(f"unexpected RPC method {method}")

        report = module.compare_gas_accounts(
            db_path=Path("data/mainnet-validate"),
            addresses=["NRMrnHtDT4PENPpmuZAaEbPVaq7XvpVpQE"],
            probe_bin=Path("target/debug/neo-db-probe"),
            reference_rpc="http://seed1.neo.org:10332",
            probe_runner=fake_probe,
            rpc=fake_rpc,
        )

        self.assertFalse(report["all_balances_match"])
        self.assertEqual(report["balances"][0]["local_balance"], 804511)
        self.assertEqual(report["balances"][0]["reference_balance"], 815804511)
        self.assertEqual(report["balances"][0]["delta"], -815000000)
        self.assertFalse(report["balances"][0]["matches"])

    def test_operations_doc_mentions_offline_gas_storage_compare(self):
        text = (REPO_ROOT / "docs" / "operations.md").read_text(encoding="utf-8")

        self.assertIn("scripts/compare-offline-gas-storage.py", text)
        self.assertIn("neo-db-probe", text)


if __name__ == "__main__":
    unittest.main()
