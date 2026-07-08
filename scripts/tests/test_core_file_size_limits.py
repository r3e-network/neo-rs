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

    def test_protocol_settings_keeps_runtime_and_v3100_regressions_split(self):
        self.assert_file_limits(
            {
                REPO_ROOT / "neo-config" / "src" / "protocol.rs": 580,
                REPO_ROOT / "neo-config" / "src" / "protocol" / "tests.rs": 220,
            },
            "ProtocolSettings should keep Neo N3 v3.10.1 configuration runtime separate from pinned protocol regression tests",
        )

    def test_crypto_signature_keeps_curve_runtime_and_signature_regressions_split(self):
        self.assert_file_limits(
            {
                REPO_ROOT / "neo-crypto" / "src" / "signature.rs": 580,
                REPO_ROOT / "neo-crypto" / "src" / "signature" / "tests.rs": 240,
            },
            "Signature crypto should keep curve signing/runtime helpers separate from C# parity and NeoFS regression tests",
        )

    def test_primitives_verification_keeps_traits_and_mock_regressions_split(self):
        self.assert_file_limits(
            {
                REPO_ROOT / "neo-primitives" / "src" / "verification.rs": 260,
                REPO_ROOT / "neo-primitives" / "src" / "verification" / "tests.rs": 520,
            },
            "Verification primitives should keep core witness/context/snapshot traits separate from mock regression tests",
        )

    def test_vm_evaluation_stack_keeps_runtime_and_stack_regressions_split(self):
        self.assert_file_limits(
            {
                REPO_ROOT / "neo-vm" / "src" / "evaluation_stack.rs": 300,
                REPO_ROOT / "neo-vm" / "src" / "evaluation_stack" / "tests.rs": 380,
            },
            "EvaluationStack should keep VM stack runtime operations separate from stack behavior regression tests",
        )

    def test_hsm_pkcs11_keeps_signer_runtime_and_codec_regressions_split(self):
        self.assert_file_limits(
            {
                REPO_ROOT / "neo-hsm" / "src" / "pkcs11.rs": 560,
                REPO_ROOT / "neo-hsm" / "src" / "pkcs11" / "tests.rs": 120,
            },
            "PKCS#11 HSM signer should keep runtime worker/signing code separate from DER/point/script codec regressions",
        )

    def test_block_validation_keeps_stateless_runtime_and_limit_regressions_split(self):
        self.assert_file_limits(
            {
                REPO_ROOT / "neo-blockchain" / "src" / "block_validation.rs": 440,
                REPO_ROOT / "neo-blockchain" / "src" / "block_validation" / "tests.rs": 300,
            },
            "Block validation should keep stateless validation runtime separate from boundary and protocol-limit regression tests",
        )

    def test_payload_block_keeps_serialization_runtime_and_csharp_regressions_split(self):
        self.assert_file_limits(
            {
                REPO_ROOT / "neo-payloads" / "src" / "block.rs": 400,
                REPO_ROOT / "neo-payloads" / "src" / "block" / "tests.rs": 230,
            },
            "Payload Block should keep wire serialization/runtime methods separate from C# parity and Merkle-root regression tests",
        )

    def test_crypto_hash_keeps_hash_runtime_and_vector_regressions_split(self):
        self.assert_file_limits(
            {
                REPO_ROOT / "neo-crypto" / "src" / "hash.rs": 390,
                REPO_ROOT / "neo-crypto" / "src" / "hash" / "tests.rs": 230,
            },
            "Crypto hash should keep hashing/runtime APIs separate from hash-vector and Neo v3.10 HashAlgorithm regression tests",
        )

    def test_consensus_recovery_message_keeps_runtime_and_wire_regressions_split(self):
        self.assert_file_limits(
            {
                REPO_ROOT / "neo-consensus" / "src" / "messages" / "recovery.rs": 410,
                REPO_ROOT
                / "neo-consensus"
                / "src"
                / "messages"
                / "recovery"
                / "tests.rs": 180,
            },
            "Consensus recovery messages should keep dBFT recovery runtime separate from C# wire-format and validator regression tests",
        )

    def test_mempool_verification_keeps_admission_runtime_and_policy_regressions_split(self):
        self.assert_file_limits(
            {
                REPO_ROOT / "neo-mempool" / "src" / "verification.rs": 430,
                REPO_ROOT / "neo-mempool" / "src" / "verification" / "tests.rs": 170,
            },
            "Mempool verification should keep transaction-admission runtime separate from C# policy and fee regression tests",
        )

    def test_vm_script_keeps_parser_runtime_and_instruction_regressions_split(self):
        self.assert_file_limits(
            {
                REPO_ROOT / "neo-vm" / "src" / "script.rs": 445,
                REPO_ROOT / "neo-vm" / "src" / "script" / "tests.rs": 130,
            },
            "VM Script should keep bytecode parser/cache runtime separate from instruction validation and jump regression tests",
        )

    def test_payload_witness_keeps_runtime_and_multisig_regressions_split(self):
        self.assert_file_limits(
            {
                REPO_ROOT / "neo-payloads" / "src" / "witness.rs": 410,
                REPO_ROOT / "neo-payloads" / "src" / "witness" / "tests.rs": 140,
            },
            "Payload Witness should keep witness runtime and serialization separate from boundary and multisig regression tests",
        )

    def test_vm_reference_counter_keeps_runtime_and_lifecycle_regressions_split(self):
        self.assert_file_limits(
            {
                REPO_ROOT / "neo-vm" / "src" / "reference_counter.rs": 470,
                REPO_ROOT / "neo-vm" / "src" / "reference_counter" / "tests.rs": 70,
            },
            "VM ReferenceCounter should keep object-lifecycle runtime separate from stack and zero-reference regression tests",
        )

    def test_storage_key_keeps_runtime_and_csharp_ordering_regressions_split(self):
        self.assert_file_limits(
            {
                REPO_ROOT / "neo-storage" / "src" / "types" / "storage_key.rs": 290,
                REPO_ROOT / "neo-storage" / "src" / "types" / "storage_key" / "tests.rs": 260,
            },
            "StorageKey should keep key construction/runtime separate from C# ordering, encoding, and serde regression tests",
        )

    def test_payload_header_keeps_runtime_and_csharp_hash_regressions_split(self):
        self.assert_file_limits(
            {
                REPO_ROOT / "neo-payloads" / "src" / "header.rs": 455,
                REPO_ROOT / "neo-payloads" / "src" / "header" / "tests.rs": 75,
            },
            "Payload Header should keep wire/runtime serialization separate from C# version and hash regression tests",
        )


if __name__ == "__main__":
    unittest.main()
