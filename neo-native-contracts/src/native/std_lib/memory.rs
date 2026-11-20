use super::StdLib;
use crate::native::helpers::parse_bool_arg;
use memchr::memmem;
use neo_smart_contract::error::CoreError as Error;
use neo_smart_contract::error::CoreResult as Result;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

impl StdLib {
    /// Compares two memory regions.
    pub(super) fn memory_compare(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() < 2 {
            return Err(Error::native_contract(
                "memoryCompare requires two data arguments".to_string(),
            ));
        }

        self.ensure_max_input_len(&args[0], "memoryCompare")?;
        self.ensure_max_input_len(&args[1], "memoryCompare")?;
        let result = match args[0].cmp(&args[1]) {
            std::cmp::Ordering::Less => -1i32,
            std::cmp::Ordering::Equal => 0i32,
            std::cmp::Ordering::Greater => 1i32,
        };

        Ok(result.to_le_bytes().to_vec())
    }

    /// Searches for a pattern in memory.
    /// Supports 3 overloads:
    /// - memorySearch(mem, value) -> searches from start, forward
    /// - memorySearch(mem, value, start) -> searches from start index, forward
    /// - memorySearch(mem, value, start, backward) -> searches with direction control
    pub(super) fn memory_search(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() < 2 {
            return Err(Error::native_contract(
                "memorySearch requires at least 2 arguments (mem, value)".to_string(),
            ));
        }

        let mem = &args[0];
        let value = &args[1];

        // Neo only bounds the searched memory; the pattern length is unrestricted.
        self.ensure_max_input_len(mem, "memorySearch")?;

        // Parse optional start parameter (default: 0)
        let start = if args.len() >= 3 {
            let start_value = BigInt::from_signed_bytes_le(&args[2]);
            start_value
                .to_i32()
                .ok_or_else(|| Error::native_contract("start parameter out of range"))?
        } else {
            0
        };

        // Parse optional backward parameter (default: false).
        // Neo converts Boolean arguments through StackItem truthiness, so any
        // non-zero integer value (e.g. Integer(256) encoded as [0x00, 0x01]) is true.
        let backward = args.get(3).is_some_and(|arg| parse_bool_arg(arg));

        // Validate start index
        if start < 0 || start as usize > mem.len() {
            return Err(Error::native_contract(format!(
                "start index {start} out of range [0, {}]",
                mem.len()
            )));
        }

        let start_usize = start as usize;

        // Handle empty pattern
        if value.is_empty() {
            return Ok(start.to_le_bytes().to_vec());
        }

        let result = if backward {
            memmem::rfind(&mem[..start_usize], value).map_or(-1, |pos| pos as i32)
        } else {
            memmem::find(&mem[start_usize..], value).map_or(-1, |pos| (start_usize + pos) as i32)
        };

        Ok(result.to_le_bytes().to_vec())
    }
}
