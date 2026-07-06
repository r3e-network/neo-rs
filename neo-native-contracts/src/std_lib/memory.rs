//! StdLib memory comparison/search helpers.
//!
//! Keeps .NET span search compatibility separate from the contract root.

use super::StdLib;
use neo_error::{CoreError, CoreResult};
use num_bigint::BigInt;

impl StdLib {
    /// C# `StdLib.MemorySearch` (its 3 overloads dispatch by argument count):
    /// forward search returns `mem[start..].IndexOf(value) + start` (or -1);
    /// backward search returns `mem[0..start].LastIndexOf(value)` (or -1).
    pub(super) fn memory_search_impl(args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
        let mem = args.first().map(Vec::as_slice).ok_or_else(|| {
            CoreError::invalid_operation("StdLib::memorySearch requires (mem, value)")
        })?;
        Self::ensure_max_len("memorySearch", "mem", mem)?;
        let value = args.get(1).map(Vec::as_slice).ok_or_else(|| {
            CoreError::invalid_operation("StdLib::memorySearch requires (mem, value)")
        })?;
        // C# marshals the `int start` parameter with `(int)p.GetInteger()`, a
        // TRUNCATING two's-complement cast to the low 32 bits (wrapping, not
        // faulting on out-of-range). `MemorySearch` then does `AsSpan(start)` /
        // `AsSpan(0, start)`, which throw only for `start < 0` or `start > length`.
        let start_i32 = match args.get(2) {
            Some(b) => Self::dotnet_int_cast(&BigInt::from_signed_bytes_le(b)),
            None => 0,
        };
        if start_i32 < 0 || i64::from(start_i32) > mem.len() as i64 {
            return Err(CoreError::invalid_operation(
                "StdLib::memorySearch: start out of range",
            ));
        }
        let start = start_i32 as usize;
        let backward = args
            .get(3)
            .map(|b| b.iter().any(|x| *x != 0))
            .unwrap_or(false);
        Ok(BigInt::from(Self::memory_search(mem, value, start, backward)).to_signed_bytes_le())
    }

    fn memory_search(mem: &[u8], value: &[u8], start: usize, backward: bool) -> i64 {
        if backward {
            Self::last_index_of(&mem[..start], value)
        } else {
            match Self::index_of(&mem[start..], value) {
                Some(i) => (i + start) as i64,
                None => -1,
            }
        }
    }

    /// First index of `needle` in `haystack`, matching .NET `Span.IndexOf`
    /// (an empty needle is found at 0).
    fn index_of(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        if needle.is_empty() {
            return Some(0);
        }
        if needle.len() > haystack.len() {
            return None;
        }
        haystack.windows(needle.len()).position(|w| w == needle)
    }

    /// Last index of `needle` in `haystack` (or -1), matching .NET
    /// `Span.LastIndexOf` (an empty needle is reported at `haystack.len()`).
    fn last_index_of(haystack: &[u8], needle: &[u8]) -> i64 {
        if needle.is_empty() {
            return haystack.len() as i64;
        }
        if needle.len() > haystack.len() {
            return -1;
        }
        haystack
            .windows(needle.len())
            .rposition(|w| w == needle)
            .map_or(-1, |i| i as i64)
    }
}
