import shlex
import tomllib
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
IGNORED_MANIFEST_DIRS = {".git", ".omx", ".opencode", "target"}
ALLOWED_CRATE_ROOT_FILES = {
    ".gitignore",
    "Cargo.lock",
    "Cargo.toml",
    "README.md",
    "build.rs",
}
ALLOWED_CRATE_ROOT_FILES_BY_CRATE = {
    "neo-vm": {"THIRD_PARTY_NOTICES.md"},
}
ALLOWED_CRATE_SRC_ROOT_FILES_BY_CRATE = {
    "neo-vm": {"execution_profile.rs"},
}
MAX_DIRECT_RUST_FILES_PER_SOURCE_DIR = 10
MAX_ENTRY_PATH_SHIMS = 4
ALLOWED_TEST_ROOT_FILES = {"mod.rs"}
RUSTDOC_REQUIRED_SECTIONS = (
    "//! #",
    "//! ## Boundary",
    "//! ## Contents",
)
RUSTDOC_FORBIDDEN_PHRASES = (
    "public types and functions",
    "definitions and helpers",
    "types and helpers",
    "Rpc client entry module",
    "Server entry module",
    "Models entry module",
    "rPC",
    "tLS",
    "webSocket",
)
SHIPPED_NODE_CONFIGS = (
    "config/mainnet.toml",
    "config/mainnet-service.toml",
    "config/mainnet-stateroot.toml",
    "config/testnet.toml",
    "config/testnet-service.toml",
    "neo_mainnet_node.toml",
    "neo_production_node.toml",
    "neo_testnet_node.toml",
)


def crate_manifests() -> list[Path]:
    manifests = []
    for manifest in REPO_ROOT.rglob("Cargo.toml"):
        if manifest.parent == REPO_ROOT:
            continue
        relative_parts = manifest.relative_to(REPO_ROOT).parts
        if any(part in IGNORED_MANIFEST_DIRS for part in relative_parts):
            continue
        manifests.append(manifest)
    return sorted(manifests)


def rustdoc_entry_files() -> list[Path]:
    entries = []
    for manifest in crate_manifests():
        crate_dir = manifest.parent
        for root_name in ("lib.rs", "main.rs"):
            root = crate_dir / "src" / root_name
            if root.exists():
                entries.append(root)

        for source_root_name in ("src", "tests", "benches"):
            source_root = crate_dir / source_root_name
            if not source_root.exists():
                continue
            entries.extend(sorted(source_root.rglob("mod.rs")))

    return sorted(set(entries))


def initial_rustdoc_block(path: Path) -> list[str]:
    lines = path.read_text(encoding="utf-8").splitlines()
    index = 0
    while index < len(lines):
        line = lines[index]
        if not line.strip() or (line.startswith("//") and not line.startswith("//!")):
            index += 1
            continue
        break

    doc = []
    while index < len(lines):
        line = lines[index]
        if line.startswith("//!") or not line.strip():
            doc.append(line)
            index += 1
            continue
        break
    return doc


class RepositoryHygieneTests(unittest.TestCase):
    def test_docker_builder_copies_every_workspace_member(self):
        workspace = tomllib.loads((REPO_ROOT / "Cargo.toml").read_text(encoding="utf-8"))
        required = {Path(member).parts[0] for member in workspace["workspace"]["members"]}

        copied = set()
        for raw_line in (REPO_ROOT / "Dockerfile").read_text(encoding="utf-8").splitlines():
            line = raw_line.strip()
            if not line.startswith("COPY ") or line.startswith("COPY --from="):
                continue
            fields = shlex.split(line)
            for source in fields[1:-1]:
                source_path = Path(source.rstrip("/"))
                if len(source_path.parts) == 1:
                    copied.add(source_path.parts[0])

        self.assertEqual(
            sorted(required - copied),
            [],
            "Docker builder context must copy every workspace member because Cargo loads every member manifest",
        )

    def test_gitignore_excludes_local_test_and_runtime_artifacts(self):
        gitignore = (REPO_ROOT / ".gitignore").read_text(encoding="utf-8")
        required_patterns = [
            "__pycache__/",
            "*.py[cod]",
            "logs/",
            "Logs/",
            "*.log",
        ]

        for pattern in required_patterns:
            with self.subTest(pattern=pattern):
                self.assertIn(
                    pattern,
                    gitignore,
                    f".gitignore should exclude local artifact pattern {pattern}",
                )

    def test_shipped_node_configs_use_mdbx_as_persistent_storage(self):
        offenders = []
        forbidden_storage_keys = {
            "engine",
            "path",
            "cache_size",
            "write_buffer_size",
            "max_open_files",
            "compression",
        }
        required_mdbx_keys = {
            "backend",
            "data_dir",
            "mdbx_geometry_upper_gb",
            "mdbx_geometry_growth_mb",
            "mdbx_max_readers",
        }
        for config in SHIPPED_NODE_CONFIGS:
            path = REPO_ROOT / config
            storage_keys: dict[str, str] = {}
            in_storage = False
            for raw_line in path.read_text(encoding="utf-8").splitlines():
                line = raw_line.split("#", 1)[0].strip()
                if not line:
                    continue
                if line.startswith("[") and line.endswith("]"):
                    in_storage = line.lower() == "[storage]"
                    continue
                if not in_storage or "=" not in line:
                    continue
                key, value = line.split("=", 1)
                storage_keys[key.strip().lower()] = value.strip().strip('"').lower()

            missing = sorted(required_mdbx_keys - storage_keys.keys())
            forbidden = sorted(forbidden_storage_keys & storage_keys.keys())
            if storage_keys.get("backend") != "mdbx" or missing or forbidden:
                offenders.append(
                    f"{config}: backend={storage_keys.get('backend')!r}, "
                    f"missing={missing}, forbidden={forbidden}"
                )

        self.assertEqual(
            offenders,
            [],
            "shipped node presets should use canonical MDBX persistent storage; "
            "removed persistent backends must remain rejected",
        )

    def test_crate_src_roots_only_contain_entry_modules(self):
        cargo_manifests = crate_manifests()

        self.assertGreater(len(cargo_manifests), 20, "expected workspace crates")
        for manifest in cargo_manifests:
            crate_dir = manifest.parent
            src_dir = crate_dir / "src"
            if not src_dir.exists():
                continue

            root_rust_files = sorted(path.name for path in src_dir.glob("*.rs"))
            allowed_root_files = {"lib.rs", "main.rs"} | ALLOWED_CRATE_SRC_ROOT_FILES_BY_CRATE.get(
                crate_dir.name, set()
            )
            implementation_files = [
                name for name in root_rust_files if name not in allowed_root_files
            ]

            with self.subTest(crate=crate_dir.relative_to(REPO_ROOT)):
                self.assertEqual(
                    implementation_files,
                    [],
                    "crate src roots should stay thin; put implementation modules in domain folders",
                )

    def test_source_module_directories_stay_domain_sized(self):
        cargo_manifests = crate_manifests()

        self.assertGreater(len(cargo_manifests), 20, "expected workspace crates")
        offenders = []
        for manifest in cargo_manifests:
            src_dir = manifest.parent / "src"
            if not src_dir.exists():
                continue

            source_dirs = [src_dir]
            source_dirs.extend(path for path in src_dir.rglob("*") if path.is_dir())
            for source_dir in source_dirs:
                direct_rust_files = sorted(path.name for path in source_dir.glob("*.rs"))
                if len(direct_rust_files) <= MAX_DIRECT_RUST_FILES_PER_SOURCE_DIR:
                    continue

                relative_dir = source_dir.relative_to(REPO_ROOT)
                offenders.append(
                    f"{relative_dir}: {len(direct_rust_files)} direct Rust files "
                    f"({', '.join(direct_rust_files)})"
                )

        self.assertEqual(
            offenders,
            [],
            "source module directories should stay domain-sized; split crowded folders into subfolders",
        )

    def test_crate_entry_modules_do_not_recreate_flat_roots_with_path_shims(self):
        cargo_manifests = crate_manifests()

        self.assertGreater(len(cargo_manifests), 20, "expected workspace crates")
        offenders = []
        for manifest in cargo_manifests:
            src_dir = manifest.parent / "src"
            if not src_dir.exists():
                continue

            for entry_name in ("lib.rs", "main.rs"):
                entry = src_dir / entry_name
                if not entry.exists():
                    continue
                source = entry.read_text(encoding="utf-8")
                path_shims = source.count("#[path =")
                if path_shims <= MAX_ENTRY_PATH_SHIMS:
                    continue
                offenders.append(
                    f"{entry.relative_to(REPO_ROOT)}: {path_shims} #[path] shims"
                )

        self.assertEqual(
            offenders,
            [],
            "crate entry modules should stay thin; group moved modules behind folder mod.rs files",
        )

    def test_crate_manifest_roots_only_contain_manifest_and_support_files(self):
        cargo_manifests = crate_manifests()

        self.assertGreater(len(cargo_manifests), 20, "expected workspace crates")
        for manifest in cargo_manifests:
            crate_dir = manifest.parent
            direct_files = sorted(path.name for path in crate_dir.iterdir() if path.is_file())
            allowed_files = ALLOWED_CRATE_ROOT_FILES | ALLOWED_CRATE_ROOT_FILES_BY_CRATE.get(
                crate_dir.name, set()
            )
            unexpected_files = [name for name in direct_files if name not in allowed_files]

            with self.subTest(crate=crate_dir.relative_to(REPO_ROOT)):
                self.assertEqual(
                    unexpected_files,
                    [],
                    "crate roots should stay thin; put source, fixtures, and runtime artifacts in domain folders",
                )

    def test_workspace_test_and_bench_roots_use_domain_folders(self):
        crowded_roots = [
            REPO_ROOT / "tests" / "tests",
            REPO_ROOT / "benches-package" / "benches",
        ]

        for root in crowded_roots:
            with self.subTest(root=root.relative_to(REPO_ROOT)):
                direct_rust_files = sorted(path.name for path in root.glob("*.rs"))
                self.assertEqual(
                    direct_rust_files,
                    [],
                    "integration test and benchmark roots should stay thin; group targets by domain folder",
                )

    def test_crate_integration_test_and_bench_roots_stay_thin(self):
        cargo_manifests = crate_manifests()

        self.assertGreater(len(cargo_manifests), 20, "expected workspace crates")
        for manifest in cargo_manifests:
            crate_dir = manifest.parent
            for root_name in ("tests", "benches"):
                root = crate_dir / root_name
                if not root.exists():
                    continue

                direct_rust_files = {path.name for path in root.glob("*.rs")}
                with self.subTest(root=root.relative_to(REPO_ROOT)):
                    self.assertLessEqual(
                        direct_rust_files,
                        ALLOWED_TEST_ROOT_FILES,
                        "crate integration test and benchmark roots should stay thin; group targets by domain folder",
                    )

    def test_cleaned_crate_test_roots_stay_thin(self):
        crate_test_roots = {
            REPO_ROOT / "neo-blockchain" / "src" / "tests": set(),
            REPO_ROOT / "neo-config" / "src" / "tests": set(),
            REPO_ROOT / "neo-consensus" / "src" / "tests": set(),
            REPO_ROOT / "neo-crypto" / "src" / "tests": {"lib.rs", "mpt_trie.rs"},
            REPO_ROOT / "neo-error" / "src" / "tests": set(),
            REPO_ROOT / "neo-execution" / "src" / "tests": set(),
            REPO_ROOT / "neo-hsm" / "src" / "tests": set(),
            REPO_ROOT / "neo-indexer" / "src" / "tests": {"lib.rs"},
            REPO_ROOT / "neo-io" / "src" / "tests": set(),
            REPO_ROOT / "neo-manifest" / "src" / "tests": set(),
            REPO_ROOT / "neo-mempool" / "src" / "tests": set(),
            REPO_ROOT / "neo-primitives" / "src" / "tests": {"mod.rs"},
            REPO_ROOT / "neo-native-contracts" / "src" / "tests": {
                "lib.rs",
                "test_support.rs",
            },
            REPO_ROOT / "neo-network" / "src" / "tests": set(),
            REPO_ROOT / "neo-node" / "src" / "tests": set(),
            REPO_ROOT / "neo-oracle-service" / "src" / "tests": set(),
            REPO_ROOT / "neo-payloads" / "src" / "tests": set(),
            REPO_ROOT / "neo-rpc" / "src" / "tests": set(),
            REPO_ROOT / "neo-runtime" / "src" / "tests": set(),
            REPO_ROOT / "neo-serialization" / "src" / "tests": set(),
            REPO_ROOT / "neo-state-service" / "src" / "tests": set(),
            REPO_ROOT / "neo-storage" / "src" / "tests": set(),
            REPO_ROOT / "neo-system" / "src" / "tests": set(),
            REPO_ROOT / "neo-vm" / "src" / "tests": set(),
            REPO_ROOT / "neo-wallets" / "src" / "tests": set(),
        }

        for root, allowed_files in crate_test_roots.items():
            with self.subTest(root=root.relative_to(REPO_ROOT)):
                direct_rust_files = {path.name for path in root.glob("*.rs")}
                self.assertLessEqual(
                    direct_rust_files,
                    allowed_files,
                    "crate test roots should keep only harness/support files; group test modules by domain folder",
                )

    def test_crate_and_module_entry_docs_use_standard_shape(self):
        entries = rustdoc_entry_files()

        self.assertGreater(len(entries), 100, "expected crate and module entry files")
        offenders = []
        for entry in entries:
            rustdoc = initial_rustdoc_block(entry)
            if not rustdoc:
                offenders.append(f"{entry.relative_to(REPO_ROOT)}: missing inner rustdoc")
                continue

            if not rustdoc[0].startswith(RUSTDOC_REQUIRED_SECTIONS[0]):
                offenders.append(f"{entry.relative_to(REPO_ROOT)}: missing `//! #` title")
                continue

            for section in RUSTDOC_REQUIRED_SECTIONS[1:]:
                if section not in rustdoc:
                    offenders.append(
                        f"{entry.relative_to(REPO_ROOT)}: missing `{section[4:]}` section"
                    )

        self.assertEqual(
            offenders,
            [],
            "crate roots and module entries should document ownership, boundary, and contents consistently",
        )

    def test_crate_and_module_entry_docs_avoid_placeholder_language(self):
        entries = rustdoc_entry_files()

        self.assertGreater(len(entries), 100, "expected crate and module entry files")
        offenders = []
        for entry in entries:
            rustdoc = "\n".join(initial_rustdoc_block(entry))
            for phrase in RUSTDOC_FORBIDDEN_PHRASES:
                if phrase in rustdoc:
                    offenders.append(f"{entry.relative_to(REPO_ROOT)}: contains `{phrase}`")

        self.assertEqual(
            offenders,
            [],
            "crate/module entry docs should explain concrete ownership instead of placeholder wording",
        )


if __name__ == "__main__":
    unittest.main()
