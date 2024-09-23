/*
Package std provides an interface to StdLib native contract.
It implements various useful conversion functions.
*/
mod std;

use crate::interop::contract;
use crate::interop::neogointernal;

// Hash represents StdLib contract hash.
const HASH: &str = "\xc0\xef\x39\xce\xe0\xe4\xe9\x25\xc6\xc2\xa0\x6a\x79\xe1\x44\x0d\xd8\x6f\xce\xac";

// Serialize calls `serialize` method of StdLib native contract and serializes
// any given item into a byte slice. It works for all regular VM types (not ones
// from interop package) and allows to save them in the storage or pass them into Notify
// and then Deserialize them on the next run or in the external event receiver.
pub fn serialize(item: impl Into<neogointernal::Any>) -> Vec<u8> {
    neogointernal::call_with_token(HASH, "serialize", contract::NoneFlag as i32, item).unwrap()
}

// Deserialize calls `deserialize` method of StdLib native contract and unpacks
// a previously serialized value from a byte slice, it's the opposite of Serialize.
pub fn deserialize(b: Vec<u8>) -> neogointernal::Any {
    neogointernal::call_with_token(HASH, "deserialize", contract::NoneFlag as i32, b).unwrap()
}

// JSONSerialize serializes a value to json. It uses `jsonSerialize` method of StdLib native
// contract.
// Serialization format is the following:
// []byte -> base64 string
// bool -> json boolean
// nil -> Null
// string -> base64 encoded sequence of underlying bytes
// (u)int* -> integer, only value in -2^53..2^53 are allowed
// []interface{} -> json array
// []any -> json array
// map[type1]type2 -> json object with string keys marshaled as strings (not base64).
pub fn json_serialize(item: impl Into<neogointernal::Any>) -> Vec<u8> {
    neogointernal::call_with_token(HASH, "jsonSerialize", contract::NoneFlag as i32, item).unwrap()
}

// JSONDeserialize deserializes a value from json. It uses `jsonDeserialize` method of StdLib
// native contract.
// It performs deserialization as follows:
//
//	strings -> []byte (string) from base64
//	integers -> (u)int* types
//	null -> interface{}(nil)
//	arrays -> []interface{}
//	maps -> map[string]interface{}
pub fn json_deserialize(data: Vec<u8>) -> neogointernal::Any {
    neogointernal::call_with_token(HASH, "jsonDeserialize", contract::NoneFlag as i32, data).unwrap()
}

// Base64Encode calls `base64Encode` method of StdLib native contract and encodes
// the given byte slice into a base64 string and returns byte representation of this
// string.
pub fn base64_encode(b: Vec<u8>) -> String {
    neogointernal::call_with_token(HASH, "base64Encode", contract::NoneFlag as i32, b).unwrap()
}

// Base64Decode calls `base64Decode` method of StdLib native contract and decodes
// the given base64 string represented as a byte slice into byte slice.
pub fn base64_decode(b: Vec<u8>) -> Vec<u8> {
    neogointernal::call_with_token(HASH, "base64Decode", contract::NoneFlag as i32, b).unwrap()
}

// Base58Encode calls `base58Encode` method of StdLib native contract and encodes
// the given byte slice into a base58 string and returns byte representation of this
// string.
pub fn base58_encode(b: Vec<u8>) -> String {
    neogointernal::call_with_token(HASH, "base58Encode", contract::NoneFlag as i32, b).unwrap()
}

// Base58Decode calls `base58Decode` method of StdLib native contract and decodes
// the given base58 string represented as a byte slice into a new byte slice.
pub fn base58_decode(b: Vec<u8>) -> Vec<u8> {
    neogointernal::call_with_token(HASH, "base58Decode", contract::NoneFlag as i32, b).unwrap()
}

// Base58CheckEncode calls `base58CheckEncode` method of StdLib native contract and encodes
// the given byte slice into a base58 string with checksum and returns byte representation of this
// string.
pub fn base58_check_encode(b: Vec<u8>) -> String {
    neogointernal::call_with_token(HASH, "base58CheckEncode", contract::NoneFlag as i32, b).unwrap()
}

// Base58CheckDecode calls `base58CheckDecode` method of StdLib native contract and decodes
// thr given base58 string with a checksum represented as a byte slice into a new byte slice.
pub fn base58_check_decode(b: Vec<u8>) -> Vec<u8> {
    neogointernal::call_with_token(HASH, "base58CheckDecode", contract::NoneFlag as i32, b).unwrap()
}

// Itoa converts num in the given base to a string. Base should be either 10 or 16.
// It uses `itoa` method of StdLib native contract.
pub fn itoa(num: i32, base: i32) -> String {
    neogointernal::call_with_token(HASH, "itoa", contract::NoneFlag as i32, num, base).unwrap()
}

// Itoa10 converts num in base 10 to a string.
// It uses `itoa` method of StdLib native contract.
pub fn itoa10(num: i32) -> String {
    neogointernal::call_with_token(HASH, "itoa", contract::NoneFlag as i32, num).unwrap()
}

// Atoi converts a string to a number in the given base. Base should be either 10 or 16.
// It uses `atoi` method of StdLib native contract.
pub fn atoi(s: String, base: i32) -> i32 {
    neogointernal::call_with_token(HASH, "atoi", contract::NoneFlag as i32, s, base).unwrap()
}

// Atoi10 converts a string to a number in base 10.
// It uses `atoi` method of StdLib native contract.
pub fn atoi10(s: String) -> i32 {
    neogointernal::call_with_token(HASH, "atoi", contract::NoneFlag as i32, s).unwrap()
}

// MemoryCompare is similar to bytes.Compare:
// The result will be 0 if a==b, -1 if a < b, and +1 if a > b.
// It uses `memoryCompare` method of StdLib native contract.
pub fn memory_compare(s1: Vec<u8>, s2: Vec<u8>) -> i32 {
    neogointernal::call_with_token(HASH, "memoryCompare", contract::NoneFlag as i32, s1, s2).unwrap()
}

// MemorySearch returns the index of the first occurrence of the val in the mem.
// If not found, -1 is returned. It uses `memorySearch` method of StdLib native contract.
pub fn memory_search(mem: Vec<u8>, pattern: Vec<u8>) -> i32 {
    neogointernal::call_with_token(HASH, "memorySearch", contract::NoneFlag as i32, mem, pattern).unwrap()
}

// MemorySearchIndex returns the index of the first occurrence of the val in the mem starting from the start.
// If not found, -1 is returned. It uses `memorySearch` method of StdLib native contract.
pub fn memory_search_index(mem: Vec<u8>, pattern: Vec<u8>, start: i32) -> i32 {
    neogointernal::call_with_token(HASH, "memorySearch", contract::NoneFlag as i32, mem, pattern, start).unwrap()
}

// MemorySearchLastIndex returns the index of the last occurrence of the val in the mem ending before start.
// If not found, -1 is returned. It uses `memorySearch` method of StdLib native contract.
pub fn memory_search_last_index(mem: Vec<u8>, pattern: Vec<u8>, start: i32) -> i32 {
    neogointernal::call_with_token(HASH, "memorySearch", contract::NoneFlag as i32, mem, pattern, start, true).unwrap()
}

// StringSplit splits s by occurrences of the sep.
// It uses `stringSplit` method of StdLib native contract.
pub fn string_split(s: String, sep: String) -> Vec<String> {
    neogointernal::call_with_token(HASH, "stringSplit", contract::NoneFlag as i32, s, sep).unwrap()
}

// StringSplitNonEmpty splits s by occurrences of the sep and returns a list of non-empty items.
// It uses `stringSplit` method of StdLib native contract.
pub fn string_split_non_empty(s: String, sep: String) -> Vec<String> {
    neogointernal::call_with_token(HASH, "stringSplit", contract::NoneFlag as i32, s, sep, true).unwrap()
}

// StrLen returns length of the string in Utf- characters.
// It uses `strLen` method of StdLib native contract.
pub fn str_len(s: String) -> i32 {
    neogointernal::call_with_token(HASH, "strLen", contract::NoneFlag as i32, s).unwrap()
}
