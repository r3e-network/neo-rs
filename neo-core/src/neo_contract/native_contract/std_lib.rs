
use std::cmp::Ordering;
use neo_proc_macros::contract;

const MAX_INPUT_LENGTH: usize = 1024;

#[contract]
pub struct StdLib;

#[contract_impl]
impl StdLib {
    #[no_mangle]
    pub fn serialize(engine: &mut ApplicationEngine, item: StackItem) -> Vec<u8> {
        BinarySerializer::serialize(&item, &engine.limits)
    }

    #[no_mangle]
    pub fn deserialize(engine: &mut ApplicationEngine, data: Vec<u8>) -> StackItem {
        BinarySerializer::deserialize(&data, &engine.limits, &engine.reference_counter)
    }

    #[no_mangle]
    pub fn json_serialize(engine: &mut ApplicationEngine, item: StackItem) -> Vec<u8> {
        JsonSerializer::serialize_to_byte_array(&item, engine.limits.max_item_size)
    }

    #[no_mangle]
    pub fn json_deserialize(engine: &mut ApplicationEngine, json: Vec<u8>) -> StackItem {
        let jtoken = JToken::parse(&json, 10).expect("Invalid JSON");
        JsonSerializer::deserialize(engine, &jtoken, &engine.limits, &engine.reference_counter)
    }

    #[no_mangle]
    pub fn itoa(value: BigInt) -> String {
        Self::itoa_with_base(value, 10)
    }

    #[no_mangle]
    pub fn itoa_with_base(value: BigInt, base: i32) -> String {
        match base {
            10 => value.to_string(),
            16 => format!("{:x}", value),
            _ => panic!("Unsupported base"),
        }
    }

    #[no_mangle]
    pub fn atoi(value: String) -> BigInt {
        Self::atoi_with_base(value, 10)
    }

    #[no_mangle]
    pub fn atoi_with_base(value: String, base: i32) -> BigInt {
        match base {
            10 => value.parse().expect("Invalid decimal number"),
            16 => BigInt::from_str_radix(&value, 16).expect("Invalid hexadecimal number"),
            _ => panic!("Unsupported base"),
        }
    }

    #[no_mangle]
    pub fn base64_encode(data: Vec<u8>) -> String {
        Base64::encode(&data)
    }

    #[no_mangle]
    pub fn base64_decode(s: String) -> Vec<u8> {
        Base64::decode(&s).expect("Invalid base64 string")
    }

    #[no_mangle]
    pub fn base58_encode(data: Vec<u8>) -> String {
        Base58::encode(&data)
    }

    #[no_mangle]
    pub fn base58_decode(s: String) -> Vec<u8> {
        Base58::decode(&s).expect("Invalid base58 string")
    }

    #[no_mangle]
    pub fn base58_check_encode(data: Vec<u8>) -> String {
        Base58::base58check_encode(&data)
    }

    #[no_mangle]
    pub fn base58_check_decode(s: String) -> Vec<u8> {
        Base58::base58check_decode(&s).expect("Invalid base58check string")
    }

    #[no_mangle]
    pub fn memory_compare(str1: Vec<u8>, str2: Vec<u8>) -> i32 {
        match str1.cmp(&str2) {
            Ordering::Less => -1,
            Ordering::Equal => 0,
            Ordering::Greater => 1,
        }
    }

    #[no_mangle]
    pub fn memory_search(mem: Vec<u8>, value: Vec<u8>) -> i32 {
        Self::memory_search_with_params(mem, value, 0, false)
    }

    #[no_mangle]
    pub fn memory_search_with_start(mem: Vec<u8>, value: Vec<u8>, start: i32) -> i32 {
        Self::memory_search_with_params(mem, value, start, false)
    }

    #[no_mangle]
    pub fn memory_search_with_params(mem: Vec<u8>, value: Vec<u8>, start: i32, backward: bool) -> i32 {
        if value.is_empty() {
            return -1;
        }
        let start = start as usize;
        if backward {
            mem[..start].windows(value.len()).rposition(|window| window == value)
                .map(|i| i as i32)
                .unwrap_or(-1)
        } else {
            mem[start..].windows(value.len()).position(|window| window == value)
                .map(|i| (i + start) as i32)
                .unwrap_or(-1)
        }
    }

    #[no_mangle]
    pub fn string_split(str: String, separator: String) -> Vec<String> {
        str.split(&separator).map(String::from).collect()
    }

    #[no_mangle]
    pub fn string_split_with_options(str: String, separator: String, remove_empty_entries: bool) -> Vec<String> {
        let parts: Vec<String> = str.split(&separator).map(String::from).collect();
        if remove_empty_entries {
            parts.into_iter().filter(|s| !s.is_empty()).collect()
        } else {
            parts
        }
    }

    #[no_mangle]
    pub fn str_len(str: String) -> i32 {
        unicode_segmentation::UnicodeSegmentation::graphemes(str.as_str(), true).count() as i32
    }
}
