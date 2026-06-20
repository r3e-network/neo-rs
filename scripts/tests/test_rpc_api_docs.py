import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
RPC_API_DOC = REPO_ROOT / "docs" / "rpc-api.md"


class RpcApiDocsTests(unittest.TestCase):
    def test_neo_indexer_documents_page_defaults_and_caps(self):
        text = RPC_API_DOC.read_text(encoding="utf-8")
        section = text.split("### NeoIndexer", maxsplit=1)[1].split(
            "### Oracle", maxsplit=1
        )[0]

        self.assertIn("default page size is 100", section)
        self.assertIn("maximum page size is 1000", section)

    def test_neo_indexer_documents_exact_sync_semantics(self):
        text = RPC_API_DOC.read_text(encoding="utf-8")
        section = text.split("### NeoIndexer", maxsplit=1)[1].split(
            "### Oracle", maxsplit=1
        )[0]

        self.assertIn(
            "`synced` is true only when `indexedheight` exactly matches `ledgerheight`",
            section,
        )


if __name__ == "__main__":
    unittest.main()
