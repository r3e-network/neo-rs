import unittest
import tomllib
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
VM_REVISION = "3081e83db3716fd51dc58c0afc039290d2d07253"
VM_REPOSITORY = "https://github.com/r3e-network/neo-vm-rs.git"
VM_LOCK_SOURCE = f"git+{VM_REPOSITORY}?rev={VM_REVISION}#{VM_REVISION}"
DEPENDENCY_TABLE_NAMES = ("dependencies", "dev-dependencies", "build-dependencies")


def load_toml(path: Path):
    with path.open("rb") as handle:
        return tomllib.load(handle)


def lock_package(lock: dict, name: str) -> dict:
    matches = [package for package in lock["package"] if package["name"] == name]
    if len(matches) != 1:
        raise AssertionError(f"expected one {name!r} lock entry, found {len(matches)}")
    return matches[0]


def dependency_tables(manifest: dict):
    for table_name in DEPENDENCY_TABLE_NAMES:
        yield manifest.get(table_name, {})
    yield manifest.get("workspace", {}).get("dependencies", {})
    for target in manifest.get("target", {}).values():
        for table_name in DEPENDENCY_TABLE_NAMES:
            yield target.get(table_name, {})


def dependency_package_names(table: dict, workspace_packages: dict[str, str]):
    for alias, specification in table.items():
        if not isinstance(specification, dict):
            yield alias
        elif specification.get("workspace") is True:
            yield specification.get("package", workspace_packages.get(alias, alias))
        else:
            yield specification.get("package", alias)


class DependencyHygieneTests(unittest.TestCase):
    def test_root_and_fuzz_use_the_same_immutable_vm_dependency(self):
        root = load_toml(REPO_ROOT / "Cargo.toml")
        fuzz = load_toml(REPO_ROOT / "fuzz" / "Cargo.toml")

        expected = {
            "git": VM_REPOSITORY,
            "rev": VM_REVISION,
            "version": "0.2.0",
            "default-features": False,
            "features": ["std", "interpreter"],
        }
        self.assertEqual(root["workspace"]["dependencies"]["neo-vm-rs"], expected)
        self.assertEqual(fuzz["dependencies"]["neo-vm-rs"], expected)

    def test_root_and_fuzz_locks_resolve_the_exact_vm_revision(self):
        for relative_path in [Path("Cargo.lock"), Path("fuzz/Cargo.lock")]:
            with self.subTest(lock=relative_path):
                package = lock_package(load_toml(REPO_ROOT / relative_path), "neo-vm-rs")
                self.assertEqual(package["version"], "0.2.0")
                self.assertEqual(package["source"], VM_LOCK_SOURCE)

    def test_root_and_fuzz_declare_the_phase_rust_version(self):
        root = load_toml(REPO_ROOT / "Cargo.toml")
        fuzz = load_toml(REPO_ROOT / "fuzz" / "Cargo.toml")

        self.assertEqual(root["workspace"]["package"]["rust-version"], "1.89")
        self.assertEqual(root["workspace"]["metadata"]["msrv"], "1.89")
        self.assertEqual(fuzz["package"]["rust-version"], "1.89")

    def test_bincode_is_scoped_to_consensus_recovery(self):
        root = load_toml(REPO_ROOT / "Cargo.toml")
        workspace_packages = {
            alias: specification.get("package", alias)
            if isinstance(specification, dict)
            else alias
            for alias, specification in root["workspace"]["dependencies"].items()
        }
        consumers = []
        for manifest_path in REPO_ROOT.rglob("Cargo.toml"):
            if "target" in manifest_path.parts:
                continue
            manifest = load_toml(manifest_path)
            package_names = {
                package
                for table in dependency_tables(manifest)
                for package in dependency_package_names(table, workspace_packages)
            }
            if "bincode" in package_names:
                consumers.append(str(manifest_path.relative_to(REPO_ROOT)))

        self.assertEqual(sorted(consumers), ["Cargo.toml", "neo-consensus/Cargo.toml"])
        self.assertEqual(
            lock_package(load_toml(REPO_ROOT / "Cargo.lock"), "bincode")["version"],
            "1.3.3",
        )
        fuzz_names = {
            package["name"]
            for package in load_toml(REPO_ROOT / "fuzz" / "Cargo.lock")["package"]
        }
        self.assertNotIn("bincode", fuzz_names)

    def test_dependency_package_names_resolve_renamed_dependencies(self):
        table = {
            "direct-alias": {"package": "bincode", "version": "1.3.3"},
            "workspace-alias": {"workspace": True},
        }

        self.assertEqual(
            set(
                dependency_package_names(
                    table,
                    {"workspace-alias": "bincode"},
                )
            ),
            {"bincode"},
        )

    def test_cargo_deny_fails_closed_except_for_reviewed_inputs(self):
        policy = load_toml(REPO_ROOT / "deny.toml")

        self.assertEqual(policy["advisories"]["ignore"], ["RUSTSEC-2025-0141"])
        self.assertEqual(policy["sources"]["unknown-registry"], "deny")
        self.assertEqual(policy["sources"]["unknown-git"], "deny")
        self.assertEqual(policy["sources"]["allow-git"], [VM_REPOSITORY])
        self.assertIn("BSL-1.0", policy["licenses"]["allow"])
        self.assertIn("NCSA", policy["licenses"]["allow"])

    def test_workspace_hyper_dependency_does_not_enable_full_feature(self):
        cargo = load_toml(REPO_ROOT / "Cargo.toml")

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
