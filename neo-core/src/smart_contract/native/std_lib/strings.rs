use super::StdLib;
use crate::error::CoreError as Error;
use crate::error::CoreResult as Result;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::BinarySerializer;
use crate::neo_vm::{StackItem, StackItemExt};
use unicode_segmentation::UnicodeSegmentation;

impl StdLib {
    /// Splits a string by a delimiter.
    /// Supports 2 overloads:
    /// - stringSplit(str, separator) -> splits string, keeps empty entries
    /// - stringSplit(str, separator, removeEmptyEntries) -> splits with option to remove empty entries
    pub(super) fn string_split(
        &self,
        engine: &ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.len() < 2 {
            return Err(Error::native_contract(
                "stringSplit requires at least 2 arguments (str, separator)".to_string(),
            ));
        }

        let string_data = String::from_utf8(args[0].clone())
            .map_err(|_| Error::native_contract("Invalid UTF-8 string"))?;

        let separator = String::from_utf8(args[1].clone())
            .map_err(|_| Error::native_contract("Invalid UTF-8 separator"))?;

        self.ensure_max_input_len(string_data.as_bytes(), "stringSplit")?;
        self.ensure_max_input_len(separator.as_bytes(), "stringSplit")?;

        // Parse optional removeEmptyEntries parameter (default: false)
        let remove_empty_entries = if args.len() >= 3 {
            if args[2].is_empty() {
                false
            } else {
                args[2][0] != 0
            }
        } else {
            false
        };

        // Split the string
        let parts: Vec<&str> = if remove_empty_entries {
            string_data
                .split(&separator)
                .filter(|s| !s.is_empty())
                .collect()
        } else {
            string_data.split(&separator).collect()
        };

        let items = parts
            .into_iter()
            .map(|part| StackItem::from_byte_string(part.as_bytes().to_vec()))
            .collect::<Vec<_>>();
        let array_item = StackItem::from_array(items);
        BinarySerializer::serialize(&array_item, engine.execution_limits())
            .map_err(|e| Error::native_contract(format!("stringSplit failed: {e}")))
    }

    /// Gets the length of a string in grapheme clusters (text elements).
    /// This matches C#'s TextElementEnumerator behavior, correctly counting
    /// complex Unicode characters like emojis as single elements.
    /// For example: "🦆" = 1, "ã" = 1
    pub(super) fn str_len(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "strLen requires string argument".to_string(),
            ));
        }

        let string_data = String::from_utf8(args[0].clone())
            .map_err(|_| Error::native_contract("Invalid UTF-8 string"))?;

        self.ensure_max_input_len(string_data.as_bytes(), "strLen")?;

        // Count grapheme clusters (extended grapheme clusters) to match C# TextElementEnumerator
        let length = string_data.graphemes(true).count() as i32;
        Ok(length.to_le_bytes().to_vec())
    }
}
