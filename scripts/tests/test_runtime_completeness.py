import os
import re
import tomllib
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
EXCLUDED_DIRS = {
    ".git",
    ".idea",
    ".vscode",
    "benchmarks",
    "benches",
    "fuzz",
    "logs",
    "target",
    "tests",
}
PLACEHOLDER_PATTERN = re.compile(r"\b(TODO|FIXME|XXX)\b|todo!\s*\(|unimplemented!\s*\(")
RUST_SOURCE_EXCLUDED_DIRS = {
    ".git",
    ".idea",
    ".vscode",
    "logs",
    "target",
}


def production_rust_sources():
    for root, dirs, files in os.walk(REPO_ROOT):
        dirs[:] = [name for name in dirs if name not in EXCLUDED_DIRS]
        root_path = Path(root)
        if "src" not in root_path.relative_to(REPO_ROOT).parts:
            continue
        for name in files:
            if name.endswith(".rs"):
                yield root_path / name


def rust_source_files():
    for root, dirs, files in os.walk(REPO_ROOT):
        dirs[:] = [name for name in dirs if name not in RUST_SOURCE_EXCLUDED_DIRS]
        for name in files:
            if name.endswith(".rs"):
                yield Path(root) / name


def cargo_manifest_files():
    for path in REPO_ROOT.glob("*/Cargo.toml"):
        if path.parts[-2] not in EXCLUDED_DIRS:
            yield path
    yield REPO_ROOT / "Cargo.toml"


def assert_no_placeholder_markers(test_case, path):
    text = path.read_text(encoding="utf-8")
    match = PLACEHOLDER_PATTERN.search(text)
    if match is not None:
        test_case.fail(
            f"{path.relative_to(REPO_ROOT)} contains placeholder marker {match.group(0)!r}"
        )


class RuntimeCompletenessTests(unittest.TestCase):
    def test_workspace_rust_sources_do_not_contain_placeholder_markers(self):
        paths = sorted(rust_source_files())

        self.assertGreater(len(paths), 500, "expected to scan the Rust workspace")
        for path in paths:
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                assert_no_placeholder_markers(self, path)

    def test_production_sources_do_not_contain_placeholder_implementations(self):
        paths = sorted(production_rust_sources())

        self.assertGreater(len(paths), 400, "expected to scan production Rust sources")
        for path in paths:
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                assert_no_placeholder_markers(self, path)

    def test_cargo_manifests_do_not_contain_placeholder_markers(self):
        paths = sorted(cargo_manifest_files())

        self.assertGreater(len(paths), 20, "expected to scan workspace Cargo manifests")
        for path in paths:
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                assert_no_placeholder_markers(self, path)

    def test_workspace_tokio_dependency_does_not_enable_full_feature(self):
        with (REPO_ROOT / "Cargo.toml").open("rb") as handle:
            cargo = tomllib.load(handle)

        tokio = cargo["workspace"]["dependencies"]["tokio"]
        self.assertNotIn(
            "full",
            tokio.get("features", []),
            "workspace tokio should not force every async crate to inherit the full feature set",
        )


if __name__ == "__main__":
    unittest.main()
