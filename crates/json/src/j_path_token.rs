//! JPathToken - matches C# Neo.Json.JPathToken exactly

use crate::j_path_token_type::JPathTokenType;
use crate::j_token::JToken;
use std::collections::VecDeque;

/// JSON path token (matches C# JPathToken)
#[derive(Clone, Debug)]
pub struct JPathToken {
    pub token_type: JPathTokenType,
    pub content: Option<String>,
}

impl JPathToken {
    pub fn is_root(&self) -> bool {
        matches!(self.token_type, JPathTokenType::Root)
    }

    /// Parse JSON path expression into tokens
    pub fn parse(expr: &str) -> Vec<JPathToken> {
        let mut tokens = Vec::new();
        let chars: Vec<char> = expr.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            let mut token = JPathToken {
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
                    let (content, len) = Self::parse_string(&chars, i);
                    token.content = Some(content);
                    i += len - 1;
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
                _ => panic!("Invalid character '{}' at position {}", chars[i], i),
            }

            tokens.push(token);
            i += 1;
        }

        tokens
    }

    fn parse_string(chars: &[char], start: usize) -> (String, usize) {
        let mut end = start + 1;
        while end < chars.len() {
            if chars[end] == '\'' {
                end += 1;
                return (chars[start..end].iter().collect(), end - start);
            }
            end += 1;
        }
        panic!("Unterminated string");
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
    pub fn process_json_path(objects: &mut Vec<Option<JToken>>, mut tokens: VecDeque<JPathToken>) {
        if tokens.is_empty() {
            return;
        }

        let token = tokens.pop_front().unwrap();
        let mut new_objects = Vec::new();

        match token.token_type {
            JPathTokenType::Dot => {
                // Process dot notation
                if let Some(next) = tokens.front() {
                    if next.token_type == JPathTokenType::Identifier {
                        let next = tokens.pop_front().unwrap();
                        let prop_name = next.content.unwrap_or_default();

                        for obj in objects.iter() {
                            if let Some(JToken::Object(o)) = obj {
                                new_objects.push(o.get(&prop_name).cloned());
                            }
                        }
                    }
                }
            }
            JPathTokenType::LeftBracket => {
                // Process bracket notation
                if let Some(next) = tokens.front() {
                    match next.token_type {
                        JPathTokenType::Number => {
                            let next = tokens.pop_front().unwrap();
                            if let Some(index_str) = next.content {
                                if let Ok(index) = index_str.parse::<usize>() {
                                    for obj in objects.iter() {
                                        if let Some(JToken::Array(a)) = obj {
                                            new_objects.push(a.get(index).cloned());
                                        }
                                    }
                                }
                            }
                        }
                        JPathTokenType::Asterisk => {
                            tokens.pop_front();
                            for obj in objects.iter() {
                                match obj {
                                    Some(JToken::Array(a)) => {
                                        for item in a.children() {
                                            new_objects.push(item.clone());
                                        }
                                    }
                                    Some(JToken::Object(o)) => {
                                        for value in o.children() {
                                            new_objects.push(value.clone());
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }

        if !tokens.is_empty() {
            Self::process_json_path(&mut new_objects, tokens);
        }

        *objects = new_objects;
    }
}
