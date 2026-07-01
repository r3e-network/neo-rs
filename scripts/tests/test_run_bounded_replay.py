import importlib.util
import json
import tempfile
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


class FakeHttpResponse:
    def __init__(self, payload: str):
        self.payload = payload.encode("utf-8")

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, traceback):
        return False

    def read(self):
        return self.payload


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

    def test_node_command_can_import_chain_acc_before_syncing(self):
        module = load_module()

        command = module.node_command(
            Path("target/release/neo-node"),
            Path("bounded.toml"),
            100000,
            import_chain=Path("chain.0.100k.acc"),
        )

        self.assertEqual(
            command,
            [
                "target/release/neo-node",
                "--config",
                "bounded.toml",
                "--stop-at-height",
                "100000",
                "--import-chain",
                "chain.0.100k.acc",
            ],
        )

    def test_node_command_can_run_builtin_fast_sync_before_syncing(self):
        module = load_module()

        command = module.node_command(
            Path("target/release/neo-node"),
            Path("bounded.toml"),
            100000,
            fast_sync=True,
            fast_sync_cache=Path("fast-sync-cache"),
        )

        self.assertEqual(
            command,
            [
                "target/release/neo-node",
                "--config",
                "bounded.toml",
                "--stop-at-height",
                "100000",
                "--fast-sync",
                "--fast-sync-cache",
                "fast-sync-cache",
            ],
        )

    def test_node_command_can_request_fast_sync_report_sidecar(self):
        module = load_module()

        command = module.node_command(
            Path("target/release/neo-node"),
            Path("bounded.toml"),
            100000,
            fast_sync=True,
            fast_sync_cache=Path("fast-sync-cache"),
            fast_sync_report=Path("fast-sync-report.json"),
        )

        self.assertEqual(command[-2:], ["--fast-sync-report", "fast-sync-report.json"])

    def test_node_command_rejects_import_chain_and_fast_sync_together(self):
        module = load_module()

        with self.assertRaisesRegex(ValueError, "cannot combine"):
            module.node_command(
                Path("target/release/neo-node"),
                Path("bounded.toml"),
                100000,
                import_chain=Path("chain.0.100k.acc"),
                fast_sync=True,
            )

    def test_node_command_rejects_fast_sync_cache_without_fast_sync(self):
        module = load_module()

        with self.assertRaisesRegex(ValueError, "requires --fast-sync"):
            module.node_command(
                Path("target/release/neo-node"),
                Path("bounded.toml"),
                100000,
                fast_sync_cache=Path("fast-sync-cache"),
            )

    def test_fast_sync_cache_progress_reports_nested_extracted_chain_file(self):
        module = load_module()

        with tempfile.TemporaryDirectory() as tmp:
            cache_dir = Path(tmp)
            (cache_dir / "chain.0.acc.zip").write_bytes(b"zip")
            extract_dir = cache_dir / "chain.0.acc"
            extract_dir.mkdir()
            (extract_dir / "chain.0.acc").write_bytes(b"extracted-chain")

            progress = module.fast_sync_cache_progress(cache_dir)

        self.assertEqual(progress["fast_sync_stage"], "extracted")
        self.assertEqual(progress["fast_sync_package_path"], "chain.0.acc.zip")
        self.assertEqual(progress["fast_sync_package_bytes"], 3)
        self.assertEqual(progress["fast_sync_chain_path"], "chain.0.acc/chain.0.acc")
        self.assertEqual(progress["fast_sync_chain_bytes"], len(b"extracted-chain"))

    def test_fast_sync_with_stateroot_db_requires_height_match_by_default(self):
        module = load_module()
        original_argv = module.sys.argv
        try:
            module.sys.argv = [
                "run-bounded-mainnet-replay.py",
                "--config",
                "bounded.toml",
                "--target-height",
                "100000",
                "--fast-sync",
                "--stateroot-db",
                "bounded/state-root-334F454E",
            ]
            args = module.parse_args()
        finally:
            module.sys.argv = original_argv

        self.assertTrue(args.require_stateroot_height_match)

    def test_import_chain_with_stateroot_db_requires_height_match_by_default(self):
        module = load_module()
        original_argv = module.sys.argv
        try:
            module.sys.argv = [
                "run-bounded-mainnet-replay.py",
                "--config",
                "bounded.toml",
                "--target-height",
                "100000",
                "--import-chain",
                "chain.0.100k.acc",
                "--stateroot-db",
                "bounded/state-root-334F454E",
            ]
            args = module.parse_args()
        finally:
            module.sys.argv = original_argv

        self.assertTrue(args.require_stateroot_height_match)

    def test_fast_sync_uses_short_poll_interval_when_not_overridden(self):
        module = load_module()
        original_argv = module.sys.argv
        try:
            module.sys.argv = [
                "run-bounded-mainnet-replay.py",
                "--config",
                "bounded.toml",
                "--target-height",
                "100000",
                "--fast-sync",
            ]
            args = module.parse_args()
        finally:
            module.sys.argv = original_argv

        self.assertEqual(args.poll_interval, 1.0)

    def test_fast_sync_keeps_explicit_poll_interval(self):
        module = load_module()
        original_argv = module.sys.argv
        try:
            module.sys.argv = [
                "run-bounded-mainnet-replay.py",
                "--config",
                "bounded.toml",
                "--target-height",
                "100000",
                "--fast-sync",
                "--poll-interval",
                "5",
            ]
            args = module.parse_args()
        finally:
            module.sys.argv = original_argv

        self.assertEqual(args.poll_interval, 5.0)

    def test_parse_args_rejects_sync_speed_floor_below_proof_target(self):
        module = load_module()
        original_argv = module.sys.argv
        try:
            module.sys.argv = [
                "run-bounded-mainnet-replay.py",
                "--config",
                "bounded.toml",
                "--target-height",
                "100000",
                "--sync-speed-floor-bps",
                "1499.99",
            ]
            with self.assertRaises(SystemExit) as raised:
                module.parse_args()
        finally:
            module.sys.argv = original_argv

        self.assertEqual(raised.exception.code, 2)

    def test_parse_args_allows_stronger_sync_speed_floor(self):
        module = load_module()
        original_argv = module.sys.argv
        try:
            module.sys.argv = [
                "run-bounded-mainnet-replay.py",
                "--config",
                "bounded.toml",
                "--target-height",
                "100000",
                "--sync-speed-floor-bps",
                "5000",
            ]
            args = module.parse_args()
        finally:
            module.sys.argv = original_argv

        self.assertEqual(args.sync_speed_floor_bps, 5000.0)

    def test_parse_prometheus_metrics_keeps_hotspot_metrics(self):
        module = load_module()

        metrics = module.parse_prometheus_metrics(
            "\n".join(
                [
                    "# HELP ignored ignored",
                    "neo_sync_avg_total_us 4200",
                    "neo_sync_native_persist_avg_tx_us 1200",
                    'neo_sync_native_contract_hook_avg_us{trigger="onpersist",contract="GasToken",id="-6"} 7100',
                    'neo_sync_native_persist_tx_stage_avg_us{stage="load_execute"} 8100',
                    'neo_sync_neotoken_onpersist_stage_avg_us{stage="compute_committee"} 3300',
                    'neo_sync_neotoken_committee_compute_stage_avg_us{stage="candidate_state_decode"} 2100',
                    'neo_sync_neotoken_committee_candidate_scan_avg_items{kind="eligible_candidates"} 42',
                    'neo_state_service_mpt_apply_stage_avg_us{stage="queue_wait"} 900',
                    'neo_state_service_mpt_apply_stage_avg_us{stage="trie_commit"} 120',
                    'neo_state_service_mpt_apply_avg_items{kind="overlay_entries"} 19',
                    "neo_storage_rocksdb_batch_pending_operations 7",
                    "neo_storage_rocksdb_batch_batches_flushed_total 3",
                    "neo_storage_rocksdb_batch_operations_written_total 900",
                    "neo_storage_rocksdb_batch_bytes_written_total 2048",
                    "neo_storage_rocksdb_batch_flush_timeouts_total 1",
                    "neo_storage_rocksdb_batch_avg_ops_per_flush 300",
                    "neo_storage_rocksdb_batch_avg_bytes_per_flush 682",
                    "neo_storage_rocksdb_batch_avg_flush_duration_ms 11",
                    "neo_storage_rocksdb_batch_max_batch_size 5000",
                    "neo_storage_rocksdb_batch_max_batch_bytes 1048576",
                    "neo_storage_rocksdb_batch_disable_wal 1",
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
            metrics[
                'neo_sync_native_persist_tx_stage_avg_us{stage="load_execute"}'
            ],
            8100.0,
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
            metrics['neo_state_service_mpt_apply_stage_avg_us{stage="queue_wait"}'],
            900.0,
        )
        self.assertEqual(
            metrics['neo_state_service_mpt_apply_avg_items{kind="overlay_entries"}'],
            19.0,
        )
        self.assertEqual(metrics["neo_storage_rocksdb_batch_pending_operations"], 7.0)
        self.assertEqual(metrics["neo_storage_rocksdb_batch_batches_flushed_total"], 3.0)
        self.assertEqual(
            metrics["neo_storage_rocksdb_batch_operations_written_total"],
            900.0,
        )
        self.assertEqual(metrics["neo_storage_rocksdb_batch_bytes_written_total"], 2048.0)
        self.assertEqual(metrics["neo_storage_rocksdb_batch_flush_timeouts_total"], 1.0)
        self.assertEqual(metrics["neo_storage_rocksdb_batch_avg_ops_per_flush"], 300.0)
        self.assertEqual(metrics["neo_storage_rocksdb_batch_avg_bytes_per_flush"], 682.0)
        self.assertEqual(metrics["neo_storage_rocksdb_batch_avg_flush_duration_ms"], 11.0)
        self.assertEqual(metrics["neo_storage_rocksdb_batch_max_batch_size"], 5000.0)
        self.assertEqual(metrics["neo_storage_rocksdb_batch_max_batch_bytes"], 1048576.0)
        self.assertEqual(metrics["neo_storage_rocksdb_batch_disable_wal"], 1.0)
        self.assertEqual(metrics["neo_state_service_mpt_apply_avg_changes"], 17.0)
        self.assertNotIn("neo_node_service_enabled", metrics)

    def test_summarize_metric_samples_reports_hotspots(self):
        module = load_module()

        summary = module.summarize_metric_samples(
            [
                {
                    "metrics": {
                        "neo_sync_avg_total_us": 4000.0,
                        'neo_sync_native_persist_tx_stage_avg_us{stage="load_execute"}': 9000.0,
                        'neo_state_service_mpt_apply_stage_avg_us{stage="trie_commit"}': 7000.0,
                        'neo_state_service_mpt_apply_avg_items{kind="overlay_entries"}': 20.0,
                    }
                },
                {
                    "metrics": {
                        "neo_sync_avg_total_us": 6000.0,
                        'neo_sync_native_persist_tx_stage_avg_us{stage="load_execute"}': 11000.0,
                        'neo_state_service_mpt_apply_stage_avg_us{stage="trie_commit"}': 3000.0,
                        'neo_state_service_mpt_apply_avg_items{kind="overlay_entries"}': 30.0,
                    }
                },
                {"metrics_error": "timeout"},
            ]
        )

        self.assertEqual(summary["sample_count"], 3)
        self.assertEqual(summary["metrics_error_count"], 1)
        self.assertEqual(summary["metrics"]["neo_sync_avg_total_us"]["average"], 5000.0)
        self.assertEqual(
            summary["hot_metrics_by_average_us"][0]["name"],
            'neo_sync_native_persist_tx_stage_avg_us{stage="load_execute"}',
        )
        self.assertEqual(
            summary["hot_count_metrics_by_average"][0]["name"],
            'neo_state_service_mpt_apply_avg_items{kind="overlay_entries"}',
        )

    def test_localhost_rpc_bypasses_environment_proxy(self):
        module = load_module()
        captured = {}

        class FakeNoProxyOpener:
            def open(self, request, timeout=None):
                captured["url"] = request.full_url
                captured["timeout"] = timeout
                return FakeHttpResponse(json.dumps({"result": 100000}))

        def unexpected_urlopen(*_args, **_kwargs):
            raise AssertionError("localhost RPC must not use the process proxy opener")

        original_urlopen = module.urllib.request.urlopen
        original_build_opener = module.urllib.request.build_opener
        try:
            module.urllib.request.urlopen = unexpected_urlopen
            module.urllib.request.build_opener = lambda *_handlers: FakeNoProxyOpener()

            result = module.rpc_call("http://127.0.0.1:21332", "getblockcount")
        finally:
            module.urllib.request.urlopen = original_urlopen
            module.urllib.request.build_opener = original_build_opener

        self.assertEqual(result, 100000)
        self.assertEqual(captured["url"], "http://127.0.0.1:21332")
        self.assertEqual(captured["timeout"], 5.0)

    def test_localhost_metrics_bypasses_environment_proxy(self):
        module = load_module()
        captured = {}

        class FakeNoProxyOpener:
            def open(self, request, timeout=None):
                captured["url"] = request.full_url
                captured["timeout"] = timeout
                return FakeHttpResponse("neo_sync_avg_total_us 4200\n")

        def unexpected_urlopen(*_args, **_kwargs):
            raise AssertionError("localhost metrics must not use the process proxy opener")

        original_urlopen = module.urllib.request.urlopen
        original_build_opener = module.urllib.request.build_opener
        try:
            module.urllib.request.urlopen = unexpected_urlopen
            module.urllib.request.build_opener = lambda *_handlers: FakeNoProxyOpener()

            metrics = module.fetch_prometheus_metrics("http://localhost:21990/metrics")
        finally:
            module.urllib.request.urlopen = original_urlopen
            module.urllib.request.build_opener = original_build_opener

        self.assertEqual(metrics["neo_sync_avg_total_us"], 4200.0)
        self.assertEqual(captured["url"], "http://localhost:21990/metrics")
        self.assertEqual(captured["timeout"], 2.0)

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

    def test_run_until_target_can_require_sync_speed_floor(self):
        module = load_module()
        clock = FakeClock()
        process = FakeProcess()
        heights = iter([0, 10])

        report = module.run_until_target(
            command=["neo-node"],
            rpc_url="http://127.0.0.1:21332",
            target_height=10,
            poll_interval=10,
            max_seconds=100,
            spawner=lambda command, **kwargs: process,
            rpc=lambda *args, **kwargs: next(heights) + 1,
            clock=clock,
            min_blocks_per_second=1000.0,
            metrics_url="http://127.0.0.1:21990/metrics",
            metrics_fetcher=lambda _url: {
                'neo_sync_native_persist_tx_stage_avg_us{stage="load_execute"}': 8100.0
            },
        )

        self.assertEqual(report["status"], "sync-speed-too-slow")
        self.assertEqual(report["sync_speed_floor_blocks_per_second"], 1000.0)
        self.assertLess(report["blocks_per_second"], 1000.0)
        self.assertGreater(report["sync_speed_shortfall_blocks_per_second"], 0.0)
        self.assertFalse(report["sync_speed_band_met"])
        self.assertEqual(
            report["metrics_summary"]["hot_metrics_by_average_us"][0]["name"],
            'neo_sync_native_persist_tx_stage_avg_us{stage="load_execute"}',
        )

    def test_run_until_target_accepts_sync_speed_inside_band(self):
        module = load_module()
        clock = FakeClock()
        process = FakeProcess()
        heights = iter([0, 10])

        report = module.run_until_target(
            command=["neo-node"],
            rpc_url="http://127.0.0.1:21332",
            target_height=10,
            poll_interval=10,
            max_seconds=100,
            spawner=lambda command, **kwargs: process,
            rpc=lambda *args, **kwargs: next(heights) + 1,
            clock=clock,
            min_blocks_per_second=0.5,
            max_blocks_per_second=2.0,
        )

        self.assertEqual(report["status"], "target-reached")
        self.assertEqual(report["sync_speed_floor_blocks_per_second"], 0.5)
        self.assertEqual(report["sync_speed_ceiling_blocks_per_second"], 2.0)
        self.assertEqual(report["blocks_per_second"], 1.0)
        self.assertTrue(report["sync_speed_band_met"])

    def test_run_until_target_rejects_stale_rpc_when_local_db_height_is_behind(self):
        module = load_module()
        clock = FakeClock()
        process = FakeProcess()
        local_heights = iter([100, 100, 100])

        report = module.run_until_target(
            command=["neo-node"],
            rpc_url="http://127.0.0.1:21332",
            target_height=200,
            poll_interval=10,
            max_seconds=20,
            spawner=lambda command, **kwargs: process,
            rpc=lambda *args, **kwargs: 201,
            clock=clock,
            height_reader=lambda: next(local_heights),
        )

        self.assertEqual(report["status"], "timeout")
        self.assertEqual(report["last_height"], 100)
        self.assertTrue(process.terminated)
        self.assertIn("stale_rpc_height", report["height_samples"][0])
        self.assertEqual(report["height_samples"][0]["height_source"], "fallback-confirmation")

    def test_run_until_target_can_require_sync_speed_ceiling(self):
        module = load_module()
        clock = FakeClock()
        process = FakeProcess()
        heights = iter([0, 10])

        report = module.run_until_target(
            command=["neo-node"],
            rpc_url="http://127.0.0.1:21332",
            target_height=10,
            poll_interval=10,
            max_seconds=100,
            spawner=lambda command, **kwargs: process,
            rpc=lambda *args, **kwargs: next(heights) + 1,
            clock=clock,
            max_blocks_per_second=0.5,
        )

        self.assertEqual(report["status"], "sync-speed-too-fast")
        self.assertEqual(report["sync_speed_ceiling_blocks_per_second"], 0.5)
        self.assertGreater(report["blocks_per_second"], 0.5)
        self.assertGreater(report["sync_speed_overage_blocks_per_second"], 0.0)
        self.assertFalse(report["sync_speed_band_met"])

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
        self.assertEqual(
            report["metrics_summary"]["metrics"]["neo_sync_avg_total_us"]["last"],
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

    def test_run_until_target_can_require_metrics_samples(self):
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
            require_metrics_samples=True,
        )

        self.assertEqual(report["status"], "metrics-unavailable")
        self.assertEqual(report["metrics_sample_count"], 0)
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

    def test_clean_process_exit_at_target_waits_for_required_target_readiness(self):
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
            target_ready_reader=lambda: False,
        )

        self.assertEqual(report["status"], "process-exited")
        self.assertEqual(report["returncode"], 0)
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

    def test_fallback_target_waits_for_required_target_readiness(self):
        module = load_module()
        clock = FakeClock()
        process = FakeProcess()
        fallback_heights = iter([499999, 499999])
        ready_values = iter([False, True])

        def fake_rpc(*_args, **_kwargs):
            raise RuntimeError("rpc unavailable")

        report = module.run_until_target(
            command=["neo-node"],
            rpc_url="http://127.0.0.1:21332",
            target_height=499999,
            poll_interval=10,
            max_seconds=100,
            spawner=lambda command, **kwargs: process,
            rpc=fake_rpc,
            clock=clock,
            height_reader=lambda: next(fallback_heights),
            target_ready_reader=lambda: next(ready_values),
        )

        self.assertEqual(report["status"], "target-reached")
        self.assertEqual(report["last_height"], 499999)
        self.assertEqual(len(report["height_samples"]), 2)
        self.assertEqual(clock.sleeps, [10])

    def test_final_fallback_at_target_waits_for_required_target_readiness(self):
        module = load_module()
        clock = FakeClock()
        process = FakeProcess()
        process.returncode = 0
        fallback_heights = iter([0, 100000])

        def fake_rpc(*_args, **_kwargs):
            raise RuntimeError("rpc unavailable")

        def fake_spawn(*_args, **_kwargs):
            clock.sleep(20)
            return process

        report = module.run_until_target(
            command=["neo-node", "--import-chain", "chain.0.100k.acc"],
            rpc_url="http://127.0.0.1:21332",
            target_height=100000,
            poll_interval=10,
            max_seconds=100,
            spawner=fake_spawn,
            rpc=fake_rpc,
            clock=clock,
            height_reader=lambda: next(fallback_heights),
            target_ready_reader=lambda: False,
            sync_source="import-chain",
        )

        self.assertEqual(report["status"], "process-exited")
        self.assertEqual(report["last_height"], 100000)
        self.assertGreater(report["blocks_per_second"], 0.0)

    def test_stateroot_readiness_uses_observed_chain_height_for_import_overshoot(self):
        module = load_module()

        self.assertFalse(
            module.stateroot_covers_observed_chain_height(
                target_height=5549,
                observed_chain_height=148999,
                stateroot_height=113391,
            )
        )
        self.assertTrue(
            module.stateroot_covers_observed_chain_height(
                target_height=5549,
                observed_chain_height=148999,
                stateroot_height=148999,
            )
        )

    def test_process_exit_after_import_uses_fallback_samples_for_rate(self):
        module = load_module()
        clock = FakeClock()
        process = FakeProcess()
        fallback_heights = iter([0, 100000])

        def fake_rpc(*_args, **_kwargs):
            raise RuntimeError("rpc unavailable")

        def fake_spawn(*_args, **_kwargs):
            clock.sleep(20)
            process.returncode = 0
            return process

        report = module.run_until_target(
            command=["neo-node", "--import-chain", "chain.0.100k.acc"],
            rpc_url="http://127.0.0.1:21332",
            target_height=100000,
            poll_interval=10,
            max_seconds=100,
            spawner=fake_spawn,
            rpc=fake_rpc,
            clock=clock,
            height_reader=lambda: next(fallback_heights),
        )

        self.assertEqual(report["status"], "target-reached")
        self.assertEqual(report["last_height"], 100000)
        self.assertEqual(report["height_samples"][0]["height"], 0)
        self.assertEqual(report["height_samples"][1]["height"], 100000)
        self.assertGreaterEqual(report["blocks_per_second"], 5000.0)

    def test_import_samples_fallback_height_before_spawning_for_rate(self):
        module = load_module()
        clock = FakeClock()
        process = FakeProcess()
        started = False

        def fake_rpc(*_args, **_kwargs):
            raise RuntimeError("rpc unavailable")

        def fake_height_reader():
            return 100000 if started else 0

        def fake_spawn(*_args, **_kwargs):
            nonlocal started
            started = True
            clock.sleep(20)
            process.returncode = 0
            return process

        report = module.run_until_target(
            command=["neo-node", "--import-chain", "chain.0.100k.acc"],
            rpc_url="http://127.0.0.1:21332",
            target_height=100000,
            poll_interval=10,
            max_seconds=100,
            spawner=fake_spawn,
            rpc=fake_rpc,
            clock=clock,
            height_reader=fake_height_reader,
            sync_source="import-chain",
        )

        self.assertEqual(report["status"], "target-reached")
        self.assertEqual(report["last_height"], 100000)
        self.assertEqual(report["height_samples"][0]["height"], 0)
        self.assertEqual(report["height_samples"][0]["height_source"], "fallback-initial")
        self.assertEqual(report["height_samples"][1]["height"], 100000)
        self.assertEqual(report["height_samples"][1]["height_source"], "fallback")
        self.assertGreaterEqual(report["blocks_per_second"], 5000.0)

    def test_process_exit_after_import_uses_initial_height_for_rate(self):
        module = load_module()
        clock = FakeClock()
        process = FakeProcess()

        def fake_rpc(*_args, **_kwargs):
            raise RuntimeError("rpc unavailable")

        def fake_spawn(*_args, **_kwargs):
            clock.sleep(20)
            process.returncode = 0
            return process

        report = module.run_until_target(
            command=["neo-node", "--import-chain", "chain.0.100k.acc"],
            rpc_url="http://127.0.0.1:21332",
            target_height=100000,
            poll_interval=10,
            max_seconds=100,
            spawner=fake_spawn,
            rpc=fake_rpc,
            clock=clock,
            height_reader=lambda: 100000,
            initial_height=0,
        )

        self.assertEqual(report["status"], "target-reached")
        self.assertEqual(report["last_height"], 100000)
        self.assertEqual(report["height_samples"][0]["height"], 0)
        self.assertEqual(report["height_samples"][0]["height_source"], "initial")
        self.assertEqual(report["height_samples"][1]["height"], 100000)
        self.assertEqual(report["height_samples"][1]["height_source"], "fallback")
        self.assertGreaterEqual(report["blocks_per_second"], 5000.0)

    def test_process_exit_after_fast_sync_uses_fallback_samples_for_rate_and_source(self):
        module = load_module()
        clock = FakeClock()
        process = FakeProcess()

        def fake_rpc(*_args, **_kwargs):
            raise RuntimeError("rpc unavailable")

        def fake_spawn(*_args, **_kwargs):
            clock.sleep(20)
            process.returncode = 0
            return process

        report = module.run_until_target(
            command=["neo-node", "--fast-sync", "--stop-at-height", "100000"],
            rpc_url="http://127.0.0.1:21332",
            target_height=100000,
            poll_interval=10,
            max_seconds=100,
            spawner=fake_spawn,
            rpc=fake_rpc,
            clock=clock,
            height_reader=lambda: 100000,
            initial_height=0,
            sync_source="fast-sync",
        )

        self.assertEqual(report["status"], "target-reached")
        self.assertEqual(report["sync_source"], "fast-sync")
        self.assertEqual(report["last_height"], 100000)
        self.assertEqual(report["height_samples"][0]["height"], 0)
        self.assertEqual(report["height_samples"][0]["height_source"], "initial")
        self.assertEqual(report["height_samples"][1]["height"], 100000)
        self.assertEqual(report["height_samples"][1]["height_source"], "fallback")
        self.assertGreaterEqual(report["blocks_per_second"], 5000.0)

    def test_run_until_target_emits_structured_fast_sync_proof(self):
        module = load_module()
        clock = FakeClock()
        process = FakeProcess()
        heights = iter([0, 100000])

        report = module.run_until_target(
            command=["neo-node", "--fast-sync", "--stop-at-height", "100000"],
            rpc_url="http://127.0.0.1:21332",
            target_height=100000,
            poll_interval=10,
            max_seconds=100,
            spawner=lambda command, **kwargs: process,
            rpc=lambda *args, **kwargs: next(heights) + 1,
            clock=clock,
            sync_source="fast-sync",
            progress_reader=lambda: {
                "fast_sync_stage": "extracted",
                "fast_sync_package_path": "chain.0.acc.zip",
                "fast_sync_package_bytes": 100,
                "fast_sync_chain_path": "chain.0.acc/chain.0.acc",
                "fast_sync_chain_bytes": 1000,
            },
            min_blocks_per_second=5000.0,
            max_blocks_per_second=20000.0,
        )

        proof = report["sync_proof"]
        self.assertEqual(proof["sync_source"], "fast-sync")
        self.assertEqual(proof["status"], "target-reached")
        self.assertEqual(proof["target_height"], 100000)
        self.assertEqual(proof["initial_height"], 0)
        self.assertEqual(proof["final_height"], 100000)
        self.assertEqual(proof["advanced_blocks"], 100000)
        self.assertEqual(proof["elapsed_seconds"], 10)
        self.assertEqual(proof["average_blocks_per_second"], report["blocks_per_second"])
        self.assertEqual(proof["height_sample_count"], 2)
        self.assertEqual(proof["height_sample_sources"], ["rpc"])
        self.assertEqual(proof["sync_speed_floor_blocks_per_second"], 5000.0)
        self.assertEqual(proof["sync_speed_ceiling_blocks_per_second"], 20000.0)
        self.assertTrue(proof["sync_speed_band_met"])
        self.assertEqual(
            proof["fast_sync_cache"],
            {
                "stage": "extracted",
                "package_path": "chain.0.acc.zip",
                "package_bytes": 100,
                "chain_path": "chain.0.acc/chain.0.acc",
                "chain_bytes": 1000,
            },
        )

    def test_attach_post_probe_report_updates_sync_proof(self):
        module = load_module()
        report = {
            "status": "target-reached",
            "sync_source": "fast-sync",
            "target_height": 10,
            "last_height": 10,
            "elapsed_seconds": 1.0,
            "blocks_per_second": 10.0,
            "height_samples": [
                {"elapsed_seconds": 0.0, "height": 0},
                {"elapsed_seconds": 1.0, "height": 10},
            ],
        }
        original_chain_height = module.read_probe_ledger_height
        original_state_height = module.read_probe_mpt_state_height
        original_state_root = module.read_probe_mpt_state_root
        try:
            module.read_probe_ledger_height = lambda *_args, **_kwargs: 10
            module.read_probe_mpt_state_height = lambda *_args, **_kwargs: 10
            module.read_probe_mpt_state_root = lambda *_args, **_kwargs: "0xroot10"

            updated = module.attach_post_probe_report(
                report,
                chain_db=Path("clean/chain"),
                stateroot_db=Path("clean/state-root-334F454E"),
                probe_bin=Path("target/release/neo-db-probe"),
                require_stateroot_height_match=True,
            )
        finally:
            module.read_probe_ledger_height = original_chain_height
            module.read_probe_mpt_state_height = original_state_height
            module.read_probe_mpt_state_root = original_state_root

        post_probe = updated["sync_proof"]["post_probe"]
        self.assertEqual(post_probe["status_after_post_probe"], "target-reached")
        self.assertTrue(post_probe["stateroot_matches_chain"])
        self.assertEqual(post_probe["chain_height"], 10)
        self.assertEqual(post_probe["stateroot_height"], 10)
        self.assertEqual(post_probe["local_root"], "0xroot10")

    def test_attach_fast_sync_report_merges_package_and_import_window_proof(self):
        module = load_module()
        report = {
            "status": "target-reached",
            "sync_source": "fast-sync",
            "target_height": 100000,
            "last_height": 100000,
            "elapsed_seconds": 20.0,
            "blocks_per_second": 5000.0,
            "height_samples": [
                {"elapsed_seconds": 0.0, "height": 0},
                {"elapsed_seconds": 20.0, "height": 100000},
            ],
        }

        with tempfile.TemporaryDirectory() as tmp:
            proof_path = Path(tmp) / "fast-sync-report.json"
            proof_path.write_text(
                json.dumps(
                    {
                        "package": {
                            "network": "n3mainnet",
                            "url": "https://sync.example/chain.0.acc.zip",
                            "md5": "ABCDEF0123456789ABCDEF0123456789",
                            "start_height": 0,
                            "end_height": 100000,
                            "filename": "chain.0.acc.zip",
                            "zip_path": "fast-sync-cache/chain.0.acc.zip",
                            "chain_path": "fast-sync-cache/chain.0.acc/chain.0.acc",
                        },
                        "import": {
                            "imported_blocks": 100001,
                            "final_height": 100000,
                            "final_hash": "0x01",
                            "elapsed_seconds": 19.5,
                            "average_blocks_per_second": 5128.25,
                            "throughput_status": "above-target",
                        },
                        "hot_metrics": {
                            "state_service_mpt_avg_total_us": 2000,
                            "state_service_mpt_trie_commit_avg_us": 1200,
                            "native_persist_avg_total_us": 3000,
                            "native_persist_tx_hot_stage": "application",
                            "native_persist_tx_hot_stage_avg_us": 1700,
                            "rocksdb_batch_avg_flush_duration_ms": 11,
                            "rocksdb_batch_pending_operations": 19,
                        },
                        "reference": {
                            "endpoint": "http://seed1.neo.org:10332",
                            "block_height": 100000,
                            "block_hash": "0x01",
                            "state_root_height": 100000,
                            "state_root_hash": "0xabc123",
                        },
                    }
                ),
                encoding="utf-8",
            )

            updated = module.attach_fast_sync_report(report, proof_path)

        self.assertEqual(updated["fast_sync_report"]["package"]["network"], "n3mainnet")
        self.assertEqual(
            updated["sync_proof"]["fast_sync_import"]["average_blocks_per_second"],
            5128.25,
        )
        self.assertEqual(
            updated["sync_proof"]["fast_sync_import"]["throughput_status"],
            "above-target",
        )
        hot = updated["sync_proof"]["fast_sync_hot_metrics"]
        self.assertEqual(hot["state_service_mpt_avg_total_us"], 2000)
        self.assertEqual(hot["state_service_mpt_trie_commit_avg_us"], 1200)
        self.assertEqual(hot["native_persist_avg_total_us"], 3000)
        self.assertEqual(hot["native_persist_tx_hot_stage"], "application")
        self.assertEqual(hot["native_persist_tx_hot_stage_avg_us"], 1700)
        self.assertEqual(hot["rocksdb_batch_avg_flush_duration_ms"], 11)
        self.assertEqual(hot["rocksdb_batch_pending_operations"], 19)
        reference = updated["sync_proof"]["fast_sync_reference"]
        self.assertEqual(reference["endpoint"], "http://seed1.neo.org:10332")
        self.assertEqual(reference["block_height"], 100000)
        self.assertEqual(reference["block_hash"], "0x01")
        self.assertEqual(reference["state_root_height"], 100000)
        self.assertEqual(reference["state_root_hash"], "0xabc123")

    def test_attach_fast_sync_report_fails_fast_sync_without_sidecar(self):
        module = load_module()
        report = {
            "status": "target-reached",
            "sync_source": "fast-sync",
            "target_height": 100000,
            "last_height": 100000,
            "height_samples": [],
        }

        updated = module.attach_fast_sync_report(report, Path("missing-fast-sync-report.json"))

        self.assertEqual(updated["status"], "fast-sync-report-missing")
        self.assertIn("fast_sync_report_error", updated)
        self.assertEqual(
            updated["sync_proof"]["status"],
            "fast-sync-report-missing",
        )

    def test_attach_fast_sync_report_fails_fast_sync_with_invalid_sidecar(self):
        module = load_module()
        report = {
            "status": "target-reached",
            "sync_source": "fast-sync",
            "target_height": 100000,
            "last_height": 100000,
            "height_samples": [],
        }

        with tempfile.TemporaryDirectory() as tmp:
            proof_path = Path(tmp) / "fast-sync-report.json"
            proof_path.write_text("[]", encoding="utf-8")
            updated = module.attach_fast_sync_report(report, proof_path)

        self.assertEqual(updated["status"], "fast-sync-report-invalid")
        self.assertEqual(
            updated["fast_sync_report_error"],
            "fast-sync report is not a JSON object",
        )
        self.assertEqual(updated["sync_proof"]["status"], "fast-sync-report-invalid")

    def test_fast_sync_pre_rpc_samples_include_cache_download_progress(self):
        module = load_module()
        clock = FakeClock()
        process = FakeProcess()

        def fake_rpc(*_args, **_kwargs):
            raise RuntimeError("rpc unavailable")

        with tempfile.TemporaryDirectory() as tmp:
            cache_dir = Path(tmp) / "fast-sync-cache"
            cache_dir.mkdir()
            (cache_dir / "chain.0.acc.zip.part").write_bytes(b"x" * 12)

            report = module.run_until_target(
                command=["neo-node", "--fast-sync", "--stop-at-height", "100000"],
                rpc_url="http://127.0.0.1:21332",
                target_height=100000,
                poll_interval=10,
                max_seconds=10,
                spawner=lambda *_args, **_kwargs: process,
                rpc=fake_rpc,
                clock=clock,
                sync_source="fast-sync",
                progress_reader=lambda: module.fast_sync_cache_progress(cache_dir),
            )

        self.assertEqual(report["status"], "timeout")
        sample = report["height_samples"][0]
        self.assertEqual(sample["fast_sync_stage"], "downloading")
        self.assertEqual(sample["fast_sync_partial_bytes"], 12)
        self.assertEqual(sample["fast_sync_partial_path"], "chain.0.acc.zip.part")

    def test_fast_sync_quick_success_keeps_terminal_cache_snapshot(self):
        module = load_module()
        clock = FakeClock()
        process = FakeProcess()
        terminal_snapshot = {
            "fast_sync_stage": "extracted",
            "fast_sync_package_path": "chain.0.acc.zip",
            "fast_sync_package_bytes": 100,
            "fast_sync_chain_path": "chain.0.acc/chain.0.acc",
            "fast_sync_chain_bytes": 1000,
        }

        def fake_spawn(*_args, **_kwargs):
            clock.sleep(1)
            process.returncode = 0
            return process

        report = module.run_until_target(
            command=["neo-node", "--fast-sync", "--stop-at-height", "100000"],
            rpc_url="http://127.0.0.1:21332",
            target_height=100000,
            poll_interval=10,
            max_seconds=100,
            spawner=fake_spawn,
            rpc=lambda *_args, **_kwargs: 100001,
            clock=clock,
            height_reader=lambda: 100000,
            initial_height=0,
            sync_source="fast-sync",
            progress_reader=lambda: terminal_snapshot,
        )

        self.assertEqual(report["status"], "target-reached")
        self.assertEqual(report["height_samples"][-1]["height_source"], "rpc-final")
        self.assertEqual(report["height_samples"][-1]["fast_sync_stage"], "extracted")
        self.assertEqual(
            report["sync_proof"]["fast_sync_cache"],
            {
                "stage": "extracted",
                "package_path": "chain.0.acc.zip",
                "package_bytes": 100,
                "chain_path": "chain.0.acc/chain.0.acc",
                "chain_bytes": 1000,
            },
        )

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
        self.assertEqual(
            captured["command"][captured["command"].index("--storage-provider") + 1],
            "mdbx",
        )
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
        self.assertEqual(
            captured["command"][captured["command"].index("--storage-provider") + 1],
            "mdbx",
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
            module.read_probe_ledger_height = lambda *_args, **_kwargs: 677300
            module.read_probe_mpt_state_height = lambda *_args, **_kwargs: 677300
            module.read_probe_mpt_state_root = lambda *_args, **_kwargs: "0xabc123"
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
            module.read_probe_ledger_height = lambda *_args, **_kwargs: 11
            module.read_probe_mpt_state_height = lambda *_args, **_kwargs: 11
            module.read_probe_mpt_state_root = lambda *_args, **_kwargs: "0xabc123"

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
        self.assertTrue(reference["all_references_succeeded"])
        self.assertEqual(reference["successful_samples"], 2)
        self.assertEqual(reference["reference_roots"], ["0xabc123"])

    def test_reference_stateroots_require_every_configured_reference_to_succeed(self):
        module = load_module()

        def fake_rpc(url, method, params=None, timeout=5.0):
            self.assertEqual(method, "getstateroot")
            self.assertEqual(params, [11])
            if url.endswith("seed1"):
                return {"index": 11, "roothash": "0xabc123"}
            raise RuntimeError("reference unavailable")

        reference = module.fetch_reference_stateroots(
            reference_urls=["http://seed1", "http://seed2"],
            index=11,
            local_root="0xabc123",
            rpc=fake_rpc,
        )

        self.assertFalse(reference["matches_local"])
        self.assertFalse(reference["all_references_succeeded"])
        self.assertEqual(reference["successful_samples"], 1)
        self.assertEqual(reference["sample_count"], 2)
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
                        "successful_samples": len(module.DEFAULT_REFERENCE_RPCS),
                        "sample_count": len(module.DEFAULT_REFERENCE_RPCS),
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

    def test_required_reference_match_rejects_partial_reference_success(self):
        module = load_module()
        report = {"status": "target-reached", "target_height": 10}

        original_collect = module.collect_post_probe
        try:
            module.collect_post_probe = lambda **kwargs: {
                "chain_height": {"ok": True, "height": 10},
                "stateroot_height": {"ok": True, "height": 10},
                "stateroot_matches_chain": True,
                "reference_stateroot": {
                    "matches_local": True,
                    "local_root": "0xabc123",
                    "reference_roots": ["0xabc123"],
                    "successful_samples": 1,
                    "sample_count": 5,
                },
            }
            updated = module.attach_post_probe_report(
                report,
                chain_db=Path("bounded/chain"),
                stateroot_db=Path("bounded/state-root-334F454E"),
                probe_bin=Path("target/release/neo-db-probe"),
                require_stateroot_height_match=False,
                reference_urls=[
                    "http://seed1.neo.org:10332",
                    "http://seed2.neo.org:10332",
                    "http://seed3.neo.org:10332",
                    "http://seed4.neo.org:10332",
                    "http://seed5.neo.org:10332",
                ],
                require_reference_stateroot_match=True,
            )
        finally:
            module.collect_post_probe = original_collect

        self.assertEqual(updated["status"], "reference-stateroot-mismatch")


if __name__ == "__main__":
    unittest.main()
