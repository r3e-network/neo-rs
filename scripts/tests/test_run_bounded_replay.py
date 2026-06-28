import importlib.util
import json
import unittest
from pathlib import Path
from types import SimpleNamespace


MODULE_PATH = Path(__file__).resolve().parents[1] / "run-bounded-mainnet-replay.py"


def load_module():
    spec = importlib.util.spec_from_file_location("run_bounded_mainnet_replay", MODULE_PATH)
    if spec is None or spec.loader is None:
        raise ImportError(f"unable to load module from {MODULE_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class FakeClock:
    def __init__(self):
        self.now = 0.0
        self.sleeps = []

    def time(self):
        return self.now

    def sleep(self, seconds):
        self.sleeps.append(seconds)
        self.now += seconds


class FakeProcess:
    def __init__(self, pid=1234):
        self.pid = pid
        self.returncode = None
        self.terminated = False
        self.killed = False

    def poll(self):
        return self.returncode

    def terminate(self):
        self.terminated = True
        self.returncode = 0

    def kill(self):
        self.killed = True
        self.returncode = -9

    def communicate(self, timeout=None):
        return ("", None)


class RunBoundedReplayTests(unittest.TestCase):
    def test_node_command_passes_stop_height_to_node(self):
        module = load_module()

        command = module.node_command(
            Path("target/release/neo-node"),
            Path("bounded.toml"),
            665603,
        )

        self.assertEqual(
            command,
            [
                "target/release/neo-node",
                "--config",
                "bounded.toml",
                "--stop-at-height",
                "665603",
            ],
        )

    def test_parse_prometheus_metrics_keeps_hotspot_metrics(self):
        module = load_module()

        metrics = module.parse_prometheus_metrics(
            "\n".join(
                [
                    "# HELP ignored ignored",
                    "neo_sync_avg_total_us 4200",
                    "neo_sync_native_persist_avg_tx_us 1200",
                    'neo_sync_native_contract_hook_avg_us{trigger="onpersist",contract="GasToken",id="-6"} 7100',
                    'neo_sync_neotoken_onpersist_stage_avg_us{stage="compute_committee"} 3300',
                    'neo_sync_neotoken_committee_compute_stage_avg_us{stage="candidate_state_decode"} 2100',
                    'neo_sync_neotoken_committee_candidate_scan_avg_items{kind="eligible_candidates"} 42',
                    'neo_state_service_mpt_apply_stage_avg_us{stage="trie_commit"} 120',
                    'neo_state_service_mpt_apply_avg_items{kind="overlay_entries"} 19',
                    'neo_node_service_enabled{service="state_service"} 1',
                    "neo_state_service_mpt_apply_avg_changes 17",
                    "bad_line nope",
                ]
            )
        )

        self.assertEqual(metrics["neo_sync_avg_total_us"], 4200.0)
        self.assertEqual(metrics["neo_sync_native_persist_avg_tx_us"], 1200.0)
        self.assertEqual(
            metrics[
                'neo_sync_native_contract_hook_avg_us{trigger="onpersist",contract="GasToken",id="-6"}'
            ],
            7100.0,
        )
        self.assertEqual(
            metrics['neo_sync_neotoken_onpersist_stage_avg_us{stage="compute_committee"}'],
            3300.0,
        )
        self.assertEqual(
            metrics[
                'neo_sync_neotoken_committee_compute_stage_avg_us{stage="candidate_state_decode"}'
            ],
            2100.0,
        )
        self.assertEqual(
            metrics[
                'neo_sync_neotoken_committee_candidate_scan_avg_items{kind="eligible_candidates"}'
            ],
            42.0,
        )
        self.assertEqual(
            metrics['neo_state_service_mpt_apply_stage_avg_us{stage="trie_commit"}'],
            120.0,
        )
        self.assertEqual(
            metrics['neo_state_service_mpt_apply_avg_items{kind="overlay_entries"}'],
            19.0,
        )
        self.assertEqual(metrics["neo_state_service_mpt_apply_avg_changes"], 17.0)
        self.assertNotIn("neo_node_service_enabled", metrics)

    def test_stops_node_when_target_height_is_reached_and_reports_rate(self):
        module = load_module()
        clock = FakeClock()
        process = FakeProcess()
        heights = iter([608755, 620000, 665627])
        spawned = []

        def fake_spawn(command, **kwargs):
            spawned.append((command, kwargs))
            return process

        def fake_rpc(url, method, params=None, timeout=5.0):
            self.assertEqual(method, "getblockcount")
            return next(heights) + 1

        report = module.run_until_target(
            command=["neo-node", "--config", "bounded.toml"],
            rpc_url="http://127.0.0.1:21332",
            target_height=665627,
            poll_interval=10,
            max_seconds=100,
            spawner=fake_spawn,
            rpc=fake_rpc,
            clock=clock,
        )

        self.assertEqual(spawned[0][0], ["neo-node", "--config", "bounded.toml"])
        self.assertEqual(spawned[0][1]["stdout"], module.subprocess.DEVNULL)
        self.assertTrue(process.terminated)
        self.assertEqual(report["status"], "target-reached")
        self.assertEqual(report["last_height"], 665627)
        self.assertEqual(report["height_samples"][0]["height"], 608755)
        self.assertGreater(report["blocks_per_second"], 0)

    def test_run_until_target_attaches_metrics_to_height_samples(self):
        module = load_module()
        clock = FakeClock()
        process = FakeProcess()

        report = module.run_until_target(
            command=["neo-node"],
            rpc_url="http://127.0.0.1:21332",
            target_height=10,
            poll_interval=10,
            max_seconds=100,
            spawner=lambda command, **kwargs: process,
            rpc=lambda *args, **kwargs: 11,
            clock=clock,
            metrics_url="http://127.0.0.1:21990/metrics",
            metrics_fetcher=lambda url: {"neo_sync_avg_total_us": 4200.0},
        )

        self.assertEqual(report["status"], "target-reached")
        self.assertEqual(
            report["height_samples"][0]["metrics"]["neo_sync_avg_total_us"],
            4200.0,
        )

    def test_run_until_target_records_metrics_errors_without_failing_replay(self):
        module = load_module()
        clock = FakeClock()
        process = FakeProcess()

        def failing_metrics(_url):
            raise RuntimeError("metrics unavailable")

        report = module.run_until_target(
            command=["neo-node"],
            rpc_url="http://127.0.0.1:21332",
            target_height=10,
            poll_interval=10,
            max_seconds=100,
            spawner=lambda command, **kwargs: process,
            rpc=lambda *args, **kwargs: 11,
            clock=clock,
            metrics_url="http://127.0.0.1:21990/metrics",
            metrics_fetcher=failing_metrics,
        )

        self.assertEqual(report["status"], "target-reached")
        self.assertIn("metrics unavailable", report["height_samples"][0]["metrics_error"])

    def test_routes_node_output_to_requested_handle(self):
        module = load_module()
        clock = FakeClock()
        process = FakeProcess()
        output_handle = object()
        spawned = []

        def fake_spawn(command, **kwargs):
            spawned.append((command, kwargs))
            return process

        report = module.run_until_target(
            command=["neo-node"],
            rpc_url="http://127.0.0.1:21332",
            target_height=1,
            poll_interval=10,
            max_seconds=100,
            spawner=fake_spawn,
            rpc=lambda *args, **kwargs: 2,
            clock=clock,
            node_output=output_handle,
        )

        self.assertEqual(report["status"], "target-reached")
        self.assertIs(spawned[0][1]["stdout"], output_handle)

    def test_reports_process_exit_before_target(self):
        module = load_module()
        clock = FakeClock()
        process = FakeProcess()
        process.returncode = 42

        report = module.run_until_target(
            command=["neo-node"],
            rpc_url="http://127.0.0.1:21332",
            target_height=665627,
            poll_interval=10,
            max_seconds=100,
            spawner=lambda command, **kwargs: process,
            rpc=lambda *args, **kwargs: 1,
            clock=clock,
        )

        self.assertEqual(report["status"], "process-exited")
        self.assertEqual(report["returncode"], 42)
        self.assertFalse(process.terminated)

    def test_clean_process_exit_counts_as_target_reached_for_node_stop_height(self):
        module = load_module()
        clock = FakeClock()
        process = FakeProcess()
        process.returncode = 0

        report = module.run_until_target(
            command=["neo-node", "--stop-at-height", "665603"],
            rpc_url="http://127.0.0.1:21332",
            target_height=665603,
            poll_interval=10,
            max_seconds=100,
            spawner=lambda command, **kwargs: process,
            rpc=lambda *args, **kwargs: 665604,
            clock=clock,
        )

        self.assertEqual(report["status"], "target-reached")
        self.assertEqual(report["returncode"], 0)
        self.assertFalse(process.terminated)
        self.assertEqual(report["last_height"], 665603)

    def test_clean_process_exit_before_target_is_not_target_reached(self):
        module = load_module()
        clock = FakeClock()
        process = FakeProcess()
        process.returncode = 0

        report = module.run_until_target(
            command=["neo-node", "--stop-at-height", "665603"],
            rpc_url="http://127.0.0.1:21332",
            target_height=665603,
            poll_interval=10,
            max_seconds=100,
            spawner=lambda command, **kwargs: process,
            rpc=lambda *args, **kwargs: 596898,
            clock=clock,
        )

        self.assertEqual(report["status"], "process-exited")
        self.assertEqual(report["returncode"], 0)
        self.assertEqual(report["last_height"], 596897)

    def test_timeout_terminates_node_and_keeps_last_height(self):
        module = load_module()
        clock = FakeClock()
        process = FakeProcess()

        report = module.run_until_target(
            command=["neo-node"],
            rpc_url="http://127.0.0.1:21332",
            target_height=665627,
            poll_interval=10,
            max_seconds=15,
            spawner=lambda command, **kwargs: process,
            rpc=lambda *args, **kwargs: 608756,
            clock=clock,
        )

        self.assertEqual(report["status"], "timeout")
        self.assertTrue(process.terminated)
        self.assertEqual(report["last_height"], 608755)

    def test_repairable_failure_detector_stops_node_before_timeout(self):
        module = load_module()
        clock = FakeClock()
        process = FakeProcess()
        detector_calls = []

        def fake_rpc(*_args, **_kwargs):
            raise RuntimeError("rpc unavailable")

        def fake_detector():
            detector_calls.append(True)
            return len(detector_calls) == 2

        report = module.run_until_target(
            command=["neo-node"],
            rpc_url="http://127.0.0.1:21332",
            target_height=700000,
            poll_interval=10,
            max_seconds=100,
            spawner=lambda command, **kwargs: process,
            rpc=fake_rpc,
            clock=clock,
            repairable_failure_detector=fake_detector,
        )

        self.assertEqual(report["status"], "repairable-failure")
        self.assertTrue(process.terminated)
        self.assertEqual(len(detector_calls), 2)
        self.assertLess(clock.time(), 100)

    def test_rpc_failure_uses_height_reader_fallback(self):
        module = load_module()
        clock = FakeClock()
        process = FakeProcess()
        fallback_heights = iter([677297, 677300])

        def fake_rpc(*_args, **_kwargs):
            raise RuntimeError("rpc unavailable")

        report = module.run_until_target(
            command=["neo-node"],
            rpc_url="http://127.0.0.1:21332",
            target_height=677300,
            poll_interval=10,
            max_seconds=100,
            spawner=lambda command, **kwargs: process,
            rpc=fake_rpc,
            clock=clock,
            height_reader=lambda: next(fallback_heights),
        )

        self.assertEqual(report["status"], "target-reached")
        self.assertEqual(report["last_height"], 677300)
        self.assertTrue(process.terminated)
        self.assertEqual(report["height_samples"][0]["height"], 677297)
        self.assertEqual(report["height_samples"][0]["height_source"], "fallback")

    def test_read_probe_ledger_height_decodes_hash_index_probe_output(self):
        module = load_module()
        captured = {}

        def fake_run(command, **kwargs):
            captured["command"] = command
            captured["kwargs"] = kwargs
            return SimpleNamespace(
                stdout=json.dumps({"decoded": {"format": "hash-index", "index": 677297}})
            )

        original_run = module.subprocess.run
        try:
            module.subprocess.run = fake_run
            height = module.read_probe_ledger_height(
                Path("bounded/data"),
                Path("target/release/neo-db-probe"),
            )
        finally:
            module.subprocess.run = original_run

        self.assertEqual(height, 677297)
        self.assertEqual(captured["command"][0], "target/release/neo-db-probe")
        self.assertEqual(captured["command"][captured["command"].index("--db") + 1], "bounded/data")
        self.assertIn("--decode", captured["command"])
        self.assertTrue(captured["kwargs"]["check"])

    def test_read_probe_ledger_height_returns_none_when_ledger_key_missing(self):
        module = load_module()

        def fake_run(command, **kwargs):
            return SimpleNamespace(stdout=json.dumps({"found": False}))

        original_run = module.subprocess.run
        try:
            module.subprocess.run = fake_run
            height = module.read_probe_ledger_height(
                Path("fresh/chain"),
                Path("target/release/neo-db-probe"),
            )
        finally:
            module.subprocess.run = original_run

        self.assertIsNone(height)

    def test_read_probe_mpt_state_height_decodes_current_root_index(self):
        module = load_module()
        captured = {}

        def fake_run(command, **kwargs):
            captured["command"] = command
            captured["kwargs"] = kwargs
            return SimpleNamespace(
                stdout=json.dumps(
                    {
                        "mode": "state-service-mpt",
                        "height": {
                            "found": True,
                            "decoded": {"current_local_root_index": 677300},
                        },
                    }
                )
            )

        original_run = module.subprocess.run
        try:
            module.subprocess.run = fake_run
            height = module.read_probe_mpt_state_height(
                Path("bounded/state-root-334F454E"),
                Path("target/release/neo-db-probe"),
            )
        finally:
            module.subprocess.run = original_run

        self.assertEqual(height, 677300)
        self.assertEqual(captured["command"][0], "target/release/neo-db-probe")
        self.assertEqual(
            captured["command"][captured["command"].index("--db") + 1],
            "bounded/state-root-334F454E",
        )
        self.assertIn("--mpt-state-height", captured["command"])
        self.assertTrue(captured["kwargs"]["check"])

    def test_read_probe_mpt_state_root_decodes_roothash(self):
        module = load_module()
        captured = {}

        def fake_run(command, **kwargs):
            captured["command"] = command
            captured["kwargs"] = kwargs
            return SimpleNamespace(
                stdout=json.dumps(
                    {
                        "mode": "state-service-mpt",
                        "state_root": {
                            "found": True,
                            "decoded": {"roothash": "0xabc123"},
                        },
                    }
                )
            )

        original_run = module.subprocess.run
        try:
            module.subprocess.run = fake_run
            root = module.read_probe_mpt_state_root(
                Path("bounded/state-root-334F454E"),
                Path("target/release/neo-db-probe"),
                677300,
            )
        finally:
            module.subprocess.run = original_run

        self.assertEqual(root, "0xabc123")
        self.assertEqual(captured["command"][0], "target/release/neo-db-probe")
        self.assertIn("--mpt-state-root", captured["command"])
        self.assertIn("677300", captured["command"])
        self.assertTrue(captured["kwargs"]["check"])

    def test_collect_post_probe_reports_chain_and_stateroot_match(self):
        module = load_module()

        original_chain_reader = module.read_probe_ledger_height
        original_state_reader = module.read_probe_mpt_state_height
        original_root_reader = module.read_probe_mpt_state_root
        try:
            module.read_probe_ledger_height = lambda db, probe: 677300
            module.read_probe_mpt_state_height = lambda db, probe: 677300
            module.read_probe_mpt_state_root = lambda db, probe, index: "0xabc123"
            post_probe = module.collect_post_probe(
                chain_db=Path("bounded/chain"),
                stateroot_db=Path("bounded/state-root-334F454E"),
                probe_bin=Path("target/release/neo-db-probe"),
            )
        finally:
            module.read_probe_ledger_height = original_chain_reader
            module.read_probe_mpt_state_height = original_state_reader
            module.read_probe_mpt_state_root = original_root_reader

        self.assertTrue(post_probe["chain_height"]["ok"])
        self.assertEqual(post_probe["chain_height"]["height"], 677300)
        self.assertTrue(post_probe["stateroot_height"]["ok"])
        self.assertEqual(post_probe["stateroot_height"]["height"], 677300)
        self.assertEqual(post_probe["stateroot_root"]["root"], "0xabc123")
        self.assertTrue(post_probe["stateroot_matches_chain"])

    def test_collect_post_probe_can_compare_reference_stateroots(self):
        module = load_module()

        original_chain_reader = module.read_probe_ledger_height
        original_state_reader = module.read_probe_mpt_state_height
        original_root_reader = module.read_probe_mpt_state_root
        try:
            module.read_probe_ledger_height = lambda db, probe: 11
            module.read_probe_mpt_state_height = lambda db, probe: 11
            module.read_probe_mpt_state_root = lambda db, probe, index: "0xabc123"

            def fake_rpc(url, method, params=None, timeout=5.0):
                self.assertEqual(method, "getstateroot")
                self.assertEqual(params, [11])
                return {"index": 11, "roothash": "0xabc123"}

            post_probe = module.collect_post_probe(
                chain_db=Path("bounded/chain"),
                stateroot_db=Path("bounded/state-root-334F454E"),
                probe_bin=Path("target/release/neo-db-probe"),
                reference_urls=["http://seed1.neo.org:10332,http://seed2.neo.org:10332"],
                rpc=fake_rpc,
            )
        finally:
            module.read_probe_ledger_height = original_chain_reader
            module.read_probe_mpt_state_height = original_state_reader
            module.read_probe_mpt_state_root = original_root_reader

        reference = post_probe["reference_stateroot"]
        self.assertTrue(reference["matches_local"])
        self.assertEqual(reference["successful_samples"], 2)
        self.assertEqual(reference["reference_roots"], ["0xabc123"])

    def test_required_stateroot_match_marks_target_run_failed_on_mismatch(self):
        module = load_module()
        report = {"status": "target-reached", "target_height": 10}

        original_collect = module.collect_post_probe
        try:
            module.collect_post_probe = lambda **kwargs: {
                "chain_height": {"ok": True, "height": 10},
                "stateroot_height": {"ok": True, "height": 9},
                "stateroot_matches_chain": False,
            }
            updated = module.attach_post_probe_report(
                report,
                chain_db=Path("bounded/chain"),
                stateroot_db=Path("bounded/state-root-334F454E"),
                probe_bin=Path("target/release/neo-db-probe"),
                require_stateroot_height_match=True,
            )
        finally:
            module.collect_post_probe = original_collect

        self.assertEqual(updated["status"], "stateroot-height-mismatch")
        self.assertFalse(updated["post_probe"]["stateroot_matches_chain"])

    def test_required_reference_match_marks_target_run_failed_on_mismatch(self):
        module = load_module()
        report = {"status": "target-reached", "target_height": 10}

        original_collect = module.collect_post_probe
        try:
            module.collect_post_probe = lambda **kwargs: {
                "chain_height": {"ok": True, "height": 10},
                "stateroot_height": {"ok": True, "height": 10},
                "stateroot_matches_chain": True,
                "reference_stateroot": {
                    "matches_local": False,
                    "local_root": "0xlocal",
                    "reference_roots": ["0xref"],
                },
            }
            updated = module.attach_post_probe_report(
                report,
                chain_db=Path("bounded/chain"),
                stateroot_db=Path("bounded/state-root-334F454E"),
                probe_bin=Path("target/release/neo-db-probe"),
                require_stateroot_height_match=True,
                reference_urls=["http://seed1.neo.org:10332"],
                require_reference_stateroot_match=True,
            )
        finally:
            module.collect_post_probe = original_collect

        self.assertEqual(updated["status"], "reference-stateroot-mismatch")
        self.assertFalse(updated["post_probe"]["reference_stateroot"]["matches_local"])

    def test_required_reference_match_uses_default_references_when_omitted(self):
        module = load_module()
        report = {"status": "target-reached", "target_height": 10}
        captured = {}

        original_collect = module.collect_post_probe
        try:
            def fake_collect(**kwargs):
                captured["reference_urls"] = kwargs["reference_urls"]
                return {
                    "chain_height": {"ok": True, "height": 10},
                    "stateroot_height": {"ok": True, "height": 10},
                    "stateroot_matches_chain": True,
                    "reference_stateroot": {
                        "matches_local": True,
                        "local_root": "0xabc123",
                        "reference_roots": ["0xabc123"],
                    },
                }

            module.collect_post_probe = fake_collect
            updated = module.attach_post_probe_report(
                report,
                chain_db=Path("bounded/chain"),
                stateroot_db=Path("bounded/state-root-334F454E"),
                probe_bin=Path("target/release/neo-db-probe"),
                require_stateroot_height_match=True,
                require_reference_stateroot_match=True,
            )
        finally:
            module.collect_post_probe = original_collect

        self.assertEqual(updated["status"], "target-reached")
        self.assertEqual(captured["reference_urls"], module.DEFAULT_REFERENCE_RPCS)


if __name__ == "__main__":
    unittest.main()
