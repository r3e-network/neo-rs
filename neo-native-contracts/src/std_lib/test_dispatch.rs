//! Test-only pure StdLib dispatch seam.
//!
//! Production invokes StdLib through the native binding table in `metadata` plus
//! the engine-backed handlers in `invoke`. Unit tests keep this pure arity-aware
//! seam so C# compatibility vectors can exercise stateless StdLib behavior
//! without constructing an `ApplicationEngine`.

use neo_error::CoreResult;

use super::{StdLib, encoding, serialization};

type StdLibPureHandler = fn(&[Vec<u8>], bool) -> CoreResult<Vec<u8>>;

struct StdLibPureBinding {
    name: &'static str,
    arity: usize,
    handler: StdLibPureHandler,
}

impl StdLibPureBinding {
    const fn new(name: &'static str, arity: usize, handler: StdLibPureHandler) -> Self {
        Self {
            name,
            arity,
            handler,
        }
    }

    fn matches(&self, method: &str, arity: usize) -> bool {
        self.name == method && self.arity == arity
    }

    fn invoke(&self, args: &[Vec<u8>], basilisk_active: bool) -> CoreResult<Vec<u8>> {
        (self.handler)(args, basilisk_active)
    }
}

static STD_LIB_PURE_BINDINGS: &[StdLibPureBinding] = &[
    StdLibPureBinding::new("base64Encode", 1, pure_base64_encode),
    StdLibPureBinding::new("base64Decode", 1, pure_base64_decode),
    StdLibPureBinding::new("base58Encode", 1, pure_base58_encode),
    StdLibPureBinding::new("base58Decode", 1, pure_base58_decode),
    StdLibPureBinding::new("base58CheckEncode", 1, pure_base58_check_encode),
    StdLibPureBinding::new("base58CheckDecode", 1, pure_base58_check_decode),
    StdLibPureBinding::new("serialize", 1, pure_serialize),
    StdLibPureBinding::new("deserialize", 1, pure_deserialize),
    StdLibPureBinding::new("jsonSerialize", 1, pure_json_serialize),
    StdLibPureBinding::new("jsonDeserialize", 1, pure_json_deserialize),
    StdLibPureBinding::new("memoryCompare", 2, pure_memory_compare),
    StdLibPureBinding::new("memorySearch", 2, pure_memory_search),
    StdLibPureBinding::new("memorySearch", 3, pure_memory_search),
    StdLibPureBinding::new("memorySearch", 4, pure_memory_search),
    StdLibPureBinding::new("itoa", 1, pure_itoa),
    StdLibPureBinding::new("itoa", 2, pure_itoa),
    StdLibPureBinding::new("atoi", 1, pure_atoi),
    StdLibPureBinding::new("atoi", 2, pure_atoi),
    StdLibPureBinding::new("stringSplit", 2, pure_string_split),
    StdLibPureBinding::new("stringSplit", 3, pure_string_split),
    StdLibPureBinding::new("strLen", 1, pure_str_len),
    StdLibPureBinding::new("base64UrlEncode", 1, pure_base64_url_encode),
    StdLibPureBinding::new("base64UrlDecode", 1, pure_base64_url_decode),
    StdLibPureBinding::new("hexEncode", 1, pure_hex_encode),
    StdLibPureBinding::new("hexDecode", 1, pure_hex_decode),
];

impl StdLib {
    /// Pure dispatch for StdLib's stateless methods.
    ///
    /// `basilisk_active` is the only block-height context StdLib needs; it gates
    /// `jsonDeserialize` number handling exactly as the production engine wrapper
    /// does. Returns `None` for unknown methods or wrong overload arity.
    pub(in crate::std_lib) fn dispatch(
        method: &str,
        args: &[Vec<u8>],
        basilisk_active: bool,
    ) -> Option<CoreResult<Vec<u8>>> {
        STD_LIB_PURE_BINDINGS
            .iter()
            .find(|binding| binding.matches(method, args.len()))
            .map(|binding| binding.invoke(args, basilisk_active))
    }
}

fn pure_base64_encode(args: &[Vec<u8>], _basilisk_active: bool) -> CoreResult<Vec<u8>> {
    encoding::base64_encode_impl(args)
}

fn pure_base64_decode(args: &[Vec<u8>], _basilisk_active: bool) -> CoreResult<Vec<u8>> {
    encoding::base64_decode_impl(args)
}

fn pure_base64_url_encode(args: &[Vec<u8>], _basilisk_active: bool) -> CoreResult<Vec<u8>> {
    encoding::base64_url_encode_impl(args)
}

fn pure_base64_url_decode(args: &[Vec<u8>], _basilisk_active: bool) -> CoreResult<Vec<u8>> {
    encoding::base64_url_decode_impl(args)
}

fn pure_hex_encode(args: &[Vec<u8>], _basilisk_active: bool) -> CoreResult<Vec<u8>> {
    encoding::hex_encode_impl(args)
}

fn pure_hex_decode(args: &[Vec<u8>], _basilisk_active: bool) -> CoreResult<Vec<u8>> {
    encoding::hex_decode_impl(args)
}

fn pure_base58_encode(args: &[Vec<u8>], _basilisk_active: bool) -> CoreResult<Vec<u8>> {
    encoding::base58_encode_impl(args)
}

fn pure_base58_decode(args: &[Vec<u8>], _basilisk_active: bool) -> CoreResult<Vec<u8>> {
    encoding::base58_decode_impl(args)
}

fn pure_base58_check_encode(args: &[Vec<u8>], _basilisk_active: bool) -> CoreResult<Vec<u8>> {
    encoding::base58_check_encode_impl(args)
}

fn pure_base58_check_decode(args: &[Vec<u8>], _basilisk_active: bool) -> CoreResult<Vec<u8>> {
    encoding::base58_check_decode_impl(args)
}

fn pure_memory_compare(args: &[Vec<u8>], _basilisk_active: bool) -> CoreResult<Vec<u8>> {
    StdLib::memory_compare_impl(args)
}

fn pure_memory_search(args: &[Vec<u8>], _basilisk_active: bool) -> CoreResult<Vec<u8>> {
    StdLib::memory_search_impl(args)
}

fn pure_itoa(args: &[Vec<u8>], _basilisk_active: bool) -> CoreResult<Vec<u8>> {
    StdLib::itoa_impl(args)
}

fn pure_atoi(args: &[Vec<u8>], _basilisk_active: bool) -> CoreResult<Vec<u8>> {
    StdLib::atoi_impl(args)
}

fn pure_string_split(args: &[Vec<u8>], _basilisk_active: bool) -> CoreResult<Vec<u8>> {
    StdLib::string_split_impl(args)
}

fn pure_str_len(args: &[Vec<u8>], _basilisk_active: bool) -> CoreResult<Vec<u8>> {
    StdLib::str_len_impl(args)
}

fn pure_serialize(args: &[Vec<u8>], _basilisk_active: bool) -> CoreResult<Vec<u8>> {
    serialization::serialize_impl(args)
}

fn pure_deserialize(args: &[Vec<u8>], _basilisk_active: bool) -> CoreResult<Vec<u8>> {
    serialization::deserialize_impl(args)
}

fn pure_json_serialize(args: &[Vec<u8>], _basilisk_active: bool) -> CoreResult<Vec<u8>> {
    serialization::json_serialize_impl(args)
}

fn pure_json_deserialize(args: &[Vec<u8>], basilisk_active: bool) -> CoreResult<Vec<u8>> {
    serialization::json_deserialize_impl(args, basilisk_active)
}
