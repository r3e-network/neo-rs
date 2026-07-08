import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]


class CoreFileSizeLimitTests(unittest.TestCase):
    def assert_file_limits(self, limits, message):
        for path, max_lines in limits.items():
            with self.subTest(path=path.relative_to(REPO_ROOT)):
                self.assertTrue(
                    path.exists(),
                    f"{path.relative_to(REPO_ROOT)} should exist after splitting focused core modules",
                )
                line_count = len(path.read_text(encoding="utf-8").splitlines())
                self.assertLessEqual(line_count, max_lines, message)

    def test_protocol_settings_keeps_runtime_and_v3101_regressions_split(self):
        self.assert_file_limits(
            {
                REPO_ROOT / "neo-config" / "src" / "settings" / "protocol.rs": 220,
                REPO_ROOT / "neo-config" / "src" / "tests" / "settings" / "protocol.rs": 260,
            },
            "ProtocolSettings should keep Neo N3 v3.10.1 configuration runtime separate from pinned protocol regression tests",
        )

    def test_crypto_signature_keeps_curve_runtime_and_signature_regressions_split(self):
        self.assert_file_limits(
            {
                REPO_ROOT / "neo-crypto" / "src" / "keys" / "signature.rs": 560,
                REPO_ROOT / "neo-crypto" / "src" / "tests" / "keys" / "signature.rs": 220,
            },
            "Signature crypto should keep curve signing/runtime helpers separate from C# parity and NeoFS regression tests",
        )

    def test_primitives_verification_keeps_traits_and_mock_regressions_split(self):
        self.assert_file_limits(
            {
                REPO_ROOT / "neo-primitives" / "src" / "payload" / "verification.rs": 270,
                REPO_ROOT / "neo-primitives" / "src" / "tests" / "payload" / "verification.rs": 470,
            },
            "Verification primitives should keep core witness/context/snapshot traits separate from mock regression tests",
        )

    def test_vm_evaluation_stack_keeps_runtime_and_stack_regressions_split(self):
        self.assert_file_limits(
            {
                REPO_ROOT / "neo-vm" / "src" / "runtime" / "evaluation_stack.rs": 310,
                REPO_ROOT / "neo-vm" / "src" / "tests" / "runtime" / "evaluation_stack.rs": 320,
            },
            "EvaluationStack should keep VM stack runtime operations separate from stack behavior regression tests",
        )

    def test_hsm_pkcs11_keeps_signer_runtime_and_codec_regressions_split(self):
        self.assert_file_limits(
            {
                REPO_ROOT / "neo-hsm" / "src" / "providers" / "pkcs11.rs": 560,
                REPO_ROOT / "neo-hsm" / "src" / "tests" / "providers" / "pkcs11.rs": 90,
            },
            "PKCS#11 HSM signer should keep runtime worker/signing code separate from DER/point/script codec regressions",
        )

    def test_block_validation_keeps_stateless_runtime_and_limit_regressions_split(self):
        self.assert_file_limits(
            {
                REPO_ROOT / "neo-blockchain" / "src" / "pipeline" / "block_validation.rs": 120,
                REPO_ROOT / "neo-blockchain" / "src" / "tests" / "pipeline" / "block_validation.rs": 340,
            },
            "Block validation should keep stateless validation runtime separate from boundary and protocol-limit regression tests",
        )

    def test_payload_block_keeps_serialization_runtime_and_csharp_regressions_split(self):
        self.assert_file_limits(
            {
                REPO_ROOT / "neo-payloads" / "src" / "ledger" / "block.rs": 410,
                REPO_ROOT / "neo-payloads" / "src" / "tests" / "ledger" / "block.rs": 230,
            },
            "Payload Block should keep wire serialization/runtime methods separate from C# parity and Merkle-root regression tests",
        )

    def test_crypto_hash_keeps_hash_runtime_and_vector_regressions_split(self):
        self.assert_file_limits(
            {
                REPO_ROOT / "neo-crypto" / "src" / "hashes" / "hash.rs": 400,
                REPO_ROOT / "neo-crypto" / "src" / "tests" / "hashes" / "hash.rs": 220,
            },
            "Crypto hash should keep hashing/runtime APIs separate from hash-vector and Neo v3.10 HashAlgorithm regression tests",
        )

    def test_consensus_recovery_message_keeps_runtime_and_wire_regressions_split(self):
        self.assert_file_limits(
            {
                REPO_ROOT / "neo-consensus" / "src" / "messages" / "recovery.rs": 430,
                REPO_ROOT
                / "neo-consensus"
                / "src"
                / "tests"
                / "messages"
                / "recovery.rs": 180,
            },
            "Consensus recovery messages should keep dBFT recovery runtime separate from C# wire-format and validator regression tests",
        )

    def test_mempool_verification_keeps_admission_runtime_and_policy_regressions_split(self):
        self.assert_file_limits(
            {
                REPO_ROOT / "neo-mempool" / "src" / "admission" / "verification.rs": 500,
                REPO_ROOT / "neo-mempool" / "src" / "tests" / "admission" / "verification.rs": 300,
            },
            "Mempool verification should keep transaction-admission runtime separate from C# policy and fee regression tests",
        )

    def test_vm_script_keeps_parser_runtime_and_instruction_regressions_split(self):
        self.assert_file_limits(
            {
                REPO_ROOT / "neo-vm" / "src" / "types" / "script.rs": 460,
                REPO_ROOT / "neo-vm" / "src" / "tests" / "types" / "script.rs": 120,
            },
            "VM Script should keep bytecode parser/cache runtime separate from instruction validation and jump regression tests",
        )

    def test_payload_witness_keeps_runtime_and_multisig_regressions_split(self):
        self.assert_file_limits(
            {
                REPO_ROOT / "neo-payloads" / "src" / "signing" / "witness.rs": 240,
                REPO_ROOT / "neo-payloads" / "src" / "tests" / "signing" / "witness.rs": 90,
            },
            "Payload Witness should keep witness runtime and serialization separate from boundary and multisig regression tests",
        )

    def test_vm_reference_counter_keeps_runtime_and_lifecycle_regressions_split(self):
        self.assert_file_limits(
            {
                REPO_ROOT / "neo-vm" / "src" / "runtime" / "reference_counter.rs": 240,
                REPO_ROOT / "neo-vm" / "src" / "tests" / "runtime" / "reference_counter.rs": 220,
            },
            "VM ReferenceCounter should keep object-lifecycle runtime separate from stack and zero-reference regression tests",
        )

    def test_storage_key_keeps_runtime_and_csharp_ordering_regressions_split(self):
        self.assert_file_limits(
            {
                REPO_ROOT / "neo-storage" / "src" / "types" / "storage_key.rs": 290,
                REPO_ROOT / "neo-storage" / "src" / "tests" / "types" / "storage_key.rs": 270,
            },
            "StorageKey should keep key construction/runtime separate from C# ordering, encoding, and serde regression tests",
        )

    def test_payload_header_keeps_runtime_and_csharp_hash_regressions_split(self):
        self.assert_file_limits(
            {
                REPO_ROOT / "neo-payloads" / "src" / "ledger" / "header.rs": 480,
                REPO_ROOT / "neo-payloads" / "src" / "tests" / "ledger" / "header.rs": 70,
            },
            "Payload Header should keep wire/runtime serialization separate from C# version and hash regression tests",
        )


if __name__ == "__main__":
    unittest.main()
