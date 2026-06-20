use std::sync::LazyLock;

use neo_config::Hardfork;
use neo_execution::NativeMethod;
use neo_primitives::ContractParameterType;

// C# `CpuFee = 1 << 15` for sha256 / ripemd160 / keccak256.
const CPU_FEE_HASH: i64 = 1 << 15;

// C# CryptoLib BLS12-381 CpuFees (CryptoLib.BLS12_381.cs).
const CPU_FEE_BLS_SERIALIZE: i64 = 1 << 19;
const CPU_FEE_BLS_EQUAL: i64 = 1 << 5;
const CPU_FEE_BLS_ADD: i64 = 1 << 19;
const CPU_FEE_BLS_MUL: i64 = 1 << 21;
const CPU_FEE_BLS_PAIRING: i64 = 1 << 23;

pub(super) static CRYPTO_LIB_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    let byte_array = ContractParameterType::ByteArray;
    let interop = ContractParameterType::InteropInterface;
    vec![
        // Unconditional since genesis.
        NativeMethod::new(
            "sha256",
            CPU_FEE_HASH,
            true,
            0,
            vec![byte_array],
            byte_array,
        )
        .with_parameter_names(["data"]),
        NativeMethod::new(
            "ripemd160",
            CPU_FEE_HASH,
            true,
            0,
            vec![byte_array],
            byte_array,
        )
        .with_parameter_names(["data"]),
        // Activated by the Cockatrice hardfork
        // (C# `[ContractMethod(Hardfork.HF_Cockatrice, ...)]`).
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
        // murmur32(data: ByteArray, seed: Integer) -> ByteArray, C# CpuFee 1<<13.
        NativeMethod::new(
            "murmur32",
            1 << 13,
            true,
            0,
            vec![byte_array, ContractParameterType::Integer],
            byte_array,
        )
        .with_parameter_names(["data", "seed"]),
        // verifyWithEd25519 is a single C# v3.10.0 registration:
        // ActiveIn HF_Echidna, wrong-length sig/pubkey -> false.
        NativeMethod::new(
            "verifyWithEd25519",
            CPU_FEE_HASH,
            true,
            0,
            vec![byte_array, byte_array, byte_array],
            ContractParameterType::Boolean,
        )
        .with_active_in(Hardfork::HfEchidna)
        .with_parameter_names(["message", "pubkey", "signature"]),
        // verifyWithECDsa: dual manifest registration under one name (C# V0/V1).
        // V0 = `[ContractMethod(true, Hardfork.HF_Cockatrice, ...)]`:
        // genesis-active, DeprecatedIn Cockatrice, SHA-256 curves only, and its
        // fourth C# parameter is named `curve`. V1 = ActiveIn HF_Cockatrice,
        // adds the Keccak-256 curves, and renames the parameter `curveHash` -
        // so the manifests differ across the boundary even though the types
        // match. Exactly one is active at any height; the Keccak gate is
        // applied in invoke via the HF_Cockatrice check.
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
        .with_parameter_names(["message", "pubkey", "signature", "curveHash"]),
        // recoverSecp256K1(messageHash, signature) -> ByteArray? (HF_Echidna).
        // Returns the compressed pubkey, or null on failure (signaled at runtime
        // via engine.set_native_return_null()).
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
        // BLS12-381 operations (genesis-active; CryptoLib.BLS12_381.cs). Points
        // are passed/returned as InteropInterface objects (Bls12381Interop).
        NativeMethod::new(
            "bls12381Serialize",
            CPU_FEE_BLS_SERIALIZE,
            true,
            0,
            vec![interop],
            byte_array,
        )
        .with_parameter_names(["g"]),
        NativeMethod::new(
            "bls12381Deserialize",
            CPU_FEE_BLS_SERIALIZE,
            true,
            0,
            vec![byte_array],
            interop,
        )
        .with_parameter_names(["data"]),
        NativeMethod::new(
            "bls12381Equal",
            CPU_FEE_BLS_EQUAL,
            true,
            0,
            vec![interop, interop],
            ContractParameterType::Boolean,
        )
        .with_parameter_names(["x", "y"]),
        NativeMethod::new(
            "bls12381Add",
            CPU_FEE_BLS_ADD,
            true,
            0,
            vec![interop, interop],
            interop,
        )
        .with_parameter_names(["x", "y"]),
        NativeMethod::new(
            "bls12381Mul",
            CPU_FEE_BLS_MUL,
            true,
            0,
            vec![interop, byte_array, ContractParameterType::Boolean],
            interop,
        )
        .with_parameter_names(["x", "mul", "neg"]),
        NativeMethod::new(
            "bls12381Pairing",
            CPU_FEE_BLS_PAIRING,
            true,
            0,
            vec![interop, interop],
            interop,
        )
        .with_parameter_names(["g1", "g2"]),
    ]
});
