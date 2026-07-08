use std::sync::LazyLock;

use neo_config::Hardfork;
use neo_execution::NativeMethod;
use neo_primitives::ContractParameterType;

use super::CryptoLib;
use crate::support::invoke::{NativeMethodBinding, method_metadata};

// C# `CpuFee = 1 << 15` for sha256 / ripemd160 / keccak256.
const CPU_FEE_HASH: i64 = 1 << 15;

// C# CryptoLib BLS12-381 CpuFees (CryptoLib.BLS12_381.cs).
const CPU_FEE_BLS_SERIALIZE: i64 = 1 << 19;
const CPU_FEE_BLS_EQUAL: i64 = 1 << 5;
const CPU_FEE_BLS_ADD: i64 = 1 << 19;
const CPU_FEE_BLS_MUL: i64 = 1 << 21;
const CPU_FEE_BLS_PAIRING: i64 = 1 << 23;

pub(super) static CRYPTO_LIB_METHOD_BINDINGS: LazyLock<Vec<NativeMethodBinding<CryptoLib>>> =
    LazyLock::new(|| {
        let byte_array = ContractParameterType::ByteArray;
        let interop = ContractParameterType::InteropInterface;
        vec![
            // Unconditional since genesis.
            NativeMethodBinding::new(
                NativeMethod::new(
                    "sha256",
                    CPU_FEE_HASH,
                    true,
                    0,
                    vec![byte_array],
                    byte_array,
                )
                .with_parameter_names(["data"]),
                CryptoLib::invoke_sha256,
            ),
            NativeMethodBinding::new(
                NativeMethod::new(
                    "ripemd160",
                    CPU_FEE_HASH,
                    true,
                    0,
                    vec![byte_array],
                    byte_array,
                )
                .with_parameter_names(["data"]),
                CryptoLib::invoke_ripemd160,
            ),
            // Activated by the Cockatrice hardfork
            // (C# `[ContractMethod(Hardfork.HF_Cockatrice, ...)]`).
            NativeMethodBinding::new(
                NativeMethod::new(
                    "keccak256",
                    CPU_FEE_HASH,
                    true,
                    0,
                    vec![byte_array],
                    byte_array,
                )
                .with_active_in(Hardfork::HfCockatrice)
                .with_parameter_names(["data"]),
                CryptoLib::invoke_keccak256,
            ),
            // murmur32(data: ByteArray, seed: Integer) -> ByteArray, C# CpuFee 1<<13.
            NativeMethodBinding::new(
                NativeMethod::new(
                    "murmur32",
                    1 << 13,
                    true,
                    0,
                    vec![byte_array, ContractParameterType::Integer],
                    byte_array,
                )
                .with_parameter_names(["data", "seed"]),
                CryptoLib::invoke_murmur32,
            ),
            // verifyWithECDsa: C# v3.10.1 has three registrations under one name.
            // V2 = ActiveIn HF_Gorgon and calls `Crypto.VerifySignature`, whose
            // wrong-length/invalid-key format errors fault instead of returning false.
            NativeMethodBinding::new(
                NativeMethod::new(
                    "verifyWithECDsa",
                    CPU_FEE_HASH,
                    true,
                    0,
                    vec![
                        byte_array,
                        byte_array,
                        byte_array,
                        ContractParameterType::Integer,
                    ],
                    ContractParameterType::Boolean,
                )
                .with_active_in(Hardfork::HfGorgon)
                .with_parameter_names([
                    "message",
                    "pubkey",
                    "signature",
                    "curveHash",
                ]),
                CryptoLib::invoke_verify_with_ecdsa,
            ),
            // verifyWithEd25519: C# v3.10.1 V1 is ActiveIn HF_Gorgon and faults
            // on wrong-length signature/pubkey; V0 is ActiveIn HF_Echidna and
            // DeprecatedIn HF_Gorgon, returning false for wrong lengths.
            NativeMethodBinding::new(
                NativeMethod::new(
                    "verifyWithEd25519",
                    CPU_FEE_HASH,
                    true,
                    0,
                    vec![byte_array, byte_array, byte_array],
                    ContractParameterType::Boolean,
                )
                .with_active_in(Hardfork::HfGorgon)
                .with_parameter_names(["message", "pubkey", "signature"]),
                CryptoLib::invoke_verify_with_ed25519,
            ),
            NativeMethodBinding::new(
                NativeMethod::new(
                    "verifyWithEd25519",
                    CPU_FEE_HASH,
                    true,
                    0,
                    vec![byte_array, byte_array, byte_array],
                    ContractParameterType::Boolean,
                )
                .with_active_in(Hardfork::HfEchidna)
                .with_deprecated_in(Hardfork::HfGorgon)
                .with_parameter_names(["message", "pubkey", "signature"]),
                CryptoLib::invoke_verify_with_ed25519,
            ),
            // V0 = `[ContractMethod(true, Hardfork.HF_Cockatrice, ...)]`:
            // genesis-active, DeprecatedIn Cockatrice, SHA-256 curves only, and its
            // fourth C# parameter is named `curve`. V1 = ActiveIn HF_Cockatrice,
            // DeprecatedIn HF_Gorgon, adds the Keccak-256 curves, and renames the
            // parameter `curveHash`. Exactly one ECDSA descriptor is active at any
            // height.
            NativeMethodBinding::new(
                NativeMethod::new(
                    "verifyWithECDsa",
                    CPU_FEE_HASH,
                    true,
                    0,
                    vec![
                        byte_array,
                        byte_array,
                        byte_array,
                        ContractParameterType::Integer,
                    ],
                    ContractParameterType::Boolean,
                )
                .with_deprecated_in(Hardfork::HfCockatrice)
                .with_parameter_names(["message", "pubkey", "signature", "curve"]),
                CryptoLib::invoke_verify_with_ecdsa,
            ),
            NativeMethodBinding::new(
                NativeMethod::new(
                    "verifyWithECDsa",
                    CPU_FEE_HASH,
                    true,
                    0,
                    vec![
                        byte_array,
                        byte_array,
                        byte_array,
                        ContractParameterType::Integer,
                    ],
                    ContractParameterType::Boolean,
                )
                .with_active_in(Hardfork::HfCockatrice)
                .with_deprecated_in(Hardfork::HfGorgon)
                .with_parameter_names([
                    "message",
                    "pubkey",
                    "signature",
                    "curveHash",
                ]),
                CryptoLib::invoke_verify_with_ecdsa,
            ),
            // recoverSecp256K1(messageHash, signature) -> ByteArray? (HF_Echidna).
            // Returns the compressed pubkey, or null on failure (signaled at runtime
            // via engine.set_native_return_null()).
            NativeMethodBinding::new(
                NativeMethod::new(
                    "recoverSecp256K1",
                    CPU_FEE_HASH,
                    true,
                    0,
                    vec![byte_array, byte_array],
                    byte_array,
                )
                .with_active_in(Hardfork::HfEchidna)
                .with_parameter_names(["messageHash", "signature"]),
                CryptoLib::invoke_recover_secp256k1,
            ),
            // BLS12-381 operations (genesis-active; CryptoLib.BLS12_381.cs). Points
            // are passed/returned as InteropInterface objects (Bls12381Interop).
            NativeMethodBinding::new(
                NativeMethod::new(
                    "bls12381Serialize",
                    CPU_FEE_BLS_SERIALIZE,
                    true,
                    0,
                    vec![interop],
                    byte_array,
                )
                .with_parameter_names(["g"]),
                CryptoLib::invoke_bls12381_serialize,
            ),
            NativeMethodBinding::new(
                NativeMethod::new(
                    "bls12381Deserialize",
                    CPU_FEE_BLS_SERIALIZE,
                    true,
                    0,
                    vec![byte_array],
                    interop,
                )
                .with_parameter_names(["data"]),
                CryptoLib::invoke_bls12381_deserialize,
            ),
            NativeMethodBinding::new(
                NativeMethod::new(
                    "bls12381Equal",
                    CPU_FEE_BLS_EQUAL,
                    true,
                    0,
                    vec![interop, interop],
                    ContractParameterType::Boolean,
                )
                .with_parameter_names(["x", "y"]),
                CryptoLib::invoke_bls12381_equal,
            ),
            NativeMethodBinding::new(
                NativeMethod::new(
                    "bls12381Add",
                    CPU_FEE_BLS_ADD,
                    true,
                    0,
                    vec![interop, interop],
                    interop,
                )
                .with_parameter_names(["x", "y"]),
                CryptoLib::invoke_bls12381_add,
            ),
            NativeMethodBinding::new(
                NativeMethod::new(
                    "bls12381Mul",
                    CPU_FEE_BLS_MUL,
                    true,
                    0,
                    vec![interop, byte_array, ContractParameterType::Boolean],
                    interop,
                )
                .with_parameter_names(["x", "mul", "neg"]),
                CryptoLib::invoke_bls12381_mul,
            ),
            NativeMethodBinding::new(
                NativeMethod::new(
                    "bls12381Pairing",
                    CPU_FEE_BLS_PAIRING,
                    true,
                    0,
                    vec![interop, interop],
                    interop,
                )
                .with_parameter_names(["g1", "g2"]),
                CryptoLib::invoke_bls12381_pairing,
            ),
        ]
    });

pub(super) static CRYPTO_LIB_METHODS: LazyLock<Vec<NativeMethod>> =
    LazyLock::new(|| method_metadata(&CRYPTO_LIB_METHOD_BINDINGS));
