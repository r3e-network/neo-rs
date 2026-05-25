use super::StdLib;
use crate::smart_contract::native::method_macros::neo_native_methods;
use crate::smart_contract::native::NativeMethod;

impl StdLib {
    pub(super) fn methods() -> Vec<NativeMethod> {
        neo_native_methods![
            safe "serialize", fee = 1 << 12, flags = [], params = [Any], returns = ByteArray, names = ["item"];
            safe "deserialize", fee = 1 << 14, flags = [], params = [ByteArray], returns = Any, names = ["data"];
            safe "jsonSerialize", fee = 1 << 12, flags = [], params = [Any], returns = ByteArray, names = ["item"];
            safe "jsonDeserialize", fee = 1 << 14, flags = [], params = [ByteArray], returns = Any, names = ["json"];
            safe "itoa", fee = 1 << 12, flags = [], params = [Integer], returns = String, names = ["value"];
            safe "itoa", fee = 1 << 12, flags = [], params = [Integer, Integer], returns = String, names = ["value", "base"];
            safe "atoi", fee = 1 << 6, flags = [], params = [String], returns = Integer, names = ["value"];
            safe "atoi", fee = 1 << 6, flags = [], params = [String, Integer], returns = Integer, names = ["value", "base"];
            safe "base64Encode", fee = 1 << 5, flags = [], params = [ByteArray], returns = String, names = ["data"];
            safe "base64Decode", fee = 1 << 5, flags = [], params = [String], returns = ByteArray, names = ["s"];
            safe "base64UrlEncode", fee = 1 << 5, flags = [], params = [String], returns = String, active = HfCockatrice, names = ["data"];
            safe "base64UrlDecode", fee = 1 << 5, flags = [], params = [String], returns = String, active = HfCockatrice, names = ["s"];
            safe "base58Encode", fee = 1 << 13, flags = [], params = [ByteArray], returns = String, names = ["data"];
            safe "base58Decode", fee = 1 << 10, flags = [], params = [String], returns = ByteArray, names = ["s"];
            safe "base58CheckEncode", fee = 1 << 16, flags = [], params = [ByteArray], returns = String, names = ["data"];
            safe "base58CheckDecode", fee = 1 << 16, flags = [], params = [String], returns = ByteArray, names = ["s"];
            safe "hexEncode", fee = 1 << 5, flags = [], params = [ByteArray], returns = String, active = HfFaun, names = ["bytes"];
            safe "hexDecode", fee = 1 << 5, flags = [], params = [String], returns = ByteArray, active = HfFaun, names = ["str"];
            safe "memoryCompare", fee = 1 << 5, flags = [], params = [ByteArray, ByteArray], returns = Integer, names = ["str1", "str2"];
            safe "memorySearch", fee = 1 << 6, flags = [], params = [ByteArray, ByteArray], returns = Integer, names = ["mem", "value"];
            safe "memorySearch", fee = 1 << 6, flags = [], params = [ByteArray, ByteArray, Integer], returns = Integer, names = ["mem", "value", "start"];
            safe "memorySearch", fee = 1 << 6, flags = [], params = [ByteArray, ByteArray, Integer, Boolean], returns = Integer, names = ["mem", "value", "start", "backward"];
            safe "stringSplit", fee = 1 << 8, flags = [], params = [String, String], returns = Array, names = ["str", "separator"];
            safe "stringSplit", fee = 1 << 8, flags = [], params = [String, String, Boolean], returns = Array, names = ["str", "separator", "removeEmptyEntries"];
            safe "strLen", fee = 1 << 8, flags = [], params = [String], returns = Integer, names = ["str"];
        ]
    }
}
