use super::StdLib;
use crate::error::CoreError as Error;
use crate::error::CoreResult as Result;
use crate::smart_contract::native::method_macros::{
    neo_native_method_dispatch, neo_native_method_metadata,
};
use crate::smart_contract::native::NativeMethod;
use crate::smart_contract::ApplicationEngine;

macro_rules! stdlib_method_table {
    ($callback:ident; $($args:tt)*) => {
        $callback! {
            $($args)*
            ;
            {
                safe "serialize", fee = 1 << 12, flags = [], params = [Any], returns = ByteArray, names = ["item"] => engine serialize;
                safe "deserialize", fee = 1 << 14, flags = [], params = [ByteArray], returns = Any, names = ["data"] => engine deserialize;
                safe "jsonSerialize", fee = 1 << 12, flags = [], params = [Any], returns = ByteArray, names = ["item"] => engine json_serialize;
                safe "jsonDeserialize", fee = 1 << 14, flags = [], params = [ByteArray], returns = Any, names = ["json"] => engine json_deserialize;
                safe "itoa", fee = 1 << 12, flags = [], params = [Integer], returns = String, names = ["value"] => args itoa;
                safe "itoa", fee = 1 << 12, flags = [], params = [Integer, Integer], returns = String, names = ["value", "base"] => args itoa;
                safe "atoi", fee = 1 << 6, flags = [], params = [String], returns = Integer, names = ["value"] => args atoi;
                safe "atoi", fee = 1 << 6, flags = [], params = [String, Integer], returns = Integer, names = ["value", "base"] => args atoi;
                safe "base64Encode", fee = 1 << 5, flags = [], params = [ByteArray], returns = String, names = ["data"] => args base64_encode;
                safe "base64Decode", fee = 1 << 5, flags = [], params = [String], returns = ByteArray, names = ["s"] => args base64_decode;
                safe "base64UrlEncode", fee = 1 << 5, flags = [], params = [String], returns = String, active = HfCockatrice, names = ["data"] => args base64_url_encode;
                safe "base64UrlDecode", fee = 1 << 5, flags = [], params = [String], returns = String, active = HfCockatrice, names = ["s"] => args base64_url_decode;
                safe "base58Encode", fee = 1 << 13, flags = [], params = [ByteArray], returns = String, names = ["data"] => args base58_encode;
                safe "base58Decode", fee = 1 << 10, flags = [], params = [String], returns = ByteArray, names = ["s"] => args base58_decode;
                safe "base58CheckEncode", fee = 1 << 16, flags = [], params = [ByteArray], returns = String, names = ["data"] => args base58_check_encode;
                safe "base58CheckDecode", fee = 1 << 16, flags = [], params = [String], returns = ByteArray, names = ["s"] => args base58_check_decode;
                safe "hexEncode", fee = 1 << 5, flags = [], params = [ByteArray], returns = String, active = HfFaun, names = ["bytes"] => args hex_encode;
                safe "hexDecode", fee = 1 << 5, flags = [], params = [String], returns = ByteArray, active = HfFaun, names = ["str"] => args hex_decode;
                safe "memoryCompare", fee = 1 << 5, flags = [], params = [ByteArray, ByteArray], returns = Integer, names = ["str1", "str2"] => args memory_compare;
                safe "memorySearch", fee = 1 << 6, flags = [], params = [ByteArray, ByteArray], returns = Integer, names = ["mem", "value"] => args memory_search;
                safe "memorySearch", fee = 1 << 6, flags = [], params = [ByteArray, ByteArray, Integer], returns = Integer, names = ["mem", "value", "start"] => args memory_search;
                safe "memorySearch", fee = 1 << 6, flags = [], params = [ByteArray, ByteArray, Integer, Boolean], returns = Integer, names = ["mem", "value", "start", "backward"] => args memory_search;
                safe "stringSplit", fee = 1 << 8, flags = [], params = [String, String], returns = Array, names = ["str", "separator"] => engine string_split;
                safe "stringSplit", fee = 1 << 8, flags = [], params = [String, String, Boolean], returns = Array, names = ["str", "separator", "removeEmptyEntries"] => engine string_split;
                safe "strLen", fee = 1 << 8, flags = [], params = [String], returns = Integer, names = ["str"] => args str_len;
            }
        }
    };
}

impl StdLib {
    pub(super) fn methods() -> Vec<NativeMethod> {
        stdlib_method_table!(neo_native_method_metadata;)
    }

    pub(super) fn dispatch_method(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        stdlib_method_table!(
            neo_native_method_dispatch;
            self,
            engine,
            method,
            args,
            aliases = ["stringLen" => args str_len],
            unknown = |method| Error::native_contract(format!("Unknown method: {}", method))
        )
    }
}
