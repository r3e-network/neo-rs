import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
CONFIGURATION_DOC = REPO_ROOT / "docs" / "configuration.md"
GETTING_STARTED_DOC = REPO_ROOT / "docs" / "getting-started.md"
OPERATIONS_DOC = REPO_ROOT / "docs" / "operations.md"


class ObservabilityDocsTests(unittest.TestCase):
    def test_heartbeat_payload_documents_indexer_sync_state(self):
        expected = "NeoIndexer readiness/lag/sync"
        for path in [CONFIGURATION_DOC, OPERATIONS_DOC]:
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertIn(expected, path.read_text(encoding="utf-8"))

    def test_observability_secret_env_vars_are_documented(self):
        required = [
            "BETTER_STACK_SOURCE_TOKEN",
            "GOOGLE_ERROR_REPORTING_TOKEN",
            "SENTRY_AUTH_HEADER",
        ]
        for path in [CONFIGURATION_DOC, GETTING_STARTED_DOC, OPERATIONS_DOC]:
            text = environment_variables_section(path)
            for name in required:
                with self.subTest(path=path.relative_to(REPO_ROOT), env=name):
                    self.assertIn(
                        name,
                        text,
                        f"{path.relative_to(REPO_ROOT)} should document {name} for observability provider secrets",
                    )


def environment_variables_section(path):
    text = path.read_text(encoding="utf-8")
    if path == OPERATIONS_DOC:
        return text.split(
            "The image entrypoint reads a few environment variables",
            maxsplit=1,
        )[1].split(
            "For bundled service profiles", maxsplit=1
        )[0]
    if path == GETTING_STARTED_DOC:
        return text.split("Key environment knobs:", maxsplit=1)[1].split(
            "For bundled service profiles", maxsplit=1
        )[0]
    return text.split("## Environment variables", maxsplit=1)[1].split(
        "\n## ", maxsplit=1
    )[0]


if __name__ == "__main__":
    unittest.main()
