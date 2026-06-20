import json
import os
import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
ENTRYPOINT = REPO_ROOT / "scripts" / "docker-entrypoint.sh"


class DockerEntrypointTests(unittest.TestCase):
    def write_stub_node(self, bin_dir, capture_path):
        stub = bin_dir / "neo-node"
        stub.write_text(
            textwrap.dedent(
                f"""\
                #!{sys.executable}
                import json
                import os
                import sys

                with open({str(capture_path)!r}, "w", encoding="utf-8") as output:
                    json.dump({{
                        "args": sys.argv[1:],
                        "neo_rpc_port": os.environ.get("NEO_RPC_PORT"),
                    }}, output)
                """
            ),
            encoding="utf-8",
        )
        stub.chmod(0o755)

    def test_service_profile_selects_network_service_preset(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir)
            etc_neo = root / "etc" / "neo"
            config_dir = etc_neo / "config"
            config_dir.mkdir(parents=True)
            service_config = config_dir / "testnet-service.toml"
            service_config.write_text(
                textwrap.dedent(
                    """
                    [rpc]
                    enabled = true
                    port = 20332
                    """
                ),
                encoding="utf-8",
            )
            (etc_neo / "neo_testnet_node.toml").write_text(
                "[rpc]\nport = 29999\n",
                encoding="utf-8",
            )

            capture_path = root / "neo-node-args.json"
            bin_dir = root / "bin"
            bin_dir.mkdir()
            self.write_stub_node(bin_dir, capture_path)

            env = os.environ.copy()
            env.update(
                {
                    "PATH": f"{bin_dir}{os.pathsep}{env['PATH']}",
                    "NEO_CONFIG_ROOT": str(etc_neo),
                    "NEO_NETWORK": "testnet",
                    "NEO_PROFILE": "service",
                    "NEO_STORAGE": str(root / "data" / "testnet"),
                    "NEO_PLUGINS_DIR": str(root / "plugins"),
                    "NEO_LOGS_DIR": str(root / "logs"),
                }
            )

            result = subprocess.run(
                [str(ENTRYPOINT)],
                env=env,
                text=True,
                capture_output=True,
                check=False,
            )

            self.assertEqual(
                result.returncode,
                0,
                f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}",
            )
            captured = json.loads(capture_path.read_text(encoding="utf-8"))
            runtime_config = Path(captured["args"][1])
            self.assertEqual(
                captured["args"][0],
                "--config",
            )
            self.assertTrue(runtime_config.exists())
            self.assertIn("port = 20332", runtime_config.read_text(encoding="utf-8"))
            self.assertEqual(captured["neo_rpc_port"], "20332")

    def test_service_profile_rebinds_rpc_and_metrics_for_container_ports(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir)
            etc_neo = root / "etc" / "neo"
            config_dir = etc_neo / "config"
            config_dir.mkdir(parents=True)
            service_config = config_dir / "testnet-service.toml"
            service_config.write_text(
                textwrap.dedent(
                    """
                    [rpc]
                    enabled = true
                    port = 20332
                    bind_address = "127.0.0.1"

                    [telemetry.metrics]
                    enabled = true
                    port = 9091
                    bind_address = "127.0.0.1"
                    """
                ),
                encoding="utf-8",
            )

            capture_path = root / "neo-node-args.json"
            bin_dir = root / "bin"
            bin_dir.mkdir()
            self.write_stub_node(bin_dir, capture_path)

            env = os.environ.copy()
            env.update(
                {
                    "PATH": f"{bin_dir}{os.pathsep}{env['PATH']}",
                    "NEO_CONFIG_ROOT": str(etc_neo),
                    "NEO_NETWORK": "testnet",
                    "NEO_PROFILE": "service",
                    "NEO_STORAGE": str(root / "data" / "testnet"),
                    "NEO_PLUGINS_DIR": str(root / "plugins"),
                    "NEO_LOGS_DIR": str(root / "logs"),
                }
            )

            result = subprocess.run(
                [str(ENTRYPOINT)],
                env=env,
                text=True,
                capture_output=True,
                check=False,
            )

            self.assertEqual(
                result.returncode,
                0,
                f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}",
            )
            captured = json.loads(capture_path.read_text(encoding="utf-8"))
            runtime_config = Path(captured["args"][1])
            self.assertNotEqual(runtime_config, service_config)
            text = runtime_config.read_text(encoding="utf-8")
            self.assertIn('bind_address = "0.0.0.0"', text)
            self.assertNotIn('bind_address = "127.0.0.1"', text)

    def test_service_profile_rewrites_service_data_paths_for_container_storage(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir)
            etc_neo = root / "etc" / "neo"
            config_dir = etc_neo / "config"
            config_dir.mkdir(parents=True)
            service_config = config_dir / "testnet-service.toml"
            service_config.write_text(
                textwrap.dedent(
                    """
                    [storage]
                    data_dir = "./data/testnet"

                    [state_service]
                    path = "./data/testnet/state-root-{0}"

                    [indexer]
                    store_path = "./data/testnet/indexer"

                    [application_logs]
                    path = "./data/testnet/application-logs"

                    [tokens_tracker]
                    db_path = "./data/testnet/tokens"

                    [logging]
                    file_path = "./logs/neo-node-testnet-service.log"
                    """
                ),
                encoding="utf-8",
            )

            capture_path = root / "neo-node-args.json"
            bin_dir = root / "bin"
            bin_dir.mkdir()
            self.write_stub_node(bin_dir, capture_path)

            storage = root / "data" / "testnet"
            logs = root / "logs"
            env = os.environ.copy()
            env.update(
                {
                    "PATH": f"{bin_dir}{os.pathsep}{env['PATH']}",
                    "NEO_CONFIG_ROOT": str(etc_neo),
                    "NEO_NETWORK": "testnet",
                    "NEO_PROFILE": "service",
                    "NEO_STORAGE": str(storage),
                    "NEO_PLUGINS_DIR": str(root / "plugins"),
                    "NEO_LOGS_DIR": str(logs),
                }
            )

            result = subprocess.run(
                [str(ENTRYPOINT)],
                env=env,
                text=True,
                capture_output=True,
                check=False,
            )

            self.assertEqual(
                result.returncode,
                0,
                f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}",
            )
            captured = json.loads(capture_path.read_text(encoding="utf-8"))
            runtime_config = Path(captured["args"][1])
            text = runtime_config.read_text(encoding="utf-8")
            self.assertIn(f'data_dir = "{storage}"', text)
            self.assertIn(f'path = "{storage}/state-root-{{0}}"', text)
            self.assertIn(f'store_path = "{storage}/indexer"', text)
            self.assertIn(f'path = "{storage}/application-logs"', text)
            self.assertIn(f'db_path = "{storage}/tokens"', text)
            self.assertIn(f'file_path = "{logs}/neo-node-testnet-service.log"', text)
            self.assertNotIn("./data/testnet", text)
            self.assertNotIn("./logs/", text)

    def test_unknown_profile_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir)
            etc_neo = root / "etc" / "neo"
            etc_neo.mkdir(parents=True)
            (etc_neo / "neo_testnet_node.toml").write_text(
                "[rpc]\nport = 20332\n",
                encoding="utf-8",
            )

            bin_dir = root / "bin"
            bin_dir.mkdir()
            stub = bin_dir / "neo-node"
            stub.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
            stub.chmod(0o755)

            env = os.environ.copy()
            env.update(
                {
                    "PATH": f"{bin_dir}{os.pathsep}{env['PATH']}",
                    "NEO_CONFIG_ROOT": str(etc_neo),
                    "NEO_NETWORK": "testnet",
                    "NEO_PROFILE": "servcie",
                    "NEO_STORAGE": str(root / "data" / "testnet"),
                    "NEO_PLUGINS_DIR": str(root / "plugins"),
                    "NEO_LOGS_DIR": str(root / "logs"),
                }
            )

            result = subprocess.run(
                [str(ENTRYPOINT)],
                env=env,
                text=True,
                capture_output=True,
                check=False,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("unsupported NEO_PROFILE", result.stderr)


if __name__ == "__main__":
    unittest.main()
