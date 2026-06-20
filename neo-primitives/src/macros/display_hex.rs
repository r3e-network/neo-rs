/// Implements `Display` for a type by formatting its byte representation as hex.
///
/// The second argument is a field name whose value is a byte container
/// (`Vec<u8>`, `&[u8]`, etc.). Each byte is written as a two-digit
/// lowercase hex pair (e.g. `0a`).
///
/// # Example
///
/// ```rust,ignore
/// impl_display_hex!(MyType, data);
/// ```
#[macro_export]
macro_rules! impl_display_hex {
    ($type:ty, $field:ident) => {
        impl ::std::fmt::Display for $type {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                for byte in &self.$field {
                    ::std::write!(f, "{:02x}", byte)?;
                }
                ::std::result::Result::Ok(())
            }
        }
    };
}
