import importlib.util
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).resolve().parents[1] / "repair-bounded-replay-gas.py"


def load_module():
    spec = importlib.util.spec_from_file_location("repair_bounded_replay_gas", MODULE_PATH)
    if spec is None or spec.loader is None:
        raise ImportError(f"unable to load module from {MODULE_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class RepairBoundedReplayGasTests(unittest.TestCase):
    def test_builds_reference_repair_plan_from_latest_unique_failure(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            log = Path(tmp) / "neo-node.log"
            log.write_text(
                "native GasToken TriggerType(ON_PERSIST) hook failed at block 474702: "
                "Invalid operation: GasToken::burn: insufficient balance 804511 to burn 6694164\n"
                "native GasToken TriggerType(ON_PERSIST) hook failed at block 474702: "
                "Invalid operation: GasToken::burn: insufficient balance 804511 to burn 6694164\n"
                "native GasToken TriggerType(ON_PERSIST) hook failed at block 595391: "
                "Invalid operation: GasToken::burn: insufficient balance 364 to burn 424613\n",
                encoding="utf-8",
            )
            calls = []

            def fake_rpc(url, method, params, timeout=20.0):
                calls.append((url, method, params, timeout))
                if method == "getblock":
                    self.assertEqual(params, [595391, 1])
                    return {
                        "hash": "0xblock",
                        "tx": [
                            {
                                "hash": "0xskip",
                                "sender": "NREtce2dKienewHiA9pRiDZVXz7UUmeiMW",
                                "sysfee": "1",
                                "netfee": "2",
                            },
                            {
                                "hash": "0xb3",
                                "sender": "NR7Z7MY1QDSYpUTRYPLNz9HHc72u7dH3su",
                                "sysfee": "304613",
                                "netfee": "120000",
                            },
                        ],
                    }
                if method == "getstateroot":
                    self.assertEqual(params, [595390])
                    return {"roothash": "0xroot"}
                if method == "getstate":
                    self.assertEqual(params[0], "0xroot")
                    self.assertEqual(params[1], module.GAS_HASH)
                    return "QQEhBvJgJcxlAw=="
                raise AssertionError(f"unexpected RPC method {method}")

            plan = module.build_repair_plan(
                log_path=log,
                reference_rpc="http://seed1.neo.org:10332",
                address_version=0x35,
                which_failure="latest",
                rpc=fake_rpc,
            )

        self.assertEqual(plan["failure"]["height"], 595391)
        self.assertEqual(plan["repair"]["sender"], "NR7Z7MY1QDSYpUTRYPLNz9HHc72u7dH3su")
        self.assertEqual(plan["repair"]["reference_balance"], 3735751581938)
        self.assertEqual(plan["reference"]["matched_transaction"]["hash"], "0xb3")
        self.assertEqual(calls[0][1], "getblock")

    def test_apply_writes_reference_value_with_probe_writer(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            log = Path(tmp) / "neo-node.log"
            log.write_text(
                "native GasToken TriggerType(ON_PERSIST) hook failed at block 474702: "
                "Invalid operation: GasToken::burn: insufficient balance 804511 to burn 6694164\n",
                encoding="utf-8",
            )
            probe_calls = []

            def fake_rpc(_url, method, params, _timeout=20.0):
                if method == "getblock":
                    return {
                        "hash": "0xblock",
                        "tx": [
                            {
                                "hash": "0xfd",
                                "sender": "NRMrnHtDT4PENPpmuZAaEbPVaq7XvpVpQE",
                                "sysfee": "6573312",
                                "netfee": "120852",
                            }
                        ],
                    }
                if method == "getstateroot":
                    return {"roothash": "0xroot"}
                if method == "getstate":
                    return "QQEhBF8woDA="
                raise AssertionError(f"unexpected RPC method {method}")

            def fake_probe_writer(
                db_path,
                sender,
                value_base64,
                probe_bin,
            ):
                probe_calls.append((db_path, sender, value_base64, probe_bin))
                return {"found": True, "written_value_len": 8}

            result = module.repair_bounded_replay_gas(
                db_path=Path("bounded/data"),
                log_path=log,
                probe_bin=Path("target/release/neo-db-probe"),
                reference_rpc="http://seed1.neo.org:10332",
                apply=True,
                rpc=fake_rpc,
                probe_writer=fake_probe_writer,
            )

        self.assertTrue(result["applied"])
        self.assertEqual(result["probe"]["written_value_len"], 8)
        self.assertEqual(
            probe_calls,
            [
                (
                    Path("bounded/data"),
                    "NRMrnHtDT4PENPpmuZAaEbPVaq7XvpVpQE",
                    "QQEhBF8woDA=",
                    Path("target/release/neo-db-probe"),
                )
            ],
        )

    def test_ambiguous_fee_matches_are_disambiguated_by_local_balance(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            log = Path(tmp) / "neo-node.log"
            log.write_text(
                "native GasToken TriggerType(ON_PERSIST) hook failed at block 596898: "
                "Invalid operation: GasToken::burn: insufficient balance 0 to burn 7010070\n",
                encoding="utf-8",
            )

            def fake_rpc(_url, method, _params, _timeout=20.0):
                if method == "getblock":
                    return {
                        "hash": "0xblock",
                        "tx": [
                            {
                                "hash": "0xhas-balance",
                                "sender": "NdKTcxNfDXjKhkot3F1Mf85DKjrbE1Uvnt",
                                "sysfee": "6890068",
                                "netfee": "120002",
                            },
                            {
                                "hash": "0xmissing",
                                "sender": "NbKZAH1KpJbGE1XJarid5qxJYjLHJNHqRn",
                                "sysfee": "6890068",
                                "netfee": "120002",
                            },
                        ],
                    }
                if method == "getstateroot":
                    return {"roothash": "0xroot"}
                if method == "getstate":
                    return "QQEhBADC6ws="
                raise AssertionError(f"unexpected RPC method {method}")

            def fake_local_balance(_db_path, sender, _probe_bin):
                return {
                    "sender": sender,
                    "found": sender.startswith("NdK"),
                    "balance": 43918313 if sender.startswith("NdK") else 0,
                    "key_base64": "key",
                }

            plan = module.build_repair_plan(
                log_path=log,
                reference_rpc="http://seed1.neo.org:10332",
                address_version=0x35,
                which_failure="latest",
                db_path=Path("bounded/data"),
                probe_bin=Path("target/release/neo-db-probe"),
                rpc=fake_rpc,
                local_balance_reader=fake_local_balance,
            )

        self.assertEqual(plan["reference"]["matched_transaction"]["hash"], "0xmissing")
        self.assertEqual(plan["repair"]["sender"], "NbKZAH1KpJbGE1XJarid5qxJYjLHJNHqRn")
        self.assertEqual(len(plan["reference"]["candidate_transactions"]), 2)

    def test_repeated_fee_matches_from_one_sender_are_disambiguated_by_sender(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            log = Path(tmp) / "neo-node.log"
            log.write_text(
                "native GasToken TriggerType(ON_PERSIST) hook failed at block 682485: "
                "Invalid operation: GasToken::burn: insufficient balance 5024789 to burn 7110219\n",
                encoding="utf-8",
            )

            def fake_rpc(_url, method, _params, _timeout=20.0):
                if method == "getblock":
                    return {
                        "hash": "0xblock",
                        "tx": [
                            {
                                "hash": "0xfirst",
                                "sender": "NegDzjPnMMPt723oCsTX4gMo4y3DkyRG8S",
                                "sysfee": "6084929",
                                "netfee": "1025290",
                            },
                            {
                                "hash": "0xsecond",
                                "sender": "NegDzjPnMMPt723oCsTX4gMo4y3DkyRG8S",
                                "sysfee": "6084929",
                                "netfee": "1025290",
                            },
                        ],
                    }
                if method == "getstateroot":
                    return {"roothash": "0xroot"}
                if method == "getstate":
                    return "QQEhBADC6ws="
                raise AssertionError(f"unexpected RPC method {method}")

            def fake_local_balance(_db_path, sender, _probe_bin):
                return {
                    "sender": sender,
                    "found": True,
                    "balance": 111708074,
                    "key_base64": "key",
                }

            plan = module.build_repair_plan(
                log_path=log,
                reference_rpc="http://seed1.neo.org:10332",
                address_version=0x35,
                which_failure="latest",
                db_path=Path("bounded/data"),
                probe_bin=Path("target/release/neo-db-probe"),
                rpc=fake_rpc,
                local_balance_reader=fake_local_balance,
            )

        self.assertEqual(plan["reference"]["matched_transaction"]["hash"], "0xfirst")
        self.assertEqual(plan["repair"]["sender"], "NegDzjPnMMPt723oCsTX4gMo4y3DkyRG8S")
        self.assertEqual(len(plan["reference"]["candidate_transactions"]), 2)

    def test_repeated_block_sender_is_disambiguated_by_projected_in_block_balance(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            log = Path(tmp) / "neo-node.log"
            log.write_text(
                "native GasToken TriggerType(ON_PERSIST) hook failed at block 682490: "
                "Invalid operation: GasToken::burn: insufficient balance 1868519 to burn 12208081\n",
                encoding="utf-8",
            )

            def fake_rpc(_url, method, _params, _timeout=20.0):
                if method == "getblock":
                    return {
                        "hash": "0xblock",
                        "tx": [
                            {
                                "hash": "0xother",
                                "sender": "Nb7BhTXDTsZuWd8Tynry7q5tvNrDYEoNoy",
                                "sysfee": "6084929",
                                "netfee": "6123152",
                            },
                            {
                                "hash": "0xfirst-na",
                                "sender": "NaBEbvxLb94zcFYMTn1dxSb2rsqMHQjz9n",
                                "sysfee": "6084929",
                                "netfee": "6123152",
                            },
                            {
                                "hash": "0xsecond-na",
                                "sender": "NaBEbvxLb94zcFYMTn1dxSb2rsqMHQjz9n",
                                "sysfee": "6084929",
                                "netfee": "6123152",
                            },
                        ],
                    }
                if method == "getstateroot":
                    return {"roothash": "0xroot"}
                if method == "getstate":
                    return "QQEhBADC6ws="
                raise AssertionError(f"unexpected RPC method {method}")

            def fake_local_balance(_db_path, sender, _probe_bin):
                balances = {
                    "Nb7BhTXDTsZuWd8Tynry7q5tvNrDYEoNoy": 43245228046,
                    "NaBEbvxLb94zcFYMTn1dxSb2rsqMHQjz9n": 14076600,
                }
                return {
                    "sender": sender,
                    "found": True,
                    "balance": balances[sender],
                    "key_base64": "key",
                }

            plan = module.build_repair_plan(
                log_path=log,
                reference_rpc="http://seed1.neo.org:10332",
                address_version=0x35,
                which_failure="latest",
                db_path=Path("bounded/data"),
                probe_bin=Path("target/release/neo-db-probe"),
                rpc=fake_rpc,
                local_balance_reader=fake_local_balance,
            )

        self.assertEqual(plan["reference"]["matched_transaction"]["hash"], "0xsecond-na")
        self.assertEqual(plan["repair"]["sender"], "NaBEbvxLb94zcFYMTn1dxSb2rsqMHQjz9n")
        self.assertEqual(
            plan["reference"]["matched_transaction"]["projected_local_balance"],
            1868519,
        )

    def test_repair_plan_can_scan_only_new_log_bytes(self):
        module = load_module()
        with tempfile.TemporaryDirectory() as tmp:
            log = Path(tmp) / "neo-node.log"
            old_failure = (
                "native GasToken TriggerType(ON_PERSIST) hook failed at block 474702: "
                "Invalid operation: GasToken::burn: insufficient balance 804511 to burn 6694164\n"
            )
            log.write_text(old_failure, encoding="utf-8")
            offset = log.stat().st_size
            log.write_text(
                old_failure
                + "native GasToken TriggerType(ON_PERSIST) hook failed at block 596898: "
                "Invalid operation: GasToken::burn: insufficient balance 0 to burn 7010070\n",
                encoding="utf-8",
            )

            def fake_rpc(_url, method, params, _timeout=20.0):
                if method == "getblock":
                    self.assertEqual(params, [596898, 1])
                    return {
                        "hash": "0xnewblock",
                        "tx": [
                            {
                                "hash": "0xnew",
                                "sender": "NbKZAH1KpJbGE1XJarid5qxJYjLHJNHqRn",
                                "sysfee": "6890068",
                                "netfee": "120002",
                            }
                        ],
                    }
                if method == "getstateroot":
                    return {"roothash": "0xroot"}
                if method == "getstate":
                    return "QQEhBADC6ws="
                raise AssertionError(f"unexpected RPC method {method}")

            plan = module.build_repair_plan(
                log_path=log,
                reference_rpc="http://seed1.neo.org:10332",
                address_version=0x35,
                which_failure="latest",
                log_start_offset=offset,
                rpc=fake_rpc,
            )

        self.assertEqual(plan["failure"]["height"], 596898)
        self.assertEqual(plan["log"]["start_offset"], offset)


if __name__ == "__main__":
    unittest.main()
