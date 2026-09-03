//! `JPathTokenType` - matches C# Neo.Json.JPathTokenType exactly

/// JSON path token types (matches C# `JPathTokenType` enum)
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JPathTokenType {
    /// The root token (`$`).
    Root = 0,
    /// A dot separator (`.`).
    Dot = 1,
    /// A left bracket (`[`).
    LeftBracket = 2,
    /// A right bracket (`]`).
    RightBracket = 3,
    /// A wildcard (`*`).
    Asterisk = 4,
    /// A comma separator (`,`).
    Comma = 5,
    /// A colon separator (`:`).
    Colon = 6,
    /// A property or field name.
    Identifier = 7,
    /// A quoted string literal.
    String = 8,
    /// A numeric index or value.
    Number = 9,
}
