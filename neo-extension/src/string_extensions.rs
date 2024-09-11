pub trait StringExtensions {
    fn hex_to_bytes(&self) -> Result<Vec<u8>, String>;
}

impl StringExtensions for str {
    /// Converts a hex string to byte array.
    ///
    /// # Arguments
    ///
    /// * `self` - The hex string to convert.
    ///
    /// # Returns
    ///
    /// The converted byte array, or an error if the input is invalid.
    fn hex_to_bytes(&self) -> Result<Vec<u8>, String> {
        if self.is_empty() {
            return Ok(vec![]);
        }
        if self.len() % 2 != 0 {
            return Err("Invalid hex string length".to_string());
        }
        (0..self.len())
            .step_by(2)
            .map(|i| {
                u8::from_str_radix(&self[i..i + 2], 16)
                    .map_err(|e| format!("Invalid hex character: {}", e))
            })
            .collect()
    }
}
