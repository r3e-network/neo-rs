//! JSON string escaping that matches C# `System.Text.Json` with the default
//! encoder (`JavaScriptEncoder.Default`).
//!
//! Neo's C# RPC server and `Neo.Json` serialize every token through
//! `Utf8JsonWriter` constructed *without* a custom encoder, which means it uses
//! `JavaScriptEncoder.Default`. That encoder escapes a broader set of
//! characters than the JSON specification requires:
//!
//! * The structural characters `"` and `\`.
//! * The HTML/script-sensitive ASCII characters `<` `>` `&` `'` `+` and `` ` ``.
//! * Every code point greater than `0x7F` (all non-ASCII), emitted as `\uXXXX`
//!   with surrogate pairs for astral-plane characters.
//!
//! `serde_json`'s built-in serializer only escapes the JSON-mandatory set and
//! writes raw UTF-8 for everything else, so its output diverges from C# for any
//! string containing non-ASCII data (token names, symbols, manifests,
//! notification arguments, error data, ...) or the HTML-sensitive characters.
//! Because RPC responses are consensus/interop-sensitive, this module provides a
//! [`serde_json::ser::Formatter`] wrapper that reproduces the C# byte-for-byte
//! output.
//!
//! Short escape forms match `Utf8JsonWriter` exactly: `\b` `\t` `\n` `\f` `\r`
//! and `\\`. Note that the quote character is emitted as `"` (not `\"`),
//! matching `JavaScriptEncoder.Default`. All other escapes use uppercase
//! hexadecimal digits, e.g. `<`, `&`, `\uD83D`.

use serde::Serialize;
use serde_json::ser::{CharEscape, CompactFormatter, Formatter, PrettyFormatter};
use std::io::{self, Write};

const HEX_UPPER: [u8; 16] = *b"0123456789ABCDEF";

/// Writes `\uXXXX` (uppercase hex) for the given 16-bit code unit.
fn write_unicode_escape<W>(writer: &mut W, unit: u16) -> io::Result<()>
where
    W: ?Sized + Write,
{
    let bytes = [
        b'\\',
        b'u',
        HEX_UPPER[((unit >> 12) & 0xF) as usize],
        HEX_UPPER[((unit >> 8) & 0xF) as usize],
        HEX_UPPER[((unit >> 4) & 0xF) as usize],
        HEX_UPPER[(unit & 0xF) as usize],
    ];
    writer.write_all(&bytes)
}

/// Returns `true` if the ASCII byte must be escaped by `JavaScriptEncoder.Default`.
///
/// `serde_json` does not escape these (they are JSON-legal unescaped), but the
/// C# default encoder does. The structural characters `"` (0x22) and `\` (0x5C)
/// are handled by `serde_json` via `write_char_escape` and are intentionally not
/// listed here.
const fn is_html_sensitive(byte: u8) -> bool {
    matches!(byte, b'<' | b'>' | b'&' | b'\'' | b'+' | b'`')
}

/// A [`serde_json::ser::Formatter`] that escapes output exactly like C#
/// `JavaScriptEncoder.Default` while delegating all structural formatting
/// (indentation, separators, numbers) to an inner formatter.
#[derive(Clone, Debug)]
pub struct CSharpEscapeFormatter<F> {
    inner: F,
}

impl<F> CSharpEscapeFormatter<F> {
    /// Wraps the given base formatter.
    pub const fn new(inner: F) -> Self {
        Self { inner }
    }
}

impl CSharpEscapeFormatter<CompactFormatter> {
    /// Creates a compact (non-indented) C#-compatible formatter.
    #[must_use]
    pub const fn compact() -> Self {
        Self::new(CompactFormatter)
    }
}

impl<'a> CSharpEscapeFormatter<PrettyFormatter<'a>> {
    /// Creates an indented C#-compatible formatter using the given indent.
    #[must_use]
    pub fn pretty(indent: &'a [u8]) -> Self {
        Self::new(PrettyFormatter::with_indent(indent))
    }
}

impl<F> Formatter for CSharpEscapeFormatter<F>
where
    F: Formatter,
{
    #[inline]
    fn write_null<W>(&mut self, writer: &mut W) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        self.inner.write_null(writer)
    }

    #[inline]
    fn write_bool<W>(&mut self, writer: &mut W, value: bool) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        self.inner.write_bool(writer, value)
    }

    #[inline]
    fn write_i8<W>(&mut self, writer: &mut W, value: i8) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        self.inner.write_i8(writer, value)
    }

    #[inline]
    fn write_i16<W>(&mut self, writer: &mut W, value: i16) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        self.inner.write_i16(writer, value)
    }

    #[inline]
    fn write_i32<W>(&mut self, writer: &mut W, value: i32) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        self.inner.write_i32(writer, value)
    }

    #[inline]
    fn write_i64<W>(&mut self, writer: &mut W, value: i64) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        self.inner.write_i64(writer, value)
    }

    #[inline]
    fn write_i128<W>(&mut self, writer: &mut W, value: i128) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        self.inner.write_i128(writer, value)
    }

    #[inline]
    fn write_u8<W>(&mut self, writer: &mut W, value: u8) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        self.inner.write_u8(writer, value)
    }

    #[inline]
    fn write_u16<W>(&mut self, writer: &mut W, value: u16) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        self.inner.write_u16(writer, value)
    }

    #[inline]
    fn write_u32<W>(&mut self, writer: &mut W, value: u32) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        self.inner.write_u32(writer, value)
    }

    #[inline]
    fn write_u64<W>(&mut self, writer: &mut W, value: u64) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        self.inner.write_u64(writer, value)
    }

    #[inline]
    fn write_u128<W>(&mut self, writer: &mut W, value: u128) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        self.inner.write_u128(writer, value)
    }

    #[inline]
    fn write_f32<W>(&mut self, writer: &mut W, value: f32) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        self.inner.write_f32(writer, value)
    }

    #[inline]
    fn write_f64<W>(&mut self, writer: &mut W, value: f64) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        self.inner.write_f64(writer, value)
    }

    #[inline]
    fn write_number_str<W>(&mut self, writer: &mut W, value: &str) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        self.inner.write_number_str(writer, value)
    }

    #[inline]
    fn begin_string<W>(&mut self, writer: &mut W) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        self.inner.begin_string(writer)
    }

    #[inline]
    fn end_string<W>(&mut self, writer: &mut W) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        self.inner.end_string(writer)
    }

    /// Re-escapes the "unescaped" fragment that `serde_json` produced, escaping
    /// the HTML-sensitive ASCII characters and all non-ASCII code points the
    /// same way C# `JavaScriptEncoder.Default` does.
    fn write_string_fragment<W>(&mut self, writer: &mut W, fragment: &str) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        let bytes = fragment.as_bytes();
        let mut start = 0;
        for (index, ch) in fragment.char_indices() {
            if ch.is_ascii() {
                let byte = ch as u8;
                if is_html_sensitive(byte) {
                    if start < index {
                        writer.write_all(&bytes[start..index])?;
                    }
                    write_unicode_escape(writer, u16::from(byte))?;
                    start = index + 1;
                }
                // Other ASCII characters are already safe and pass through.
            } else {
                if start < index {
                    writer.write_all(&bytes[start..index])?;
                }
                // Emit one `\uXXXX` per UTF-16 code unit (surrogate pair for
                // astral-plane characters), matching `Utf8JsonWriter`.
                let mut buf = [0u16; 2];
                for unit in ch.encode_utf16(&mut buf) {
                    write_unicode_escape(writer, *unit)?;
                }
                start = index + ch.len_utf8();
            }
        }
        if start < bytes.len() {
            writer.write_all(&bytes[start..])?;
        }
        Ok(())
    }

    /// Renders the JSON-mandatory escapes the way `Utf8JsonWriter` does: short
    /// forms for `\b \t \n \f \r \\`, and `"` for the quote character.
    fn write_char_escape<W>(&mut self, writer: &mut W, char_escape: CharEscape) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        match char_escape {
            CharEscape::Quote => writer.write_all(b"\\u0022"),
            CharEscape::ReverseSolidus => writer.write_all(b"\\\\"),
            CharEscape::Solidus => writer.write_all(b"/"),
            CharEscape::Backspace => writer.write_all(b"\\b"),
            CharEscape::FormFeed => writer.write_all(b"\\f"),
            CharEscape::LineFeed => writer.write_all(b"\\n"),
            CharEscape::CarriageReturn => writer.write_all(b"\\r"),
            CharEscape::Tab => writer.write_all(b"\\t"),
            CharEscape::AsciiControl(byte) => write_unicode_escape(writer, u16::from(byte)),
        }
    }

    #[inline]
    fn write_byte_array<W>(&mut self, writer: &mut W, value: &[u8]) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        self.inner.write_byte_array(writer, value)
    }

    #[inline]
    fn begin_array<W>(&mut self, writer: &mut W) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        self.inner.begin_array(writer)
    }

    #[inline]
    fn end_array<W>(&mut self, writer: &mut W) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        self.inner.end_array(writer)
    }

    #[inline]
    fn begin_array_value<W>(&mut self, writer: &mut W, first: bool) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        self.inner.begin_array_value(writer, first)
    }

    #[inline]
    fn end_array_value<W>(&mut self, writer: &mut W) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        self.inner.end_array_value(writer)
    }

    #[inline]
    fn begin_object<W>(&mut self, writer: &mut W) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        self.inner.begin_object(writer)
    }

    #[inline]
    fn end_object<W>(&mut self, writer: &mut W) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        self.inner.end_object(writer)
    }

    #[inline]
    fn begin_object_key<W>(&mut self, writer: &mut W, first: bool) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        self.inner.begin_object_key(writer, first)
    }

    #[inline]
    fn end_object_key<W>(&mut self, writer: &mut W) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        self.inner.end_object_key(writer)
    }

    #[inline]
    fn begin_object_value<W>(&mut self, writer: &mut W) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        self.inner.begin_object_value(writer)
    }

    #[inline]
    fn end_object_value<W>(&mut self, writer: &mut W) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        self.inner.end_object_value(writer)
    }

    #[inline]
    fn write_raw_fragment<W>(&mut self, writer: &mut W, fragment: &str) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        self.inner.write_raw_fragment(writer, fragment)
    }
}

/// Serializes `value` to a UTF-8 byte vector using C#-compatible escaping.
///
/// When `indented` is `true`, the output is pretty-printed with a two-space
/// indent (matching `Neo.Json` `ToByteArray(indented: true)`); otherwise it is
/// compact (matching `ToString()`).
pub fn to_vec<T>(value: &T, indented: bool) -> Result<Vec<u8>, serde_json::Error>
where
    T: ?Sized + Serialize,
{
    let mut buffer = Vec::new();
    to_writer(&mut buffer, value, indented)?;
    Ok(buffer)
}

/// Serializes `value` to the given writer using C#-compatible escaping.
pub fn to_writer<W, T>(writer: W, value: &T, indented: bool) -> Result<(), serde_json::Error>
where
    W: Write,
    T: ?Sized + Serialize,
{
    if indented {
        let formatter = CSharpEscapeFormatter::pretty(b"  ");
        let mut serializer = serde_json::Serializer::with_formatter(writer, formatter);
        value.serialize(&mut serializer)
    } else {
        let formatter = CSharpEscapeFormatter::compact();
        let mut serializer = serde_json::Serializer::with_formatter(writer, formatter);
        value.serialize(&mut serializer)
    }
}

/// Serializes `value` to a `String` using compact C#-compatible escaping.
///
/// The output is always valid UTF-8 because every byte written is ASCII (all
/// non-ASCII code points are emitted as `\uXXXX`).
pub fn to_string<T>(value: &T, indented: bool) -> Result<String, serde_json::Error>
where
    T: ?Sized + Serialize,
{
    use serde::ser::Error as _;
    let bytes = to_vec(value, indented)?;
    // The formatter only emits ASCII bytes, so this conversion never fails; we
    // still surface any (impossible) error as a serde error rather than panicking.
    String::from_utf8(bytes).map_err(|err| serde_json::Error::custom(err.to_string()))
}

#[cfg(test)]
#[path = "../tests/json/escape.rs"]
mod tests;
