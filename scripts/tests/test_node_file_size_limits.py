import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]


class NodeFileSizeLimitTests(unittest.TestCase):
    def test_node_observability_keeps_transport_payloads_and_health_split(self):
        limits = {
            REPO_ROOT / "neo-node" / "src" / "node" / "observability.rs": 500,
            REPO_ROOT / "neo-node" / "src" / "node" / "observability" / "endpoints.rs": 500,
            REPO_ROOT / "neo-node" / "src" / "node" / "observability" / "health.rs": 500,
            REPO_ROOT / "neo-node" / "src" / "node" / "observability" / "payloads.rs": 500,
            REPO_ROOT / "neo-node" / "src" / "node" / "observability" / "tests.rs": 80,
            REPO_ROOT
            / "neo-node"
            / "src"
            / "node"
            / "observability"
            / "tests"
            / "endpoints.rs": 240,
            REPO_ROOT
            / "neo-node"
            / "src"
            / "node"
            / "observability"
            / "tests"
            / "health.rs": 60,
            REPO_ROOT
            / "neo-node"
            / "src"
            / "node"
            / "observability"
            / "tests"
            / "health"
            / "payload.rs": 90,
            REPO_ROOT
            / "neo-node"
            / "src"
            / "node"
            / "observability"
            / "tests"
            / "health"
            / "support.rs": 50,
            REPO_ROOT
            / "neo-node"
            / "src"
            / "node"
            / "observability"
            / "tests"
            / "payloads.rs": 260,
            REPO_ROOT
            / "neo-node"
            / "src"
            / "node"
            / "observability"
            / "tests"
            / "runtime_config.rs": 80,
            REPO_ROOT
            / "neo-node"
            / "src"
            / "node"
            / "observability"
            / "tests"
            / "runtime_errors.rs": 120,
        }

        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting observability tests",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    max_lines,
                    "neo-node observability should split HTTP endpoint wiring, payload codecs, health snapshots, and tests",
                )

    def test_node_seed_dialing_keeps_startup_flow_and_runtime_errors_split(self):
        limits = {
            REPO_ROOT / "neo-node" / "src" / "node.rs": 590,
            REPO_ROOT / "neo-node" / "src" / "node" / "seeds.rs": 110,
        }

        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting node seed dialing",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    max_lines,
                    "neo-node seed dialing should keep startup orchestration separate from seed DNS/dial runtime error reporting",
                )

    def test_node_telemetry_keeps_server_exporter_readiness_and_tests_split(self):
        paths = [
            REPO_ROOT / "neo-node" / "src" / "node" / "telemetry.rs",
            REPO_ROOT / "neo-node" / "src" / "node" / "telemetry" / "exporter.rs",
            REPO_ROOT / "neo-node" / "src" / "node" / "telemetry" / "http.rs",
            REPO_ROOT / "neo-node" / "src" / "node" / "telemetry" / "readiness.rs",
            REPO_ROOT / "neo-node" / "src" / "node" / "telemetry" / "tests.rs",
        ]

        for path in paths:
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    500,
                    "neo-node telemetry should split HTTP routing, Prometheus exporter, readiness payloads, and tests",
                )

    def test_node_config_keeps_core_and_service_sections_split(self):
        paths = [
            REPO_ROOT / "neo-node" / "src" / "node" / "config.rs",
            REPO_ROOT / "neo-node" / "src" / "node" / "config" / "services.rs",
        ]

        for path in paths:
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting service-specific node config sections",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    500,
                    "neo-node config should keep core daemon settings separate from service/plugin configuration",
                )

    def test_node_entrypoint_keeps_runtime_context_and_service_factories_split(self):
        limits = {
            REPO_ROOT / "neo-node" / "src" / "node.rs": 650,
            REPO_ROOT / "neo-node" / "src" / "node" / "context.rs": 220,
            REPO_ROOT / "neo-node" / "src" / "node" / "services.rs": 350,
        }

        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting node runtime context and operational service factories",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    max_lines,
                    "neo-node entrypoint should keep runtime callbacks and operational service startup in focused modules",
                )

    def test_node_config_parsing_tests_keep_core_services_and_observability_split(self):
        paths = [
            REPO_ROOT / "neo-node" / "src" / "node" / "tests" / "config_parsing.rs",
            REPO_ROOT / "neo-node" / "src" / "node" / "tests" / "config_parsing" / "services.rs",
            REPO_ROOT
            / "neo-node"
            / "src"
            / "node"
            / "tests"
            / "config_parsing"
            / "observability.rs",
        ]

        for path in paths:
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting node config parsing tests",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    500,
                    "neo-node config parsing tests should split core daemon, service-provider, and observability parsing coverage",
                )

    def test_node_indexer_runtime_keeps_backfill_and_application_log_recovery_split(self):
        limits = {
            REPO_ROOT / "neo-node" / "src" / "node" / "indexer_runtime.rs": 500,
            REPO_ROOT
            / "neo-node"
            / "src"
            / "node"
            / "indexer_runtime"
            / "application_logs.rs": 300,
        }

        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting indexer ApplicationLogs recovery",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    max_lines,
                    "neo-node indexer runtime should keep live/backfill orchestration separate from ApplicationLogs recovery parsing",
                )


if __name__ == "__main__":
    unittest.main()
