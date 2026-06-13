//! `JPathToken` - matches C# Neo.Json.JPathToken exactly

use crate::json::error::JsonError;
use crate::json::j_path_token_type::JPathTokenType;
use crate::json::j_token::JToken;

/// JSON path token (matches C# `JPathToken`)
#[derive(Clone, Debug)]
pub struct JPathToken {
    /// The type of this path token.
    pub token_type: JPathTokenType,
    /// Optional content payload (identifier name, string literal, or number).
    pub content: Option<String>,
}

impl JPathToken {
    /// Returns `true` if this token is the JSONPath root (`$`).
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
                    // C# ParseString returns the literal INCLUDING both quotes;
                    // advance `i += content.len() - 1` (chars), matching
                    // JPathToken.cs:49-50.
                    let content = Self::parse_string(&chars, i)?;
                    let advance = content.chars().count().saturating_sub(1);
                    token.content = Some(content);
                    i += advance;
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

    /// Mirrors C# `JPathToken.ParseString` (JPathToken.cs:72-82): `start` is the
    /// opening-quote index; scanning begins at `start + 1` and the returned slice
    /// is INCLUSIVE of both surrounding quotes.
    fn parse_string(chars: &[char], start: usize) -> Result<String, JsonError> {
        let mut end = start + 1;
        while end < chars.len() {
            let c = chars[end];
            end += 1;
            if c == '\'' {
                return Ok(chars[start..end].iter().collect());
            }
        }
        Err(JsonError::format("Unterminated string"))
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

    /// Evaluates a parsed JSONPath token stream against `root`.
    ///
    /// Ports C# `JPathToken.ProcessJsonPath` (JPathToken.cs:119-138) verbatim:
    /// the working set is seeded with `[root]`, the first token MUST be `Root`,
    /// and the remaining tokens are dispatched on `Dot` / `LeftBracket` only.
    /// The shared `max_depth` (6) is decremented on every descent and the
    /// working set is capped at `max_objects` (1024) after each descent — these
    /// are the C# DoS bounds, and they are consensus-load-bearing for the Oracle
    /// filter.
    ///
    /// Null JSON values are preserved throughout (C# operates on `JToken?[]`):
    /// because this codebase deserializes JSON `null` into `Some(JToken::Null)`,
    /// the `.as_ref()`/`get` accessors below keep null entries (e.g.
    /// `$.store..price` includes the trailing `null`), matching C#.
    pub fn evaluate<'a>(
        tokens: &'a [Self],
        root: &'a JToken,
    ) -> Result<Vec<&'a JToken>, JsonError> {
        let mut objects: Vec<&JToken> = vec![root];
        let mut pos = 0usize;
        let first = tokens
            .get(pos)
            .ok_or_else(|| JsonError::format("Unexpected end of expression"))?;
        if first.token_type != JPathTokenType::Root {
            return Err(JsonError::format(format!(
                "Unexpected token {:?}",
                first.token_type
            )));
        }
        pos += 1;

        let mut max_depth: i32 = 6;
        let max_objects: usize = 1024;

        while pos < tokens.len() {
            let token = Self::dequeue(tokens, &mut pos)?;
            match token.token_type {
                JPathTokenType::Dot => {
                    Self::process_dot(&mut objects, &mut max_depth, max_objects, tokens, &mut pos)?;
                }
                JPathTokenType::LeftBracket => {
                    Self::process_bracket(
                        &mut objects,
                        &mut max_depth,
                        max_objects,
                        tokens,
                        &mut pos,
                    )?;
                }
                other => {
                    return Err(JsonError::format(format!("Unexpected token {other:?}")));
                }
            }
        }
        Ok(objects)
    }

    /// Pops the next token, mirroring C# `DequeueToken` (JPathToken.cs:112-117).
    fn dequeue<'a>(tokens: &'a [Self], pos: &mut usize) -> Result<&'a Self, JsonError> {
        let t = tokens
            .get(*pos)
            .ok_or_else(|| JsonError::format("Unexpected end of expression"))?;
        *pos += 1;
        Ok(t)
    }

    /// `ProcessDot` (JPathToken.cs:140-157): `.*`, `..ident`, or `.ident`.
    fn process_dot<'a>(
        objects: &mut Vec<&'a JToken>,
        max_depth: &mut i32,
        max_objects: usize,
        tokens: &'a [Self],
        pos: &mut usize,
    ) -> Result<(), JsonError> {
        let token = Self::dequeue(tokens, pos)?;
        match token.token_type {
            JPathTokenType::Asterisk => Self::descent_all(objects, max_depth, max_objects),
            JPathTokenType::Dot => {
                Self::process_recursive_descent(objects, max_depth, max_objects, tokens, pos)
            }
            JPathTokenType::Identifier => {
                let name = token
                    .content
                    .as_deref()
                    .ok_or_else(|| JsonError::format("Identifier missing content"))?;
                Self::descent_names(objects, max_depth, max_objects, &[name.to_string()])
            }
            other => Err(JsonError::format(format!("Unexpected token {other:?}"))),
        }
    }

    /// `ProcessBracket` (JPathToken.cs:159-207): `[*]`, `[:..]`, `[n..]`/`[n]`/
    /// `[n,..]`, `['s']`/`['s',..]`.
    fn process_bracket<'a>(
        objects: &mut Vec<&'a JToken>,
        max_depth: &mut i32,
        max_objects: usize,
        tokens: &'a [Self],
        pos: &mut usize,
    ) -> Result<(), JsonError> {
        let token = Self::dequeue(tokens, pos)?;
        match token.token_type {
            JPathTokenType::Asterisk => {
                let rb = Self::dequeue(tokens, pos)?;
                if rb.token_type != JPathTokenType::RightBracket {
                    return Err(JsonError::format(format!(
                        "Unexpected token {:?}",
                        rb.token_type
                    )));
                }
                Self::descent_all(objects, max_depth, max_objects)
            }
            JPathTokenType::Colon => {
                Self::process_slice(objects, max_depth, max_objects, tokens, pos, 0)
            }
            JPathTokenType::Number => {
                let next = Self::dequeue(tokens, pos)?;
                match next.token_type {
                    JPathTokenType::Colon => {
                        let start = Self::parse_i32(token)?;
                        Self::process_slice(objects, max_depth, max_objects, tokens, pos, start)
                    }
                    JPathTokenType::Comma => {
                        Self::process_union(objects, max_depth, max_objects, tokens, pos, token)
                    }
                    JPathTokenType::RightBracket => {
                        let idx = Self::parse_i32(token)?;
                        Self::descent_indexes(objects, max_depth, max_objects, &[idx])
                    }
                    other => Err(JsonError::format(format!("Unexpected token {other:?}"))),
                }
            }
            JPathTokenType::String => {
                let next = Self::dequeue(tokens, pos)?;
                match next.token_type {
                    JPathTokenType::Comma => {
                        Self::process_union(objects, max_depth, max_objects, tokens, pos, token)
                    }
                    JPathTokenType::RightBracket => {
                        let name = Self::unescape_quoted_key(token)?;
                        Self::descent_names(objects, max_depth, max_objects, &[name])
                    }
                    other => Err(JsonError::format(format!("Unexpected token {other:?}"))),
                }
            }
            other => Err(JsonError::format(format!("Unexpected token {other:?}"))),
        }
    }

    /// `ProcessRecursiveDescent` (JPathToken.cs:209-223): `$..ident` collects the
    /// property `ident` at every depth, flattening the working set after each
    /// pass via `Descent` (no-arg). The `max_objects` guard inside the loop is the
    /// DoS bound for unbounded recursion.
    fn process_recursive_descent<'a>(
        objects: &mut Vec<&'a JToken>,
        max_depth: &mut i32,
        max_objects: usize,
        tokens: &'a [Self],
        pos: &mut usize,
    ) -> Result<(), JsonError> {
        let token = Self::dequeue(tokens, pos)?;
        if token.token_type != JPathTokenType::Identifier {
            return Err(JsonError::format(format!(
                "Unexpected token {:?}",
                token.token_type
            )));
        }
        let key = token
            .content
            .as_deref()
            .ok_or_else(|| JsonError::format("Identifier missing content"))?;
        let mut results: Vec<&JToken> = Vec::new();
        while !objects.is_empty() {
            for obj in objects.iter() {
                if let JToken::Object(o) = obj {
                    for (k, v) in o.iter() {
                        if k == key {
                            if let Some(val) = v.as_ref() {
                                results.push(val);
                            }
                        }
                    }
                }
            }
            Self::descent_all(objects, max_depth, max_objects)?;
            if results.len() > max_objects {
                return Err(JsonError::format("maxObjects"));
            }
        }
        *objects = results;
        Ok(())
    }

    /// `ProcessSlice` (JPathToken.cs:225-242): `[start:end]` / `[start:]`.
    fn process_slice<'a>(
        objects: &mut Vec<&'a JToken>,
        max_depth: &mut i32,
        max_objects: usize,
        tokens: &'a [Self],
        pos: &mut usize,
        start: i32,
    ) -> Result<(), JsonError> {
        let token = Self::dequeue(tokens, pos)?;
        match token.token_type {
            JPathTokenType::Number => {
                let next = Self::dequeue(tokens, pos)?;
                if next.token_type != JPathTokenType::RightBracket {
                    return Err(JsonError::format(format!(
                        "Unexpected token {:?}",
                        next.token_type
                    )));
                }
                let end = Self::parse_i32(token)?;
                Self::descent_range(objects, max_depth, max_objects, start, end)
            }
            JPathTokenType::RightBracket => {
                Self::descent_range(objects, max_depth, max_objects, start, 0)
            }
            other => Err(JsonError::format(format!("Unexpected token {other:?}"))),
        }
    }

    /// `ProcessUnion` (JPathToken.cs:244-271): `[a,b,..]` for numbers or strings.
    fn process_union<'a>(
        objects: &mut Vec<&'a JToken>,
        max_depth: &mut i32,
        max_objects: usize,
        tokens: &'a [Self],
        pos: &mut usize,
        first: &Self,
    ) -> Result<(), JsonError> {
        let first_type = first.token_type;
        let mut items: Vec<&Self> = vec![first];
        loop {
            let token = Self::dequeue(tokens, pos)?;
            if token.token_type != first_type {
                return Err(JsonError::format(format!(
                    "Unexpected token {:?} != {first_type:?}",
                    token.token_type
                )));
            }
            items.push(token);
            let token = Self::dequeue(tokens, pos)?;
            if token.token_type == JPathTokenType::RightBracket {
                break;
            }
            if token.token_type != JPathTokenType::Comma {
                return Err(JsonError::format(format!(
                    "Unexpected token {:?} != Comma",
                    token.token_type
                )));
            }
        }
        match first_type {
            JPathTokenType::Number => {
                let mut idxs = Vec::with_capacity(items.len());
                for it in &items {
                    idxs.push(Self::parse_i32(it)?);
                }
                Self::descent_indexes(objects, max_depth, max_objects, &idxs)
            }
            JPathTokenType::String => {
                let mut names = Vec::with_capacity(items.len());
                for it in &items {
                    names.push(Self::unescape_quoted_key(it)?);
                }
                Self::descent_names(objects, max_depth, max_objects, &names)
            }
            other => Err(JsonError::format(format!("Unexpected token {other:?}"))),
        }
    }

    /// `Descent` (no-arg, JPathToken.cs:273-282): flatten children of every
    /// `JContainer` (BOTH objects and arrays), preserving null entries.
    fn descent_all<'a>(
        objects: &mut Vec<&'a JToken>,
        max_depth: &mut i32,
        max_objects: usize,
    ) -> Result<(), JsonError> {
        if *max_depth <= 0 {
            return Err(JsonError::format("Exceeded max depth"));
        }
        *max_depth -= 1;
        let mut next: Vec<&JToken> = Vec::new();
        for obj in objects.iter() {
            match obj {
                JToken::Array(a) => {
                    for child in a.children() {
                        if let Some(v) = child.as_ref() {
                            next.push(v);
                        }
                    }
                }
                JToken::Object(o) => {
                    for (_, v) in o.iter() {
                        if let Some(val) = v.as_ref() {
                            next.push(val);
                        }
                    }
                }
                _ => {}
            }
        }
        *objects = next;
        if objects.len() > max_objects {
            return Err(JsonError::format("maxObjects"));
        }
        Ok(())
    }

    /// `Descent(params string[] names)` (JPathToken.cs:284-300): for each object,
    /// yield `obj[name]` for every existing name (including null values).
    fn descent_names<'a>(
        objects: &mut Vec<&'a JToken>,
        max_depth: &mut i32,
        max_objects: usize,
        names: &[String],
    ) -> Result<(), JsonError> {
        if *max_depth <= 0 {
            return Err(JsonError::format("Exceeded max depth"));
        }
        *max_depth -= 1;
        let mut next: Vec<&JToken> = Vec::new();
        for obj in objects.iter() {
            if let JToken::Object(o) = obj {
                for name in names {
                    if o.contains_property(name) {
                        if let Some(v) = o.get(name) {
                            next.push(v);
                        }
                    }
                }
            }
        }
        *objects = next;
        if objects.len() > max_objects {
            return Err(JsonError::format("maxObjects"));
        }
        Ok(())
    }

    /// `Descent(params int[] indexes)` (JPathToken.cs:302-321): negative indices
    /// normalize via `index + count`; out-of-range indices are dropped.
    fn descent_indexes<'a>(
        objects: &mut Vec<&'a JToken>,
        max_depth: &mut i32,
        max_objects: usize,
        indexes: &[i32],
    ) -> Result<(), JsonError> {
        if *max_depth <= 0 {
            return Err(JsonError::format("Exceeded max depth"));
        }
        *max_depth -= 1;
        let mut next: Vec<&JToken> = Vec::new();
        for obj in objects.iter() {
            if let JToken::Array(a) = obj {
                let count = a.len() as i32;
                for &index in indexes {
                    let i = if index >= 0 { index } else { index + count };
                    if i >= 0 && i < count {
                        if let Some(v) = a.get(i as usize) {
                            next.push(v);
                        }
                    }
                }
            }
        }
        *objects = next;
        if objects.len() > max_objects {
            return Err(JsonError::format("maxObjects"));
        }
        Ok(())
    }

    /// `DescentRange` (JPathToken.cs:323-339): slice normalization mirrors C#
    /// `iStart = start>=0?start:start+count; if iStart<0 iStart=0;
    /// iEnd = end>0?end:end+count; Skip(iStart).Take(iEnd-iStart)`.
    fn descent_range<'a>(
        objects: &mut Vec<&'a JToken>,
        max_depth: &mut i32,
        max_objects: usize,
        start: i32,
        end: i32,
    ) -> Result<(), JsonError> {
        if *max_depth <= 0 {
            return Err(JsonError::format("Exceeded max depth"));
        }
        *max_depth -= 1;
        let mut next: Vec<&JToken> = Vec::new();
        for obj in objects.iter() {
            if let JToken::Array(a) = obj {
                let count = a.len() as i32;
                let mut i_start = if start >= 0 { start } else { start + count };
                if i_start < 0 {
                    i_start = 0;
                }
                let i_end = if end > 0 { end } else { end + count };
                let take = i_end - i_start;
                if take > 0 {
                    let begin = i_start as usize;
                    let stop = ((i_start + take) as usize).min(a.len());
                    for k in begin..stop {
                        if let Some(v) = a.get(k) {
                            next.push(v);
                        }
                    }
                }
            }
        }
        *objects = next;
        if objects.len() > max_objects {
            return Err(JsonError::format("maxObjects"));
        }
        Ok(())
    }

    /// Parses a `Number` token's content into `i32` (C# `int.Parse`).
    fn parse_i32(token: &Self) -> Result<i32, JsonError> {
        token
            .content
            .as_deref()
            .ok_or_else(|| JsonError::format("Number missing content"))?
            .parse::<i32>()
            .map_err(|_| JsonError::format("Invalid number in JSONPath"))
    }

    /// Unescapes a quoted-key literal, mirroring C#
    /// `JToken.Parse($"\"{Content.Trim('\'')}\"")!.GetString()`
    /// (JPathToken.cs:198,266). The stored content includes the surrounding
    /// single quotes, so they are trimmed before being wrapped in double quotes
    /// and parsed back as a JSON string.
    fn unescape_quoted_key(token: &Self) -> Result<String, JsonError> {
        let raw = token
            .content
            .as_deref()
            .ok_or_else(|| JsonError::format("String missing content"))?;
        let inner = raw.trim_matches('\'');
        let json = format!("\"{inner}\"");
        match JToken::parse(&json, crate::json::j_token::MAX_JSON_DEPTH)? {
            JToken::String(s) => Ok(s),
            _ => Err(JsonError::format("Invalid string key in JSONPath")),
        }
    }
}
