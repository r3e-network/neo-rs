/// Represents the vm_types of tokens in a JSON Path expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JPathTokenType {
    Root,
    Dot,
    LeftBracket,
    RightBracket,
    Asterisk,
    Comma,
    Colon,
    Identifier,
    String,
    Number,
}

impl JPathTokenType {
    /// Converts the enum to a byte representation.
    pub fn as_byte(&self) -> u8 {
        match self {
            JPathTokenType::Root => 0,
            JPathTokenType::Dot => 1,
            JPathTokenType::LeftBracket => 2,
            JPathTokenType::RightBracket => 3,
            JPathTokenType::Asterisk => 4,
            JPathTokenType::Comma => 5,
            JPathTokenType::Colon => 6,
            JPathTokenType::Identifier => 7,
            JPathTokenType::String => 8,
            JPathTokenType::Number => 9,
        }
    }

    /// Creates a JPathTokenType from a byte value.
    pub fn from_byte(value: u8) -> Option<Self> {
        match value {
            0 => Some(JPathTokenType::Root),
            1 => Some(JPathTokenType::Dot),
            2 => Some(JPathTokenType::LeftBracket),
            3 => Some(JPathTokenType::RightBracket),
            4 => Some(JPathTokenType::Asterisk),
            5 => Some(JPathTokenType::Comma),
            6 => Some(JPathTokenType::Colon),
            7 => Some(JPathTokenType::Identifier),
            8 => Some(JPathTokenType::String),
            9 => Some(JPathTokenType::Number),
            _ => None,
        }
    }
}
