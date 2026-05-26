use super::CryptoLib;
use crate::error::CoreError as Error;
use crate::error::CoreResult as Result;
use crate::smart_contract::native::method_macros::{
    neo_native_method_dispatch, neo_native_method_metadata,
};
use crate::smart_contract::native::NativeMethod;
use crate::smart_contract::ApplicationEngine;

macro_rules! crypto_method_table {
    ($callback:ident; $($args:tt)*) => {
        $callback! {
            $($args)*
            ;
            {
                safe "recoverSecp256K1", fee = 1 << 15, flags = [], params = [ByteArray, ByteArray], returns = ByteArray, active = HfEchidna, names = ["messageHash", "signature"] => args recover_secp256k1;
                safe "sha256", fee = 1 << 15, flags = [], params = [ByteArray], returns = ByteArray, names = ["data"] => args sha256;
                safe "ripemd160", fee = 1 << 15, flags = [], params = [ByteArray], returns = ByteArray, names = ["data"] => args ripemd160;
                safe "murmur32", fee = 1 << 13, flags = [], params = [ByteArray, Integer], returns = ByteArray, names = ["data", "seed"] => args murmur32;
                safe "keccak256", fee = 1 << 15, flags = [], params = [ByteArray], returns = ByteArray, active = HfCockatrice, names = ["data"] => args keccak256;
                safe "verifyWithECDsa", fee = 1 << 15, flags = [], params = [ByteArray, ByteArray, ByteArray, Integer], returns = Boolean, deprecated = HfCockatrice, names = ["message", "pubkey", "signature", "curve"] => engine verify_with_ecdsa;
                safe "verifyWithECDsa", fee = 1 << 15, flags = [], params = [ByteArray, ByteArray, ByteArray, Integer], returns = Boolean, active = HfCockatrice, names = ["message", "pubkey", "signature", "curveHash"] => engine verify_with_ecdsa;
                safe "verifyWithEd25519", fee = 1 << 15, flags = [], params = [ByteArray, ByteArray, ByteArray], returns = Boolean, active = HfEchidna, names = ["message", "pubkey", "signature"] => args verify_with_ed25519;
                safe "bls12381Add", fee = 1 << 19, flags = [], params = [InteropInterface, InteropInterface], returns = InteropInterface, names = ["x", "y"] => args bls12381_add;
                safe "bls12381Equal", fee = 1 << 5, flags = [], params = [InteropInterface, InteropInterface], returns = Boolean, names = ["x", "y"] => args bls12381_equal;
                safe "bls12381Mul", fee = 1 << 21, flags = [], params = [InteropInterface, ByteArray, Boolean], returns = InteropInterface, names = ["x", "mul", "neg"] => args bls12381_mul;
                safe "bls12381Pairing", fee = 1 << 23, flags = [], params = [InteropInterface, InteropInterface], returns = InteropInterface, names = ["g1", "g2"] => args bls12381_pairing;
                safe "bls12381Serialize", fee = 1 << 19, flags = [], params = [InteropInterface], returns = ByteArray, names = ["g"] => args bls12381_serialize;
                safe "bls12381Deserialize", fee = 1 << 19, flags = [], params = [ByteArray], returns = InteropInterface, names = ["data"] => args bls12381_deserialize;
            }
        }
    };
}

impl CryptoLib {
    pub(super) fn native_methods() -> Vec<NativeMethod> {
        crypto_method_table!(neo_native_method_metadata;)
    }

    pub(super) fn dispatch_method(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        crypto_method_table!(
            neo_native_method_dispatch;
            self,
            engine,
            method,
            args,
            aliases = [],
            unknown = |method| Error::native_contract(format!("Unknown CryptoLib method: {}", method))
        )
    }
}
