//! `JPathToken` - matches C# Neo.Json.JPathToken exactly

use crate::error::JsonError;
use crate::j_path_token_type::JPathTokenType;
use crate::j_token::JToken;

/// JSON path token (matches C# `JPathToken`)
#[derive(Clone, Debug)]
pub struct JPathToken {
    pub token_type: JPathTokenType,
    pub content: Option<String>,
}

impl JPathToken {
    #[must_use] 
    pub const fn is_root(&self) -> bool {
        matches!(self.token_type, JPathTokenType::Root)
    }

    /// Parse JSON path expression into tokens
    pub fn parse(expr: &str) -> Result<Vec<Self>, JsonError> {
        if expr.is_empty() {
            return Err(JsonError::format("JSONPath expression cannot be empty"));
        }

        let mut tokens = Vec::new();
        let chars: Vec<char> = expr.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            let mut token = Self {
                token_type: JPathTokenType::Root,
                content: None,
            };

            match chars[i] {
                '$' => token.token_type = JPathTokenType::Root,
                '.' => token.token_type = JPathTokenType::Dot,
                '[' => token.token_type = JPathTokenType::LeftBracket,
                ']' => token.token_type = JPathTokenType::RightBracket,
                '*' => token.token_type = JPathTokenType::Asterisk,
                ',' => token.token_type = JPathTokenType::Comma,
                ':' => token.token_type = JPathTokenType::Colon,
                '\'' => {
                    token.token_type = JPathTokenType::String;
                    let (content, len) = Self::parse_string(&chars, i + 1)?;
                    token.content = Some(content);
                    i += len;
                }
                '_' | 'a'..='z' | 'A'..='Z' => {
                    token.token_type = JPathTokenType::Identifier;
                    let (content, len) = Self::parse_identifier(&chars, i);
                    token.content = Some(content);
                    i += len - 1;
                }
                '-' | '0'..='9' => {
                    token.token_type = JPathTokenType::Number;
                    let (content, len) = Self::parse_number(&chars, i);
                    token.content = Some(content);
                    i += len - 1;
                }
                ch => {
                    return Err(JsonError::format(format!(
                        "Invalid character '{ch}' at position {i}"
                    )));
                }
            }

            tokens.push(token);
            i += 1;
        }

        Ok(tokens)
    }

    fn parse_string(chars: &[char], start: usize) -> Result<(String, usize), JsonError> {
        let mut end = start;
        while end < chars.len() {
            if chars[end] == '\'' {
                let slice: String = chars[start..end].iter().collect();
                return Ok((slice, end - start));
            }
            end += 1;
        }
        Err(JsonError::format("Unterminated string literal in JSONPath"))
    }

    fn parse_identifier(chars: &[char], start: usize) -> (String, usize) {
        let mut end = start;
        while end < chars.len() {
            match chars[end] {
                '_' | 'a'..='z' | 'A'..='Z' | '0'..='9' => end += 1,
                _ => break,
            }
        }
        (chars[start..end].iter().collect(), end - start)
    }

    fn parse_number(chars: &[char], start: usize) -> (String, usize) {
        let mut end = start;
        if end < chars.len() && chars[end] == '-' {
            end += 1;
        }
        while end < chars.len() && chars[end].is_ascii_digit() {
            end += 1;
        }
        (chars[start..end].iter().collect(), end - start)
    }

    /// Process JSON path on objects
    pub fn evaluate<'a>(
        tokens: &'a [Self],
        root: &'a JToken,
    ) -> Result<Vec<&'a JToken>, JsonError> {
        if tokens.is_empty() {
            return Ok(Vec::new());
        }

        let mut results: Vec<&JToken> = Vec::new();
        let mut i = 0;

        while i < tokens.len() {
            match tokens[i].token_type {
                JPathTokenType::Root => {
                    results.clear();
                    results.push(root);
                    i += 1;
                }
                JPathTokenType::Dot => {
                    i += 1;
                }
                JPathTokenType::Identifier => {
                    let name = tokens[i]
                        .content
                        .as_ref()
                        .ok_or_else(|| JsonError::format("Identifier token missing content"))?;
                    let mut new_results = Vec::new();
                    for token in &results {
                        if let JToken::Object(object) = token {
                            if let Some(value) = object.get(name) {
                                new_results.push(value);
                            }
                        }
                    }
                    results = new_results;
                    i += 1;
                }
                JPathTokenType::LeftBracket => {
                    i += 1;
                    if i >= tokens.len() {
                        return Err(JsonError::format("Unclosed '[' in JSONPath expression"));
                    }
                    let next = &tokens[i];
                    let mut new_results = Vec::new();
                    match next.token_type {
                        JPathTokenType::Number => {
                            let index_str = next
                                .content
                                .as_ref()
                                .ok_or_else(|| JsonError::format("Array index missing"))?;
                            let index: usize = index_str.parse().map_err(|_| {
                                JsonError::format("Invalid array index in JSONPath expression")
                            })?;
                            for token in &results {
                                if let JToken::Array(array) = token {
                                    if let Some(value) = array.get(index) {
                                        new_results.push(value);
                                    }
                                }
                            }
                            i += 1;
                        }
                        JPathTokenType::Asterisk => {
                            for token in &results {
                                match token {
                                    JToken::Array(array) => {
                                        for child in array.children() {
                                            if let Some(value) = child.as_ref() {
                                                new_results.push(value);
                                            }
                                        }
                                    }
                                    JToken::Object(object) => {
                                        for child in object.children() {
                                            if let Some(value) = child.as_ref() {
                                                new_results.push(value);
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            i += 1;
                        }
                        _ => return Err(JsonError::format("Unsupported bracket expression")),
                    }

                    if i >= tokens.len() || tokens[i].token_type != JPathTokenType::RightBracket {
                        return Err(JsonError::format("Missing ']' in JSONPath expression"));
                    }
                    i += 1; // consume right bracket
                    results = new_results;
                }
                JPathTokenType::RightBracket | JPathTokenType::Comma | JPathTokenType::Colon => {
                    i += 1;
                }
                _ => {
                    return Err(JsonError::format(
                        "Unsupported token in JSONPath expression",
                    ));
                }
            }
        }

        Ok(results)
    }
}
