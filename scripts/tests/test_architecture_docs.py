import re
import tomllib
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
WORKSPACE_TOML = REPO_ROOT / "Cargo.toml"
ARCHITECTURE_DOC = REPO_ROOT / "docs" / "architecture.md"
DOCS_README = REPO_ROOT / "docs" / "README.md"

CONSOLIDATION_CANDIDATES = {
    "neo-config",
    "neo-error",
    "neo-hsm",
    "neo-indexer",
    "neo-io",
    "neo-manifest",
    "neo-oracle-service",
    "neo-runtime",
    "neo-system",
    "benches-package",
    "tests",
}
SMALL_CRATE_LINE_THRESHOLD = 1_500
ARCHITECTURE_LAYER_NAMES = {
    "Foundation",
    "Infrastructure",
    "Protocol",
    "Domain service",
    "Node service",
    "Composition",
    "Plugin/RPC boundary",
    "Application",
}
DEV_ONLY_MEMBERS = {"tests", "benches-package"}


def workspace_members():
    with WORKSPACE_TOML.open("rb") as handle:
        cargo = tomllib.load(handle)
    return cargo["workspace"]["members"]


def rust_line_count(member):
    crate_dir = REPO_ROOT / member
    return sum(
        len(path.read_text(encoding="utf-8").splitlines())
        for path in crate_dir.rglob("*.rs")
    )


def consolidation_audit_candidate_cells(text):
    audit = text.split("## Crate consolidation audit", maxsplit=1)[-1]
    table = audit.split("The practical rule for future consolidation", maxsplit=1)[0]
    candidates = set()
    for line in table.splitlines():
        if not line.startswith("|"):
            continue
        cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
        if len(cells) < 3 or cells[0] in {"Candidate", "-----------"}:
            continue
        primary_candidate = cells[0].split(" into ", maxsplit=1)[0]
        candidates.update(re.findall(r"`([^`]+)`", primary_candidate))
    return candidates


class ArchitectureDocsTests(unittest.TestCase):
    def test_architecture_doc_states_current_workspace_member_count(self):
        text = ARCHITECTURE_DOC.read_text(encoding="utf-8")
        members = workspace_members()
        production_count = len([member for member in members if member not in DEV_ONLY_MEMBERS])
        dev_count = len([member for member in members if member in DEV_ONLY_MEMBERS])

        self.assertIn(
            f"{production_count} production workspace members plus {dev_count} development-only members",
            text,
            "docs/architecture.md should state the current workspace member count "
            "so crate-count reductions cannot drift silently",
        )

    def test_docs_index_architecture_summary_uses_current_layer_model(self):
        text = DOCS_README.read_text(encoding="utf-8")
        architecture_row = next(
            line
            for line in text.splitlines()
            if line.startswith("| [architecture.md]")
        )

        missing = sorted(
            layer for layer in ARCHITECTURE_LAYER_NAMES if layer not in architecture_row
        )

        self.assertEqual(
            missing,
            [],
            "docs/README.md should summarize the same dependency layers as "
            "docs/architecture.md",
        )

    def test_architecture_reference_covers_every_workspace_member(self):
        text = ARCHITECTURE_DOC.read_text(encoding="utf-8")

        missing = []
        for member in workspace_members():
            table_reference = f"| {member} |"
            inline_reference = f"`{member}`"
            if table_reference not in text and inline_reference not in text:
                missing.append(member)

        self.assertEqual(
            missing,
            [],
            "docs/architecture.md should list every workspace member so crate "
            "boundaries cannot drift silently",
        )

    def test_consolidation_audit_covers_current_small_boundary_candidates(self):
        text = ARCHITECTURE_DOC.read_text(encoding="utf-8")
        audit = text.split("## Crate consolidation audit", maxsplit=1)[-1]

        missing = [
            crate
            for crate in sorted(CONSOLIDATION_CANDIDATES)
            if re.search(rf"`{re.escape(crate)}`|\| {re.escape(crate)}\b", audit)
            is None
        ]

        self.assertEqual(
            missing,
            [],
            "docs/architecture.md should record the merge/keep decision for "
            "small or boundary-looking crates",
        )

    def test_consolidation_audit_covers_all_current_small_crates(self):
        text = ARCHITECTURE_DOC.read_text(encoding="utf-8")
        audited = consolidation_audit_candidate_cells(text)
        small_members = {
            member
            for member in workspace_members()
            if rust_line_count(member) <= SMALL_CRATE_LINE_THRESHOLD
        }

        self.assertEqual(
            sorted(small_members - audited),
            [],
            "docs/architecture.md should record a merge/keep decision for every "
            f"workspace crate at or below {SMALL_CRATE_LINE_THRESHOLD} Rust lines",
        )


if __name__ == "__main__":
    unittest.main()
