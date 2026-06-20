import unittest
import tomllib
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]


class DependencyHygieneTests(unittest.TestCase):
    def test_workspace_hyper_dependency_does_not_enable_full_feature(self):
        with (REPO_ROOT / "Cargo.toml").open("rb") as handle:
            cargo = tomllib.load(handle)

        hyper = cargo["workspace"]["dependencies"]["hyper"]
        self.assertNotIn(
            "full",
            hyper.get("features", []),
            "workspace hyper should expose only the features used by the node telemetry server",
        )

    def test_neo_rpc_does_not_reintroduce_warp_transport(self):
        manifest = (REPO_ROOT / "neo-rpc" / "Cargo.toml").read_text(encoding="utf-8")

        self.assertNotIn("warp =", manifest)
        self.assertNotIn("dep:warp", manifest)

    def test_active_neo_rpc_sources_do_not_reference_legacy_warp_transport(self):
        paths = sorted((REPO_ROOT / "neo-rpc" / "src").rglob("*.rs"))
        matches = []
        for path in paths:
            for line_number, line in enumerate(
                path.read_text(encoding="utf-8").splitlines(),
                start=1,
            ):
                if "warp" in line.lower():
                    matches.append(f"{path.relative_to(REPO_ROOT)}:{line_number}: {line.strip()}")

        self.assertEqual(
            matches,
            [],
            "active neo-rpc sources should describe only the jsonrpsee transport path",
        )


if __name__ == "__main__":
    unittest.main()
