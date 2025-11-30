//! JPathTokenType - matches C# Neo.Json.JPathTokenType exactly

/// JSON path token types (matches C# JPathTokenType enum)
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JPathTokenType {
    Root = 0,
    Dot = 1,
    LeftBracket = 2,
    RightBracket = 3,
    Asterisk = 4,
    Comma = 5,
    Colon = 6,
    Identifier = 7,
    String = 8,
    Number = 9,
}
