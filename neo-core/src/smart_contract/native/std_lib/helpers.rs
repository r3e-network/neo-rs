use super::StdLib;
use crate::error::CoreError as Error;
use crate::error::CoreResult as Result;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::BinarySerializer;
use crate::neo_vm::{StackItem};
use num_bigint::{BigInt, Sign};
use num_traits::{Num, ToPrimitive, Zero};

impl StdLib {
    /// Validate that `args` has at least one element and its length is within limits.
    pub(super) fn validate_single_arg<'a>(
        &self,
        args: &'a [Vec<u8>],
        method: &str,
    ) -> Result<&'a [u8]> {
        if args.is_empty() {
            return Err(Error::native_contract(format!(
                "{method} requires an argument"
            )));
        }
        self.ensure_max_input_len(&args[0], method)?;
        Ok(&args[0])
    }

    /// Validate a single arg and convert it to a UTF-8 string.
    pub(super) fn validate_string_arg(
        &self,
        args: &[Vec<u8>],
        method: &str,
    ) -> Result<String> {
        let data = self.validate_single_arg(args, method)?;
        String::from_utf8(data.to_vec())
            .map_err(|_| Error::native_contract("Invalid UTF-8 string"))
    }

    pub(super) fn ensure_max_input_len(&self, data: &[u8], method: &str) -> Result<()> {
        if data.len() > Self::MAX_INPUT_LENGTH {
            return Err(Error::native_contract(format!(
                "{} input exceeds max length {}",
                method,
                Self::MAX_INPUT_LENGTH
            )));
        }
        Ok(())
    }

    pub(super) fn parse_optional_base(
        &self,
        args: &[Vec<u8>],
        index: usize,
        method: &str,
    ) -> Result<i32> {
        if args.len() <= index {
            return Ok(10);
        }
        let base = BigInt::from_signed_bytes_le(&args[index]);
        base.to_i32()
            .ok_or_else(|| Error::native_contract(format!("Invalid base argument for {}", method)))
    }

    pub(super) fn parse_hex_twos_complement(&self, input: &str) -> Result<BigInt> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(Error::native_contract(
                "Invalid hex number format".to_string(),
            ));
        }
        if trimmed.starts_with('+') || trimmed.starts_with('-') {
            return Err(Error::native_contract(
                "Invalid hex number format".to_string(),
            ));
        }

        let normalized = trimmed.to_ascii_lowercase();
        let unsigned = BigInt::from_str_radix(&normalized, 16)
            .map_err(|_| Error::native_contract("Invalid hex number format"))?;
        let bits = trimmed
            .len()
            .checked_mul(4)
            .ok_or_else(|| Error::native_contract("Hex value too large"))?;
        if bits == 0 {
            return Ok(BigInt::from(0));
        }

        let sign_bit = BigInt::from(1) << (bits - 1);
        if (&unsigned & &sign_bit) != BigInt::from(0) {
            let modulus = BigInt::from(1) << bits;
            Ok(unsigned - modulus)
        } else {
            Ok(unsigned)
        }
    }

    pub(super) fn format_hex_twos_complement(&self, value: &BigInt) -> String {
        if value.is_zero() {
            return "0".to_string();
        }
        if value.sign() != Sign::Minus {
            let hex = value.to_str_radix(16);
            let requires_sign_padding =
                hex.len() % 2 == 0 && matches!(hex.as_bytes().first(), Some(b'8'..=b'f'));
            return if requires_sign_padding {
                format!("0{hex}")
            } else {
                hex
            };
        }

        let abs_value = (-value).to_biguint().unwrap_or_default();
        let bit_len = abs_value.to_str_radix(2).len();
        let is_power_of_two = !abs_value.is_zero() && (&abs_value & (&abs_value - 1u32)).is_zero();
        let bits_required = if is_power_of_two {
            bit_len
        } else {
            bit_len + 1
        };
        let nibbles = bits_required.div_ceil(4);
        let bits = nibbles * 4;
        let modulus = BigInt::from(1) << bits;
        let unsigned = modulus + value;
        let mut hex = unsigned.to_str_radix(16);
        if hex.len() < nibbles {
            let padding = "0".repeat(nibbles - hex.len());
            hex = format!("{}{}", padding, hex);
        }
        hex
    }

    pub(super) fn decode_stack_item(
        &self,
        engine: &ApplicationEngine,
        data: &[u8],
    ) -> Result<StackItem> {
        let limits = engine.execution_limits();
        match BinarySerializer::deserialize(data, limits, None) {
            Ok(item) => Ok(item),
            Err(_) => Ok(StackItem::from_byte_string(data.to_vec())),
        }
    }
}
