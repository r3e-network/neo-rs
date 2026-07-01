import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]


def assert_file_limits(test_case, limits, existence_message, limit_message):
    for relative, max_lines in limits.items():
        path = REPO_ROOT / relative
        with test_case.subTest(path=relative):
            test_case.assertTrue(path.exists(), existence_message.format(path=relative))
            line_count = len(path.read_text(encoding="utf-8").splitlines())
            test_case.assertLessEqual(line_count, max_lines, limit_message)


class RpcFileSizeLimitTests(unittest.TestCase):
    def test_application_logs_service_keeps_rendering_split(self):
        assert_file_limits(
            self,
            {
                "neo-rpc/src/application_logs/service.rs": 520,
                "neo-rpc/src/application_logs/rendering.rs": 190,
                "neo-rpc/src/application_logs/stack_json.rs": 180,
            },
            "{path} should exist after splitting ApplicationLogs JSON rendering",
            "ApplicationLogs service should keep commit/storage orchestration separate from RPC JSON rendering",
        )

    def test_rpc_blockchain_handlers_keep_request_helpers_and_response_rendering_split(self):
        assert_file_limits(
            self,
            {
                "neo-rpc/src/server/rpc_server_blockchain/mod.rs": 520,
                "neo-rpc/src/server/rpc_server_blockchain/request_helpers.rs": 260,
                "neo-rpc/src/server/rpc_server_blockchain/responses.rs": 300,
                "neo-rpc/src/server/rpc_server_blockchain/storage.rs": 170,
            },
            "{path} should exist after splitting blockchain RPC request helpers and response rendering",
            "neo-rpc blockchain handlers should keep protocol method dispatch separate from request parsing and verbose response rendering",
        )

    def test_rpc_state_tests_keep_mpt_and_findstates_coverage_split(self):
        assert_file_limits(
            self,
            {
                "neo-rpc/src/tests/server/handlers/rpc_server_state.rs": 160,
                "neo-rpc/src/tests/server/rpc_server_state/basics.rs": 160,
                "neo-rpc/src/tests/server/rpc_server_state/proof.rs": 180,
                "neo-rpc/src/tests/server/rpc_server_state/mpt_fixture.rs": 160,
                "neo-rpc/src/tests/server/rpc_server_state/state_queries.rs": 180,
                "neo-rpc/src/tests/server/rpc_server_state/state_gates.rs": 120,
                "neo-rpc/src/tests/server/rpc_server_state/find_states.rs": 240,
            },
            "{path} should exist after splitting StateService RPC coverage",
            "neo-rpc StateService tests should keep handler, proof, MPT query, and findstates coverage focused",
        )

    def test_rpc_utility_handlers_keep_inventory_split(self):
        assert_file_limits(
            self,
            {
                "neo-rpc/src/server/rpc_server_utilities/mod.rs": 170,
                "neo-rpc/src/server/rpc_server_utilities/inventory.rs": 40,
                "neo-rpc/src/server/rpc_server_utilities/inventory/plugins.rs": 130,
                "neo-rpc/src/server/rpc_server_utilities/inventory/services.rs": 190,
                "neo-rpc/src/tests/server/handlers/rpc_server_utilities.rs": 260,
            },
            "{path} should exist after splitting utility RPC inventory helpers",
            "neo-rpc utility handlers should keep method dispatch, plugin/service inventory, and tests separate",
        )

    def test_rpc_indexer_tests_keep_records_status_and_errors_split(self):
        assert_file_limits(
            self,
            {
                "neo-rpc/src/tests/server/handlers/rpc_server_indexer.rs": 80,
                "neo-rpc/src/tests/server/rpc_server_indexer/support.rs": 120,
                "neo-rpc/src/tests/server/rpc_server_indexer/records.rs": 300,
                "neo-rpc/src/tests/server/rpc_server_indexer/status.rs": 160,
                "neo-rpc/src/tests/server/rpc_server_indexer/params.rs": 190,
                "neo-rpc/src/tests/server/rpc_server_indexer/errors.rs": 120,
            },
            "{path} should exist after splitting NeoIndexer RPC coverage",
            "neo-rpc NeoIndexer tests should keep indexed record coverage, status reporting, parameter validation, and error handling focused",
        )

    def test_rpc_indexer_handlers_keep_status_params_and_responses_split(self):
        assert_file_limits(
            self,
            {
                "neo-rpc/src/server/rpc_server_indexer/mod.rs": 330,
                "neo-rpc/src/server/rpc_server_indexer/status.rs": 90,
                "neo-rpc/src/server/rpc_server_indexer/params.rs": 160,
                "neo-rpc/src/server/rpc_server_indexer/responses.rs": 120,
            },
            "{path} should exist after splitting NeoIndexer RPC handlers",
            "neo-rpc NeoIndexer handlers should keep method dispatch separate from status reporting, parameter parsing, and response rendering",
        )


if __name__ == "__main__":
    unittest.main()
