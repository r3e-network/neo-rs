import re
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]


class NativeContractStyleTests(unittest.TestCase):
    def test_standard_contract_source_helper_uses_catalog_count(self):
        source = (
            REPO_ROOT
            / "neo-native-contracts"
            / "src"
            / "tests"
            / "style"
            / "mod.rs"
        ).read_text(encoding="utf-8")
        start = source.index("fn standard_contract_sources(")
        signature = source[start : source.index("{", start)]

        self.assertIn(
            "STANDARD_NATIVE_CONTRACT_COUNT",
            signature,
            "standard_contract_sources should use the canonical catalog count instead of duplicating 11",
        )
        self.assertNotIn(
            "; 11",
            signature,
            "standard_contract_sources should not duplicate the native contract count",
        )

    def test_standard_contract_source_helper_includes_all_production_submodules(self):
        src_root = REPO_ROOT / "neo-native-contracts" / "src"
        style_path = src_root / "tests" / "style" / "mod.rs"
        style_source = style_path.read_text(encoding="utf-8")
        included = {
            (style_path.parent / relative_path).resolve().relative_to(src_root).as_posix()
            for relative_path in re.findall(r'include_str!\("([^"]+)"\)', style_source)
        }

        standard_roots = [
            "contract_management",
            "crypto_lib",
            "gas_token",
            "ledger_contract",
            "neo_token",
            "notary",
            "oracle_contract",
            "policy_contract",
            "role_management",
            "std_lib",
            "treasury",
        ]
        expected = set()
        for root_name in standard_roots:
            root_file = src_root / f"{root_name}.rs"
            root_dir = src_root / root_name
            if root_file.exists():
                expected.add(f"{root_name}.rs")
            if root_dir.exists():
                for path in root_dir.rglob("*.rs"):
                    if "tests" in path.parts:
                        continue
                    if (
                        path.name == "test_dispatch.rs"
                        or path.name == "tests.rs"
                        or path.name.endswith("_tests.rs")
                    ):
                        continue
                    expected.add(path.relative_to(src_root).as_posix())

        missing = sorted(expected - included)
        self.assertEqual(
            missing,
            [],
            "standard_contract_sources should scan every standard native contract production module",
        )


if __name__ == "__main__":
    unittest.main()
