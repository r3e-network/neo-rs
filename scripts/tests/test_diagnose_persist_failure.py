import importlib.util
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).resolve().parents[1] / "diagnose-persist-failure.py"
REPO_ROOT = Path(__file__).resolve().parents[2]


def load_module():
    spec = importlib.util.spec_from_file_location("diagnose_persist_failure", MODULE_PATH)
    if spec is None or spec.loader is None:
        raise ImportError(f"unable to load module from {MODULE_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class DiagnosePersistFailureTests(unittest.TestCase):
    def test_parse_gas_burn_failure_from_json_log_line(self):
        module = load_module()
        log = (
            '{"fields":{"err":"Invalid operation: native GasToken '
            'TriggerType(ON_PERSIST) hook failed at block 474702: Invalid '
            'operation: GasToken::burn: insufficient balance 804511 to burn '
            '6694164"}}'
        )

        failures = module.parse_gas_burn_failures(log)

        self.assertEqual(
            failures,
            [
                {
                    "height": 474702,
                    "balance": 804511,
                    "burn_amount": 6694164,
                    "contract": "GasToken",
                }
            ],
        )

    def test_build_diagnosis_matches_reference_transaction_fee(self):
        module = load_module()
        failure = {
            "height": 474702,
            "balance": 804511,
            "burn_amount": 6694164,
            "contract": "GasToken",
        }
        reference_block = {
            "hash": "0xe5bf",
            "primary": 4,
            "tx": [
                {
                    "hash": "0xfd9a",
                    "sender": "NRMrnHtDT4PENPpmuZAaEbPVaq7XvpVpQE",
                    "sysfee": "6573312",
                    "netfee": "120852",
                    "attributes": [],
                }
            ],
        }
        status = {
            "status": "WAITING_FOR_SYNC",
            "last_validated_block": 0,
            "local_block_count": 474702,
            "local_state_height": 0,
        }

        diagnosis = module.build_diagnosis(
            failure,
            reference_block=reference_block,
            status=status,
        )

        self.assertEqual(diagnosis["classification"], "local_state_divergence")
        self.assertEqual(diagnosis["reference"]["matching_fee_transactions"][0]["hash"], "0xfd9a")
        self.assertEqual(diagnosis["reference"]["matching_fee_transactions"][0]["total_fee"], 6694164)
        self.assertIn("restore", diagnosis["recommendation"])

    def test_gas_account_storage_key_uses_nep17_account_prefix_and_address_hash(self):
        module = load_module()

        key = module.gas_account_storage_key("NRMrnHtDT4PENPpmuZAaEbPVaq7XvpVpQE")

        self.assertEqual(key, "FDu+GQEs6EYlA9ypVuHWa4TlphG1")

    def test_decode_storage_integer_reads_signed_little_endian_big_integer(self):
        module = load_module()

        self.assertEqual(module.decode_storage_integer("n0YM"), 804511)
        self.assertEqual(module.decode_storage_integer(""), 0)

    def test_decode_nep17_account_balance_reads_account_state_struct(self):
        module = load_module()

        self.assertEqual(module.decode_nep17_account_balance("QQEhBEGk/QI="), 50177089)
        self.assertEqual(module.decode_nep17_account_balance(""), 0)

    def test_fetch_local_gas_balances_queries_matching_sender_storage(self):
        module = load_module()
        calls = []

        def fake_rpc_call(url, method, params, timeout=20.0):
            calls.append((url, method, params, timeout))
            return "QQEhBEGk/QI="

        module.rpc_call = fake_rpc_call

        balances = module.fetch_local_gas_balances(
            local_rpc="http://127.0.0.1:20332",
            transactions=[
                {"sender": "NRMrnHtDT4PENPpmuZAaEbPVaq7XvpVpQE"},
                {"sender": "NRMrnHtDT4PENPpmuZAaEbPVaq7XvpVpQE"},
            ],
            address_version=0x35,
        )

        self.assertEqual(
            calls,
            [
                (
                    "http://127.0.0.1:20332",
                    "getstorage",
                    ["GasToken", "FDu+GQEs6EYlA9ypVuHWa4TlphG1"],
                    20.0,
                )
            ],
        )
        self.assertEqual(
            balances,
            [
                {
                    "sender": "NRMrnHtDT4PENPpmuZAaEbPVaq7XvpVpQE",
                    "gas_account_key_base64": "FDu+GQEs6EYlA9ypVuHWa4TlphG1",
                    "value_base64": "QQEhBEGk/QI=",
                    "balance": 50177089,
                }
            ],
        )

    def test_build_diagnosis_includes_local_balance_observations(self):
        module = load_module()
        failure = {
            "height": 474702,
            "balance": 804511,
            "burn_amount": 6694164,
            "contract": "GasToken",
        }

        diagnosis = module.build_diagnosis(
            failure,
            reference_block={"hash": "0xe5bf", "primary": 4, "tx": []},
            local_gas_balances=[{"sender": "N...", "balance": 804511}],
        )

        self.assertEqual(diagnosis["local"]["gas_balances"][0]["balance"], 804511)

    def test_cli_reports_log_only_when_reference_rpc_is_omitted(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            log_path = Path(tmp) / "node.log"
            log_path.write_text(
                "native GasToken TriggerType(ON_PERSIST) hook failed at block 31220: "
                "Invalid operation: GasToken::burn: insufficient balance 17762830 "
                "to burn 64419880\n",
                encoding="utf-8",
            )

            result = module.run_diagnosis(log_path=log_path, status_path=None, reference_rpc=None)

            self.assertEqual(result["failure"]["height"], 31220)
            self.assertEqual(result["classification"], "gas_burn_failure_unenriched")

    def test_operations_doc_mentions_persist_failure_diagnosis_entrypoint(self):
        text = (REPO_ROOT / "docs" / "operations.md").read_text(encoding="utf-8")

        self.assertIn("scripts/diagnose-persist-failure.py", text)
        self.assertIn("GasToken::burn", text)


if __name__ == "__main__":
    unittest.main()
