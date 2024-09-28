use std::collections::VecDeque;
use std::convert::{TryFrom, TryInto};
use std::str::FromStr;

use crate::json_error::JsonError;
use crate::jtoken::JToken;

#[derive(Debug, Clone, PartialEq)]
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

#[derive(Debug, Clone, PartialEq)]
pub struct JPathToken {
    token_type: JPathTokenType,
    content:    Option<String>,
}

impl JPathToken {
    fn new(token_type: JPathTokenType, content: Option<String>) -> Self {
        JPathToken { token_type, content }
    }

    fn parse(expr: &str) -> Result<Vec<JPathToken>, JsonError> {
        let mut tokens = Vec::new();
        let mut chars = expr.chars().peekable();

        while let Some(c) = chars.next() {
            match c {
                '$' => tokens.push(JPathToken::new(JPathTokenType::Root, None)),
                '.' => tokens.push(JPathToken::new(JPathTokenType::Dot, None)),
                '[' => tokens.push(JPathToken::new(JPathTokenType::LeftBracket, None)),
                ']' => tokens.push(JPathToken::new(JPathTokenType::RightBracket, None)),
                '*' => tokens.push(JPathToken::new(JPathTokenType::Asterisk, None)),
                ',' => tokens.push(JPathToken::new(JPathTokenType::Comma, None)),
                ':' => tokens.push(JPathToken::new(JPathTokenType::Colon, None)),
                '\'' => {
                    let content: String = chars.by_ref().take_while(|&ch| ch != '\'').collect();
                    tokens.push(JPathToken::new(JPathTokenType::String, Some(content)));
                    chars.next(); // Consume the closing quote
                }
                c if c.is_ascii_alphabetic() || c == '_' => {
                    let mut content = c.to_string();
                    while let Some(&next) = chars.peek() {
                        if next.is_ascii_alphanumeric() || next == '_' {
                            content.push(chars.next().unwrap());
                        } else {
                            break;
                        }
                    }
                    tokens.push(JPathToken::new(JPathTokenType::Identifier, Some(content)));
                }
                c if c.is_ascii_digit() || c == '-' => {
                    let mut content = c.to_string();
                    while let Some(&next) = chars.peek() {
                        if next.is_ascii_digit() {
                            content.push(chars.next().unwrap());
                        } else {
                            break;
                        }
                    }
                    tokens.push(JPathToken::new(JPathTokenType::Number, Some(content)));
                }
                _ => return Err(JsonError::FormatError),
            }
        }

        Ok(tokens)
    }
}

pub struct JPath(Vec<JPathToken>);

impl FromStr for JPath {
    type Err = JsonError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(JPath(JPathToken::parse(s)?))
    }
}

impl JPath {
    pub fn apply(&self, token: &JToken) -> Result<Vec<JToken>, JsonError> {
        let mut objects = vec![token.clone()];
        let mut tokens = VecDeque::from(self.0.clone());

        if tokens.is_empty() {
            return Ok(objects);
        }

        let first = tokens.pop_front().unwrap();
        if first.token_type != JPathTokenType::Root {
            return Err(JsonError::FormatError);
        }

        Self::process_json_path(&mut objects, &mut tokens)
    }

    fn process_json_path(
        objects: &mut Vec<JToken>,
        tokens: &mut VecDeque<JPathToken>,
    ) -> Result<Vec<JToken>, JsonError> {
        let max_depth = 6;
        let max_objects = 1024;
        let mut depth = 0;

        while !tokens.is_empty() && depth < max_depth {
            let token = tokens.pop_front().ok_or(JsonError::FormatError)?;
            match token.token_type {
                JPathTokenType::Dot => Self::process_dot(objects, tokens, max_objects)?,
                JPathTokenType::LeftBracket => Self::process_bracket(objects, tokens, max_objects)?,
                _ => return Err(JsonError::FormatError),
            }
            depth += 1;

            if objects.len() > max_objects {
                return Err(JsonError::FormatError);
            }
        }

        Ok(objects.to_vec())
    }

    fn process_dot(
        objects: &mut Vec<JToken>,
        tokens: &mut VecDeque<JPathToken>,
        max_objects: usize,
    ) -> Result<(), JsonError> {
        let token = tokens.pop_front().ok_or(JsonError::FormatError)?;
        match token.token_type {
            JPathTokenType::Asterisk => Self::descent(objects, max_objects),
            JPathTokenType::Dot => Self::process_recursive_descent(objects, tokens, max_objects),
            JPathTokenType::Identifier => {
                Self::descent_by_name(objects, token.content.unwrap(), max_objects)
            }
            _ => Err(JsonError::FormatError),
        }
    }

    fn descent(objects: &mut Vec<JToken>, max_objects: usize) -> Result<(), JsonError> {
        let mut new_objects = Vec::new();
        for obj in objects.iter() {
            match obj {
                JToken::Array(arr) => new_objects.extend(arr.iter().cloned()),
                JToken::Object(map) => new_objects.extend(map.values().cloned()),
                _ => {}
            }
        }
        *objects = new_objects;
        if objects.len() > max_objects {
            Err(JsonError::FormatError)
        } else {
            Ok(())
        }
    }

    fn descent_by_name(
        objects: &mut Vec<JToken>,
        name: String,
        max_objects: usize,
    ) -> Result<(), JsonError> {
        let mut new_objects = Vec::new();
        for obj in objects.iter() {
            if let JToken::Object(map) = obj {
                if let Some(value) = map.get(&name) {
                    new_objects.push(value.clone());
                }
            }
        }
        *objects = new_objects;
        if objects.len() > max_objects {
            Err(JsonError::FormatError)
        } else {
            Ok(())
        }
    }

    fn process_bracket(
        objects: &mut Vec<JToken>,
        tokens: &mut VecDeque<JPathToken>,
        max_objects: usize,
    ) -> Result<(), JsonError> {
        let token = tokens.pop_front().ok_or(JsonError::FormatError)?;
        match token.token_type {
            JPathTokenType::Asterisk => {
                if tokens
                    .pop_front()
                    .map_or(false, |t| t.token_type == JPathTokenType::RightBracket)
                {
                    Self::descent(objects, max_objects)
                } else {
                    Err(JsonError::FormatError)
                }
            }
            JPathTokenType::Colon => Self::process_slice(objects, tokens, 0, max_objects),
            JPathTokenType::Number => {
                let start = token
                    .clone()
                    .content
                    .unwrap()
                    .parse::<i32>()
                    .map_err(|_| JsonError::FormatError)?;
                match tokens.pop_front().map(|t| t.token_type) {
                    Some(JPathTokenType::Colon) => {
                        Self::process_slice(objects, tokens, start, max_objects)
                    }
                    Some(JPathTokenType::Comma) => {
                        Self::process_union(objects, tokens, token, max_objects)
                    }
                    Some(JPathTokenType::RightBracket) => {
                        Self::descent_by_index(objects, start, max_objects)
                    }
                    _ => Err(JsonError::FormatError),
                }
            }
            JPathTokenType::String => match tokens.pop_front().map(|t| t.token_type) {
                Some(JPathTokenType::Comma) => {
                    Self::process_union(objects, tokens, token, max_objects)
                }
                Some(JPathTokenType::RightBracket) => {
                    let key = token.content.unwrap();
                    Self::descent_by_name(objects, key, max_objects)
                }
                _ => Err(JsonError::FormatError),
            },
            _ => Err(JsonError::FormatError),
        }
    }

    fn process_recursive_descent(
        objects: &mut Vec<JToken>,
        tokens: &mut VecDeque<JPathToken>,
        max_objects: usize,
    ) -> Result<(), JsonError> {
        let mut results = Vec::new();
        let token = tokens.pop_front().ok_or(JsonError::FormatError)?;
        if token.token_type != JPathTokenType::Identifier {
            return Err(JsonError::FormatError);
        }
        let key = token.content.unwrap();

        while !objects.is_empty() {
            let mut new_objects = Vec::new();
            for obj in objects.drain(..) {
                if let JToken::Object(map) = &obj {
                    if let Some(value) = map.get(&key) {
                        results.push(value.clone());
                    }
                }
                match obj {
                    JToken::Object(map) => new_objects.extend(map.values().cloned()),
                    JToken::Array(arr) => new_objects.extend(arr.iter().cloned()),
                    _ => {}
                }
            }
            *objects = new_objects;
            if results.len() > max_objects {
                return Err(JsonError::FormatError);
            }
        }
        *objects = results;
        Ok(())
    }

    fn process_slice(
        objects: &mut Vec<JToken>,
        tokens: &mut VecDeque<JPathToken>,
        start: i32,
        max_objects: usize,
    ) -> Result<(), JsonError> {
        let end = match tokens.pop_front() {
            Some(JPathToken { token_type: JPathTokenType::Number, content: Some(content) }) => {
                content.parse::<i32>().map_err(|_| JsonError::FormatError)?
            }
            Some(JPathToken { token_type: JPathTokenType::RightBracket, .. }) => 0,
            _ => return Err(JsonError::FormatError),
        };

        if tokens.pop_front().map_or(false, |t| t.token_type != JPathTokenType::RightBracket) {
            return Err(JsonError::FormatError);
        }

        let mut new_objects = Vec::new();
        for obj in objects.iter() {
            if let JToken::Array(arr) = obj {
                let i_start = if start >= 0 {
                    start as usize
                } else {
                    arr.len().saturating_sub((-start) as usize)
                };
                let i_end = if end > 0 {
                    end as usize
                } else {
                    arr.len().saturating_sub((-end) as usize)
                };
                new_objects
                    .extend(arr[i_start.min(arr.len())..i_end.min(arr.len())].iter().cloned());
            }
        }
        *objects = new_objects;

        if objects.len() > max_objects {
            Err(JsonError::FormatError)
        } else {
            Ok(())
        }
    }

    fn process_union(
        objects: &mut Vec<JToken>,
        tokens: &mut VecDeque<JPathToken>,
        first: JPathToken,
        max_objects: usize,
    ) -> Result<(), JsonError> {
        let mut items = vec![first.clone()];
        loop {
            match tokens.pop_front() {
                Some(token) if token.token_type == first.token_type => items.push(token),
                Some(token) if token.token_type == JPathTokenType::RightBracket => break,
                Some(token) if token.token_type == JPathTokenType::Comma => continue,
                _ => return Err(JsonError::FormatError),
            }
        }

        let mut new_objects = Vec::new();
        match first.token_type {
            JPathTokenType::Number => {
                let indices: Result<Vec<i32>, _> = items
                    .iter()
                    .filter_map(|t| t.content.as_ref())
                    .map(|s| s.parse::<i32>())
                    .collect();
                let indices = indices.map_err(|_| JsonError::FormatError)?;
                for obj in objects.iter() {
                    if let JToken::Array(arr) = obj {
                        for &index in &indices {
                            let i = if index >= 0 {
                                index as usize
                            } else {
                                arr.len().saturating_sub((-index) as usize)
                            };
                            if i < arr.len() {
                                new_objects.push(arr[i].clone());
                            }
                        }
                    }
                }
            }
            JPathTokenType::String => {
                let keys: Vec<String> = items.into_iter().filter_map(|t| t.content).collect();
                for obj in objects.iter() {
                    if let JToken::Object(map) = obj {
                        for key in &keys {
                            if let Some(value) = map.get(key) {
                                new_objects.push(value.clone());
                            }
                        }
                    }
                }
            }
            _ => return Err(JsonError::FormatError),
        }

        *objects = new_objects;

        if objects.len() > max_objects {
            Err(JsonError::FormatError)
        } else {
            Ok(())
        }
    }

    fn descent_by_index(
        objects: &mut Vec<JToken>,
        index: i32,
        max_objects: usize,
    ) -> Result<(), JsonError> {
        let mut new_objects = Vec::new();
        for obj in objects.iter() {
            if let JToken::Array(arr) = obj {
                let i = if index >= 0 {
                    index as usize
                } else {
                    arr.len().saturating_sub((-index) as usize)
                };
                if i < arr.len() {
                    new_objects.push(arr[i].clone());
                }
            }
        }
        *objects = new_objects;
        if objects.len() > max_objects {
            Err(JsonError::FormatError)
        } else {
            Ok(())
        }
    }
}

// Implementations for From and TryFrom for JToken
impl From<i32> for JToken {
    fn from(value: i32) -> Self {
        JToken::Number(value.into())
    }
}

impl From<f64> for JToken {
    fn from(value: f64) -> Self {
        JToken::Number(value.into())
    }
}

impl From<String> for JToken {
    fn from(value: String) -> Self {
        JToken::String(value)
    }
}

impl From<&str> for JToken {
    fn from(value: &str) -> Self {
        JToken::String(value.to_string())
    }
}

impl TryFrom<JToken> for i32 {
    type Error = JsonError;

    fn try_from(value: JToken) -> Result<Self, Self::Error> {
        if let JToken::Number(num) = value {
            num.as_i64().map(|n| n as i32).ok_or(JsonError::FormatError)
        } else {
            Err(JsonError::FormatError)
        }
    }
}

impl TryFrom<JToken> for f64 {
    type Error = JsonError;

    fn try_from(value: JToken) -> Result<Self, Self::Error> {
        if let JToken::Number(num) = value {
            num.as_f64().ok_or(JsonError::FormatError)
        } else {
            Err(JsonError::FormatError)
        }
    }
}

impl TryFrom<JToken> for String {
    type Error = JsonError;

    fn try_from(value: JToken) -> Result<Self, Self::Error> {
        if let JToken::String(s) = value {
            Ok(s)
        } else {
            Err(JsonError::FormatError)
        }
    }
}
