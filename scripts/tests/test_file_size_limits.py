import os
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
RUST_SOURCE_EXCLUDED_DIRS = {
    ".git",
    ".idea",
    ".vscode",
    "logs",
    "target",
}


def rust_source_files():
    for root, dirs, files in os.walk(REPO_ROOT):
        dirs[:] = [
            name
            for name in dirs
            if name not in RUST_SOURCE_EXCLUDED_DIRS
        ]
        for name in files:
            if name.endswith(".rs"):
                yield Path(root) / name


class FileSizeLimitTests(unittest.TestCase):
    def test_all_rust_source_files_stay_below_1000_lines(self):
        paths = sorted(rust_source_files())

        self.assertGreater(len(paths), 500, "expected to scan the Rust workspace")
        for path in paths:
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    1000,
                    f"{path.relative_to(REPO_ROOT)} exceeds the repository-wide 1000-line split threshold",
                )

    def test_split_protocol_entrypoints_keep_headroom(self):
        paths = [
            REPO_ROOT / "neo-native-contracts" / "src" / "lib.rs",
            REPO_ROOT / "neo-native-contracts" / "src" / "oracle_contract.rs",
            REPO_ROOT / "neo-native-contracts" / "src" / "policy_contract.rs",
            REPO_ROOT / "neo-native-contracts" / "src" / "policy_contract" / "tests" / "tests.rs",
            REPO_ROOT / "neo-native-contracts" / "src" / "neo_token.rs",
            REPO_ROOT / "neo-blockchain" / "src" / "handlers.rs",
            REPO_ROOT / "neo-crypto" / "src" / "mpt_trie" / "trie.rs",
            REPO_ROOT / "neo-blockchain" / "src" / "native_persist.rs",
            REPO_ROOT / "neo-execution" / "src" / "application_engine" / "state.rs",
            REPO_ROOT / "neo-execution" / "src" / "native_contract.rs",
            REPO_ROOT / "neo-rpc" / "src" / "server" / "rpc_server_wallet" / "mod.rs",
            REPO_ROOT / "neo-rpc" / "src" / "client" / "utility.rs",
            REPO_ROOT / "neo-vm" / "src" / "jump_table" / "compound.rs",
            REPO_ROOT / "neo-network" / "src" / "remote_node" / "session.rs",
            REPO_ROOT / "neo-node" / "src" / "consensus.rs",
            REPO_ROOT / "neo-node" / "src" / "consensus" / "tests.rs",
            REPO_ROOT / "neo-node" / "src" / "node.rs",
            REPO_ROOT / "neo-node" / "src" / "node" / "indexer_runtime.rs",
            REPO_ROOT / "neo-state-service" / "src" / "mpt_store.rs",
        ]

        for path in paths:
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    900,
                    f"{path.relative_to(REPO_ROOT)} should keep enough headroom for reviewable protocol-service changes",
                )

    def test_binary_serializer_keeps_runtime_and_protocol_tests_split(self):
        limits = {
            REPO_ROOT / "neo-serialization" / "src" / "binary_serializer.rs": 650,
            REPO_ROOT / "neo-serialization" / "src" / "binary_serializer" / "tests.rs": 320,
        }

        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting binary serializer protocol tests",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    max_lines,
                    "BinarySerializer should keep C#-compatible runtime codec logic separate from protocol regression tests",
                )

    def test_execution_context_keeps_runtime_and_vm_tests_split(self):
        limits = {
            REPO_ROOT / "neo-vm" / "src" / "execution_context" / "context.rs": 560,
            REPO_ROOT / "neo-vm" / "src" / "execution_context" / "context" / "tests.rs": 380,
        }

        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting VM execution context tests",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    max_lines,
                    "ExecutionContext should keep instruction-pointer and slot runtime logic separate from VM regression tests",
                )

    def test_execution_helper_keeps_runtime_and_witness_regressions_split(self):
        limits = {
            REPO_ROOT / "neo-execution" / "src" / "helper.rs": 470,
            REPO_ROOT / "neo-execution" / "src" / "helper" / "tests.rs": 300,
        }

        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting execution helper witness tests",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    max_lines,
                    "Execution Helper should keep witness verification runtime logic separate from transaction/container regression tests",
                )

    def test_vm_error_keeps_error_model_and_regressions_split(self):
        limits = {
            REPO_ROOT / "neo-vm" / "src" / "error.rs": 760,
            REPO_ROOT / "neo-vm" / "src" / "error" / "tests.rs": 90,
        }

        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting VM error regression tests",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    max_lines,
                    "VmError should keep error taxonomy and conversion logic separate from formatting/classification regression tests",
                )

    def test_consensus_context_keeps_runtime_and_persistence_codec_split(self):
        limits = {
            REPO_ROOT / "neo-consensus" / "src" / "context" / "mod.rs": 780,
            REPO_ROOT / "neo-consensus" / "src" / "context" / "persistence.rs": 120,
        }

        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting consensus crash-recovery persistence codec",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    max_lines,
                    "ConsensusContext should keep round-state runtime logic separate from persisted recovery state codecs",
                )

    def test_data_cache_keeps_runtime_and_storage_regressions_split(self):
        limits = {
            REPO_ROOT / "neo-storage" / "src" / "persistence" / "data_cache" / "cache.rs": 720,
            REPO_ROOT
            / "neo-storage"
            / "src"
            / "persistence"
            / "data_cache"
            / "cache"
            / "tests.rs": 160,
        }

        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting DataCache storage regression tests",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    max_lines,
                    "DataCache should keep storage overlay runtime logic separate from C# compatibility regression tests",
                )

    def test_network_handle_keeps_runtime_and_peer_event_regressions_split(self):
        limits = {
            REPO_ROOT / "neo-network" / "src" / "handle.rs": 540,
            REPO_ROOT / "neo-network" / "src" / "handle" / "tests.rs": 300,
        }

        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting network handle peer event regression tests",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    max_lines,
                    "NetworkHandle should keep runtime command/event APIs separate from peer lifecycle regression tests",
                )

    def test_rpc_wallet_api_tests_keep_query_transfer_and_wait_cases_split(self):
        limits = {
            REPO_ROOT / "neo-rpc" / "src" / "client" / "wallet_api" / "tests.rs": 240,
            REPO_ROOT / "neo-rpc" / "src" / "client" / "wallet_api" / "tests" / "balances.rs": 260,
            REPO_ROOT / "neo-rpc" / "src" / "client" / "wallet_api" / "tests" / "transfers.rs": 360,
            REPO_ROOT / "neo-rpc" / "src" / "client" / "wallet_api" / "tests" / "wait.rs": 160,
        }

        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting wallet API RPC tests by workflow",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    max_lines,
                    "Wallet API RPC tests should keep balance queries, transfer assembly, and transaction waiting in focused modules",
                )

    def test_rpc_client_keeps_transport_chain_token_and_submission_groups_split(self):
        limits = {
            REPO_ROOT / "neo-rpc" / "src" / "client" / "rpc_client" / "client.rs": 280,
            REPO_ROOT / "neo-rpc" / "src" / "client" / "rpc_client" / "blockchain.rs": 380,
            REPO_ROOT / "neo-rpc" / "src" / "client" / "rpc_client" / "tokens.rs": 160,
            REPO_ROOT / "neo-rpc" / "src" / "client" / "rpc_client" / "transactions.rs": 160,
        }

        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting RPC client method groups",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    max_lines,
                    "RpcClient should keep HTTP transport, chain queries, token/contract queries, and transaction submission in focused modules",
                )

    def test_attestation_report_keeps_validation_and_quote_regressions_split(self):
        limits = {
            REPO_ROOT / "neo-tee" / "src" / "attestation" / "report.rs": 660,
            REPO_ROOT / "neo-tee" / "src" / "attestation" / "report" / "tests.rs": 220,
        }

        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting attestation report regression tests",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    max_lines,
                    "AttestationReport should keep quote parsing and validation runtime logic separate from SGX regression tests",
                )

    def test_indexer_entrypoint_keeps_service_headroom(self):
        path = REPO_ROOT / "neo-indexer" / "src" / "indexer.rs"

        line_count = len(path.read_text(encoding="utf-8").splitlines())
        self.assertLessEqual(
            line_count,
            750,
            "neo-indexer/src/indexer.rs should keep write/recovery logic focused by splitting query services into submodules",
        )

    def test_indexer_core_keeps_notification_materialization_split(self):
        limits = {
            REPO_ROOT / "neo-indexer" / "src" / "indexer.rs": 260,
            REPO_ROOT
            / "neo-indexer"
            / "src"
            / "indexer"
            / "notifications.rs": 260,
            REPO_ROOT / "neo-indexer" / "src" / "indexer" / "reorg.rs": 120,
            REPO_ROOT / "neo-indexer" / "src" / "indexer" / "snapshot.rs": 260,
        }

        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting indexer notification materialization",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    max_lines,
                    "neo-indexer core should keep block/transaction indexing separate from snapshot hydration, notification materialization, and reorg cleanup",
                )

    def test_indexer_service_keeps_persistence_and_query_facades_split(self):
        limits = {
            REPO_ROOT / "neo-indexer" / "src" / "service.rs": 460,
            REPO_ROOT / "neo-indexer" / "src" / "service" / "persistence.rs": 300,
            REPO_ROOT / "neo-indexer" / "src" / "service" / "query.rs": 240,
            REPO_ROOT
            / "neo-indexer"
            / "src"
            / "service"
            / "notification_queries.rs": 260,
        }

        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting indexer persistence I/O and query facade",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    max_lines,
                    "neo-indexer service facade should keep durable persistence and read-query routing in focused modules",
                )

    def test_indexer_tests_keep_behavior_groups_reviewable(self):
        paths = [
            REPO_ROOT / "neo-indexer" / "src" / "indexer" / "tests.rs",
            REPO_ROOT / "neo-indexer" / "src" / "indexer" / "tests" / "blocks.rs",
            REPO_ROOT / "neo-indexer" / "src" / "indexer" / "tests" / "notifications.rs",
            REPO_ROOT / "neo-indexer" / "src" / "indexer" / "tests" / "snapshots.rs",
        ]

        for path in paths:
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    500,
                    "neo-indexer indexer tests should keep broad behavior coverage split into focused modules",
                )

    def test_indexer_service_tests_keep_persistence_and_store_cases_split(self):
        paths = [
            REPO_ROOT / "neo-indexer" / "src" / "service" / "tests.rs",
            REPO_ROOT / "neo-indexer" / "src" / "service" / "tests" / "persistence.rs",
            REPO_ROOT / "neo-indexer" / "src" / "service" / "tests" / "store_backed.rs",
        ]

        for path in paths:
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting indexer service tests",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    500,
                    "neo-indexer service tests should split snapshot persistence and store-backed query cases",
                )

    def test_indexer_store_keeps_key_schema_split(self):
        limits = {
            REPO_ROOT / "neo-indexer" / "src" / "store.rs": 160,
            REPO_ROOT / "neo-indexer" / "src" / "store" / "keys.rs": 190,
            REPO_ROOT / "neo-indexer" / "src" / "store" / "records.rs": 230,
            REPO_ROOT / "neo-indexer" / "src" / "store" / "status.rs": 80,
        }

        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting indexer store key schema",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    max_lines,
                    "neo-indexer store should keep durable key schema, record codecs, and status statistics separate from store read/write and migration logic",
                )

    def test_rpc_wallet_entrypoint_keeps_service_headroom(self):
        path = REPO_ROOT / "neo-rpc" / "src" / "server" / "rpc_server_wallet" / "mod.rs"

        line_count = len(path.read_text(encoding="utf-8").splitlines())
        self.assertLessEqual(
            line_count,
            600,
            "neo-rpc wallet RPC entrypoint should split transaction building, signing, and relay helpers into focused modules",
        )

    def test_rpc_wallet_compat_keeps_fee_probes_and_builders_split(self):
        limits = {
            REPO_ROOT / "neo-rpc" / "src" / "server" / "wallet_compat.rs": 180,
            REPO_ROOT / "neo-rpc" / "src" / "server" / "wallet_compat" / "network_fee.rs": 270,
            REPO_ROOT / "neo-rpc" / "src" / "server" / "wallet_compat" / "probes.rs": 180,
            REPO_ROOT / "neo-rpc" / "src" / "server" / "wallet_compat" / "accounts.rs": 160,
            REPO_ROOT
            / "neo-rpc"
            / "src"
            / "server"
            / "wallet_compat"
            / "transaction_builder.rs": 280,
        }

        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting RPC wallet compatibility helpers",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    max_lines,
                    "RPC wallet compatibility should keep network-fee parity, engine probes, and transaction builders separate",
                )

    def test_rpc_server_entrypoint_keeps_transport_policy_headroom(self):
        path = REPO_ROOT / "neo-rpc" / "src" / "server" / "rpc_server.rs"

        line_count = len(path.read_text(encoding="utf-8").splitlines())
        self.assertLessEqual(
            line_count,
            700,
            "neo-rpc server entrypoint should keep HTTP auth/CORS policy separated from server lifecycle and session state",
        )

    def test_native_contract_shared_tests_keep_style_hygiene_split(self):
        limits = {
            REPO_ROOT / "neo-native-contracts" / "src" / "tests.rs": 520,
            REPO_ROOT / "neo-native-contracts" / "src" / "tests" / "style.rs": 520,
            REPO_ROOT / "neo-native-contracts" / "src" / "tests" / "style" / "events.rs": 320,
        }

        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting native-contract style hygiene tests",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    max_lines,
                    "shared native-contract behavior tests should stay separate from source-style hygiene scans",
                )

    def test_policy_storage_keeps_native_contract_storage_groups_split(self):
        path = REPO_ROOT / "neo-native-contracts" / "src" / "policy_contract" / "storage.rs"

        line_count = len(path.read_text(encoding="utf-8").splitlines())
        self.assertLessEqual(
            line_count,
            700,
            "PolicyContract storage should split whitelist and recoverFund committee helpers into focused native-contract storage modules",
        )

    def test_oracle_contract_keeps_storage_and_runtime_groups_split(self):
        path = REPO_ROOT / "neo-native-contracts" / "src" / "oracle_contract.rs"

        line_count = len(path.read_text(encoding="utf-8").splitlines())
        self.assertLessEqual(
            line_count,
            700,
            "OracleContract should split storage codecs/query helpers away from the native-contract runtime entrypoint",
        )

    def test_oracle_contract_keeps_metadata_descriptors_split(self):
        limits = {
            REPO_ROOT / "neo-native-contracts" / "src" / "oracle_contract.rs": 560,
            REPO_ROOT / "neo-native-contracts" / "src" / "oracle_contract" / "metadata.rs": 140,
        }

        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting OracleContract ABI metadata descriptors",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    max_lines,
                    "OracleContract should keep method/event descriptors separate from request/finish/post-persist runtime logic",
                )

    def test_notary_keeps_deposit_storage_and_runtime_groups_split(self):
        path = REPO_ROOT / "neo-native-contracts" / "src" / "notary.rs"

        line_count = len(path.read_text(encoding="utf-8").splitlines())
        self.assertLessEqual(
            line_count,
            700,
            "Notary should split deposit storage codecs and pure deposit decisions away from the native-contract runtime entrypoint",
        )

    def test_notary_keeps_metadata_descriptors_split(self):
        limits = {
            REPO_ROOT / "neo-native-contracts" / "src" / "notary.rs": 500,
            REPO_ROOT / "neo-native-contracts" / "src" / "notary" / "metadata.rs": 120,
        }

        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting Notary ABI metadata descriptors",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    max_lines,
                    "Notary should keep method descriptors separate from deposit, withdraw, verify, and persist runtime logic",
                )

    def test_ledger_contract_keeps_storage_and_wire_helpers_split(self):
        limits = {
            REPO_ROOT / "neo-native-contracts" / "src" / "ledger_contract.rs": 560,
            REPO_ROOT / "neo-native-contracts" / "src" / "ledger_contract" / "storage.rs": 160,
            REPO_ROOT / "neo-native-contracts" / "src" / "ledger_contract" / "wire.rs": 240,
        }

        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting LedgerContract storage and wire helpers",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    max_lines,
                    "LedgerContract runtime should keep storage keys and C# wire codecs in focused modules",
                )

    def test_ledger_contract_keeps_metadata_descriptors_split(self):
        limits = {
            REPO_ROOT / "neo-native-contracts" / "src" / "ledger_contract.rs": 470,
            REPO_ROOT / "neo-native-contracts" / "src" / "ledger_contract" / "metadata.rs": 120,
        }

        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting LedgerContract ABI metadata descriptors",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    max_lines,
                    "LedgerContract should keep method descriptors separate from ledger storage queries and wire encoders",
                )

    def test_role_management_keeps_storage_and_node_list_codecs_split(self):
        limits = {
            REPO_ROOT / "neo-native-contracts" / "src" / "role_management.rs": 560,
            REPO_ROOT / "neo-native-contracts" / "src" / "role_management" / "storage.rs": 180,
            REPO_ROOT / "neo-native-contracts" / "src" / "role_management" / "node_list.rs": 220,
        }

        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting RoleManagement storage and node-list codecs",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    max_lines,
                    "RoleManagement runtime should keep designation storage and C# NodeList codecs in focused modules",
                )

    def test_role_management_keeps_metadata_descriptors_split(self):
        limits = {
            REPO_ROOT / "neo-native-contracts" / "src" / "role_management.rs": 500,
            REPO_ROOT / "neo-native-contracts" / "src" / "role_management" / "metadata.rs": 110,
        }

        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting RoleManagement ABI metadata descriptors",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    max_lines,
                    "RoleManagement should keep method/event descriptors separate from designation lookup and writer runtime logic",
                )

    def test_neo_token_storage_keeps_views_and_storage_logic_split(self):
        path = REPO_ROOT / "neo-native-contracts" / "src" / "neo_token" / "storage.rs"

        line_count = len(path.read_text(encoding="utf-8").splitlines())
        self.assertLessEqual(
            line_count,
            700,
            "NeoToken storage should split stack-value storage views away from storage keys, queries, and governance helpers",
        )

    def test_neo_token_entrypoint_keeps_invoke_dispatch_split(self):
        limits = {
            REPO_ROOT / "neo-native-contracts" / "src" / "neo_token.rs": 560,
            REPO_ROOT / "neo-native-contracts" / "src" / "neo_token" / "invoke.rs": 430,
        }

        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting NeoToken ABI invoke dispatch",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    max_lines,
                    "NeoToken entrypoint should keep native contract lifecycle hooks separate from ABI invoke dispatch",
                )

    def test_gas_token_keeps_metadata_descriptors_split(self):
        limits = {
            REPO_ROOT / "neo-native-contracts" / "src" / "gas_token.rs": 520,
            REPO_ROOT / "neo-native-contracts" / "src" / "gas_token" / "metadata.rs": 120,
        }

        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting GasToken ABI metadata descriptors",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    max_lines,
                    "GasToken should keep NEP-17 metadata descriptors separate from mint/transfer/persist runtime logic",
                )

    def test_neo_token_governance_writer_tests_keep_protocol_scenarios_split(self):
        limits = {
            REPO_ROOT
            / "neo-native-contracts"
            / "src"
            / "neo_token"
            / "tests"
            / "governance_writer_tests.rs": 170,
            REPO_ROOT
            / "neo-native-contracts"
            / "src"
            / "neo_token"
            / "tests"
            / "governance_writer_tests"
            / "candidate_registration.rs": 220,
            REPO_ROOT
            / "neo-native-contracts"
            / "src"
            / "neo_token"
            / "tests"
            / "governance_writer_tests"
            / "voting.rs": 180,
            REPO_ROOT
            / "neo-native-contracts"
            / "src"
            / "neo_token"
            / "tests"
            / "governance_writer_tests"
            / "transfers.rs": 180,
            REPO_ROOT
            / "neo-native-contracts"
            / "src"
            / "neo_token"
            / "tests"
            / "governance_writer_tests"
            / "candidates.rs": 190,
            REPO_ROOT
            / "neo-native-contracts"
            / "src"
            / "neo_token"
            / "tests"
            / "governance_writer_tests"
            / "payments.rs": 280,
        }

        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting NeoToken governance writer protocol tests",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    max_lines,
                    "NeoToken governance writer regressions should stay split by protocol scenario",
                )

    def test_crypto_lib_entrypoint_keeps_dispatch_and_tests_split(self):
        path = REPO_ROOT / "neo-native-contracts" / "src" / "crypto_lib.rs"

        line_count = len(path.read_text(encoding="utf-8").splitlines())
        self.assertLessEqual(
            line_count,
            600,
            "CryptoLib entrypoint should keep ABI dispatch separate from native-contract regression tests",
        )

    def test_crypto_lib_keeps_metadata_descriptors_split(self):
        limits = {
            REPO_ROOT / "neo-native-contracts" / "src" / "crypto_lib.rs": 360,
            REPO_ROOT / "neo-native-contracts" / "src" / "crypto_lib" / "metadata.rs": 220,
        }

        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting CryptoLib ABI metadata descriptors",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    max_lines,
                    "CryptoLib should keep C# method metadata and fee constants separate from hash/signature/BLS runtime dispatch",
                )

    def test_std_lib_keeps_encoding_and_serialization_helpers_split(self):
        limits = {
            REPO_ROOT / "neo-native-contracts" / "src" / "std_lib.rs": 560,
            REPO_ROOT / "neo-native-contracts" / "src" / "std_lib" / "encoding.rs": 220,
            REPO_ROOT / "neo-native-contracts" / "src" / "std_lib" / "serialization.rs": 160,
        }

        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting StdLib encoding and serialization helpers",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    max_lines,
                    "StdLib entrypoint should keep codec and JSON/BinarySerializer helpers in focused modules",
                )

    def test_std_lib_keeps_metadata_descriptors_split(self):
        limits = {
            REPO_ROOT / "neo-native-contracts" / "src" / "std_lib.rs": 440,
            REPO_ROOT / "neo-native-contracts" / "src" / "std_lib" / "metadata.rs": 160,
        }

        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting StdLib ABI metadata descriptors",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    max_lines,
                    "StdLib should keep method descriptors separate from encoding, parsing, memory, and serialization runtime logic",
                )

    def test_treasury_keeps_metadata_descriptors_split(self):
        limits = {
            REPO_ROOT / "neo-native-contracts" / "src" / "treasury.rs": 270,
            REPO_ROOT / "neo-native-contracts" / "src" / "treasury" / "metadata.rs": 80,
        }

        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting Treasury ABI metadata descriptors",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    max_lines,
                    "Treasury should keep method descriptors separate from activation and witness verification runtime logic",
                )

    def test_contract_management_operations_keeps_helpers_and_runtime_paths_split(self):
        path = (
            REPO_ROOT
            / "neo-native-contracts"
            / "src"
            / "contract_management"
            / "operations.rs"
        )

        line_count = len(path.read_text(encoding="utf-8").splitlines())
        self.assertLessEqual(
            line_count,
            450,
            "ContractManagement operations should split storage/query/validation helpers away from deploy/update runtime paths",
        )

    def test_contract_management_keeps_metadata_descriptors_split(self):
        limits = {
            REPO_ROOT / "neo-native-contracts" / "src" / "contract_management.rs": 430,
            REPO_ROOT
            / "neo-native-contracts"
            / "src"
            / "contract_management"
            / "metadata.rs": 220,
        }

        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting ContractManagement ABI metadata descriptors",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    max_lines,
                    "ContractManagement should keep ABI method/event descriptors separate from deploy/update/destroy runtime logic",
                )

    def test_contract_management_deploy_update_tests_keep_protocol_cases_split(self):
        old_path = (
            REPO_ROOT
            / "neo-native-contracts"
            / "src"
            / "contract_management"
            / "tests"
            / "deploy_update_engine_tests.rs"
        )
        base_path = old_path.with_suffix("")
        paths = [
            base_path / "mod.rs",
            base_path / "fixtures.rs",
            base_path / "deploy.rs",
            base_path / "update.rs",
        ]

        self.assertFalse(
            old_path.exists(),
            "ContractManagement deploy/update engine tests should not live in one large module file",
        )
        for path in paths:
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(
                    line_count,
                    500,
                    "ContractManagement deploy/update engine tests should split shared fixtures from deploy and update protocol cases",
                )

if __name__ == "__main__":
    unittest.main()
