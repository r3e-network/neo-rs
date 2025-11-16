/// Represents a lexical token parsed from the CLI command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandToken {
    offset: usize,
    value: String,
    quote_char: char,
}

impl CommandToken {
    pub const NO_QUOTE_CHAR: char = '\0';
    pub const NO_ESCAPED_CHAR: char = '`';

    pub fn new(offset: usize, value: String, quote_char: char) -> Self {
        Self {
            offset,
            value,
            quote_char,
        }
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn quote_char(&self) -> char {
        self.quote_char
    }

    pub fn is_indicator(&self) -> bool {
        self.quote_char == Self::NO_QUOTE_CHAR && self.value.starts_with("--")
    }

    pub fn is_white_space(&self) -> bool {
        self.quote_char == Self::NO_QUOTE_CHAR && self.value.chars().all(char::is_whitespace)
    }

    pub fn raw_value(&self) -> String {
        if self.quote_char == Self::NO_QUOTE_CHAR {
            self.value.clone()
        } else {
            format!("{}{}{}", self.quote_char, self.value, self.quote_char)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indicator_detection_requires_prefix() {
        let token = CommandToken::new(0, "--flag".into(), CommandToken::NO_QUOTE_CHAR);
        assert!(token.is_indicator());

        let token = CommandToken::new(0, "value".into(), CommandToken::NO_QUOTE_CHAR);
        assert!(!token.is_indicator());
    }

    #[test]
    fn raw_value_wraps_quotes() {
        let token = CommandToken::new(0, "value".into(), CommandToken::NO_QUOTE_CHAR);
        assert_eq!(token.raw_value(), "value");

        let quoted = CommandToken::new(0, "value".into(), '"');
        assert_eq!(quoted.raw_value(), "\"value\"");
    }
}
