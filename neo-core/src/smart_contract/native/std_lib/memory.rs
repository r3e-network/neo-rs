use super::StdLib;
use crate::error::CoreError as Error;
use crate::error::CoreResult as Result;
use memchr::memmem;
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

        // C# StdLib.MemorySearch annotates only `mem` with [MaxLength(MaxInputLength)];
        // `value` is NOT length-limited (StdLib.cs:223,229,235). Over-enforcing on
        // `value` would FAULT where C# succeeds, diverging VM results.
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

        // Parse optional backward parameter (default: false)
        let backward = if args.len() >= 4 {
            if args[3].is_empty() {
                false
            } else {
                args[3][0] != 0
            }
        } else {
            false
        };

        // Validate start index
        if start < 0 || start as usize > mem.len() {
            return Err(Error::native_contract(format!(
                "start index {} out of range [0, {}]",
                start,
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
