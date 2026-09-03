use super::StdLib;
use crate::error::CoreError as Error;
use crate::error::CoreResult as Result;
use num_bigint::BigInt;

impl StdLib {
    /// Converts a string to an integer.
    pub(super) fn atoi(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "atoi requires string argument".to_string(),
            ));
        }

        self.ensure_max_input_len(&args[0], "atoi")?;
        let string_data = String::from_utf8(args[0].clone())
            .map_err(|_| Error::native_contract("Invalid UTF-8 string"))?;
        let base = self.parse_optional_base(args, 1, "atoi")?;
        let value = match base {
            10 => string_data
                .parse::<BigInt>()
                .map_err(|_| Error::native_contract("Invalid number format"))?,
            16 => self.parse_hex_twos_complement(&string_data)?,
            _ => return Err(Error::native_contract(format!("Invalid base: {}", base))),
        };

        Ok(value.to_signed_bytes_le())
    }

    /// Converts an integer to a string.
    pub(super) fn itoa(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "itoa requires integer argument".to_string(),
            ));
        }

        let value = BigInt::from_signed_bytes_le(&args[0]);
        let base = self.parse_optional_base(args, 1, "itoa")?;
        let encoded = match base {
            10 => value.to_string(),
            16 => self.format_hex_twos_complement(&value),
            _ => return Err(Error::native_contract(format!("Invalid base: {}", base))),
        };

        Ok(encoded.into_bytes())
    }
}
