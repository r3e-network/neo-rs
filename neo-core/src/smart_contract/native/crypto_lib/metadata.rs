use super::CryptoLib;
use crate::smart_contract::native::method_macros::neo_native_methods;
use crate::smart_contract::native::NativeMethod;

impl CryptoLib {
    pub(super) fn native_methods() -> Vec<NativeMethod> {
        neo_native_methods![
            safe "recoverSecp256K1", fee = 1 << 15, flags = [], params = [ByteArray, ByteArray], returns = ByteArray, active = HfEchidna, names = ["messageHash", "signature"];
            safe "sha256", fee = 1 << 15, flags = [], params = [ByteArray], returns = ByteArray, names = ["data"];
            safe "ripemd160", fee = 1 << 15, flags = [], params = [ByteArray], returns = ByteArray, names = ["data"];
            safe "murmur32", fee = 1 << 13, flags = [], params = [ByteArray, Integer], returns = ByteArray, names = ["data", "seed"];
            safe "keccak256", fee = 1 << 15, flags = [], params = [ByteArray], returns = ByteArray, active = HfCockatrice, names = ["data"];
            safe "verifyWithECDsa", fee = 1 << 15, flags = [], params = [ByteArray, ByteArray, ByteArray, Integer], returns = Boolean, deprecated = HfCockatrice, names = ["message", "pubkey", "signature", "curve"];
            safe "verifyWithECDsa", fee = 1 << 15, flags = [], params = [ByteArray, ByteArray, ByteArray, Integer], returns = Boolean, active = HfCockatrice, names = ["message", "pubkey", "signature", "curveHash"];
            safe "verifyWithEd25519", fee = 1 << 15, flags = [], params = [ByteArray, ByteArray, ByteArray], returns = Boolean, active = HfEchidna, names = ["message", "pubkey", "signature"];
            safe "bls12381Add", fee = 1 << 19, flags = [], params = [InteropInterface, InteropInterface], returns = InteropInterface, names = ["x", "y"];
            safe "bls12381Equal", fee = 1 << 5, flags = [], params = [InteropInterface, InteropInterface], returns = Boolean, names = ["x", "y"];
            safe "bls12381Mul", fee = 1 << 21, flags = [], params = [InteropInterface, ByteArray, Boolean], returns = InteropInterface, names = ["x", "mul", "neg"];
            safe "bls12381Pairing", fee = 1 << 23, flags = [], params = [InteropInterface, InteropInterface], returns = InteropInterface, names = ["g1", "g2"];
            safe "bls12381Serialize", fee = 1 << 19, flags = [], params = [InteropInterface], returns = ByteArray, names = ["g"];
            safe "bls12381Deserialize", fee = 1 << 19, flags = [], params = [ByteArray], returns = InteropInterface, names = ["data"];
        ]
    }
}
