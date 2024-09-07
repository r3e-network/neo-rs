use neo::prelude::*;
use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub enum JPathTokenType {
    Root,
    Dot,
    LeftBracket,
    RightBracket,
    Asterisk,
    Comma,
    Colon,
    String,
    Identifier,
    Number,
}

#[derive(Debug, Clone)]
pub struct JPathToken {
    token_type: JPathTokenType,
    content: Option<String>,
}

impl JPathToken {
    pub fn parse(expr: &str) -> Result<Vec<JPathToken>, FormatException> {
        let mut tokens = Vec::new();
        let mut chars = expr.chars().enumerate();

        while let Some((i, c)) = chars.next() {
            let token = match c {
                '$' => JPathToken { token_type: JPathTokenType::Root, content: None },
                '.' => JPathToken { token_type: JPathTokenType::Dot, content: None },
                '[' => JPathToken { token_type: JPathTokenType::LeftBracket, content: None },
                ']' => JPathToken { token_type: JPathTokenType::RightBracket, content: None },
                '*' => JPathToken { token_type: JPathTokenType::Asterisk, content: None },
                ',' => JPathToken { token_type: JPathTokenType::Comma, content: None },
                ':' => JPathToken { token_type: JPathTokenType::Colon, content: None },
                '\'' => {
                    let content = Self::parse_string(expr, i)?;
                    chars.nth(content.len() - 2); // Skip the parsed string
                    JPathToken { token_type: JPathTokenType::String, content: Some(content) }
                },
                '_' | 'a'..='z' | 'A'..='Z' => {
                    let content = Self::parse_identifier(expr, i);
                    chars.nth(content.len() - 2); // Skip the parsed identifier
                    JPathToken { token_type: JPathTokenType::Identifier, content: Some(content) }
                },
                '-' | '0'..='9' => {
                    let content = Self::parse_number(expr, i);
                    chars.nth(content.len() - 2); // Skip the parsed number
                    JPathToken { token_type: JPathTokenType::Number, content: Some(content) }
                },
                _ => return Err(FormatException::new("Invalid character in JPath expression")),
            };
            tokens.push(token);
        }
        Ok(tokens)
    }

    fn parse_string(expr: &str, start: usize) -> Result<String, FormatException> {
        let end = expr[start + 1..].find('\'')
            .map(|i| i + start + 1)
            .ok_or_else(|| FormatException::new("Unterminated string in JPath expression"))?;
        Ok(expr[start..=end].to_string())
    }

    fn parse_identifier(expr: &str, start: usize) -> String {
        expr[start..].chars()
            .take_while(|&c| c == '_' || c.is_ascii_alphanumeric())
            .collect()
    }

    fn parse_number(expr: &str, start: usize) -> String {
        expr[start..].chars()
            .take_while(|c| c.is_ascii_digit())
            .collect()
    }

    pub fn process_json_path(objects: &mut Vec<Option<JToken>>, tokens: &mut VecDeque<JPathToken>) -> Result<(), FormatException> {
        let mut max_depth = 6;
        let max_objects = 1024;

        while let Some(token) = tokens.pop_front() {
            match token.token_type {
                JPathTokenType::Dot => Self::process_dot(objects, &mut max_depth, max_objects, tokens)?,
                JPathTokenType::LeftBracket => Self::process_bracket(objects, &mut max_depth, max_objects, tokens)?,
                _ => return Err(FormatException::new("Unexpected token in JPath expression")),
            }
        }
        Ok(())
    }

    fn process_dot(objects: &mut Vec<Option<JToken>>, max_depth: &mut i32, max_objects: usize, tokens: &mut VecDeque<JPathToken>) -> Result<(), FormatException> {
        let token = tokens.pop_front().ok_or_else(|| FormatException::new("Unexpected end of JPath expression"))?;
        match token.token_type {
            JPathTokenType::Asterisk => Self::descent(objects, max_depth, max_objects)?,
            JPathTokenType::Dot => Self::process_recursive_descent(objects, max_depth, max_objects, tokens)?,
            JPathTokenType::Identifier => Self::descent_by_name(objects, max_depth, max_objects, &[token.content.unwrap()])?,
            _ => return Err(FormatException::new("Unexpected token after dot in JPath expression")),
        }
        Ok(())
    }

    fn process_bracket(objects: &mut Vec<Option<JToken>>, max_depth: &mut i32, max_objects: usize, tokens: &mut VecDeque<JPathToken>) -> Result<(), FormatException> {
        let token = tokens.pop_front().ok_or_else(|| FormatException::new("Unexpected end of JPath expression"))?;
        match token.token_type {
            JPathTokenType::Asterisk => {
                if tokens.pop_front().map(|t| t.token_type) != Some(JPathTokenType::RightBracket) {
                    return Err(FormatException::new("Expected right bracket after asterisk"));
                }
                Self::descent(objects, max_depth, max_objects)?;
            },
            JPathTokenType::Colon => Self::process_slice(objects, max_depth, max_objects, tokens, 0)?,
            JPathTokenType::Number => {
                let index = token.content.unwrap().parse::<i32>().map_err(|_| FormatException::new("Invalid number in JPath expression"))?;
                let next = tokens.pop_front().ok_or_else(|| FormatException::new("Unexpected end of JPath expression"))?;
                match next.token_type {
                    JPathTokenType::Colon => Self::process_slice(objects, max_depth, max_objects, tokens, index)?,
                    JPathTokenType::Comma => Self::process_union(objects, max_depth, max_objects, tokens, token)?,
                    JPathTokenType::RightBracket => Self::descent_by_index(objects, max_depth, max_objects, &[index])?,
                    _ => return Err(FormatException::new("Unexpected token after number in bracket")),
                }
            },
            JPathTokenType::String => {
                let next = tokens.pop_front().ok_or_else(|| FormatException::new("Unexpected end of JPath expression"))?;
                match next.token_type {
                    JPathTokenType::Comma => Self::process_union(objects, max_depth, max_objects, tokens, token)?,
                    JPathTokenType::RightBracket => {
                        let key = token.content.unwrap().trim_matches('\'').to_string();
                        Self::descent_by_name(objects, max_depth, max_objects, &[key])?;
                    },
                    _ => return Err(FormatException::new("Unexpected token after string in bracket")),
                }
            },
            _ => return Err(FormatException::new("Unexpected token in bracket")),
        }
        Ok(())
    }

    fn process_recursive_descent(objects: &mut Vec<Option<JToken>>, max_depth: &mut i32, max_objects: usize, tokens: &mut VecDeque<JPathToken>) -> Result<(), FormatException> {
        let token = tokens.pop_front().ok_or_else(|| FormatException::new("Unexpected end of JPath expression"))?;
        if token.token_type != JPathTokenType::Identifier {
            return Err(FormatException::new("Expected identifier after recursive descent"));
        }
        let key = token.content.unwrap();
        let mut results = Vec::new();
        while !objects.is_empty() {
            results.extend(objects.iter().filter_map(|obj| {
                obj.as_ref().and_then(|j| {
                    if let JToken::Object(o) = j {
                        o.get(&key).cloned()
                    } else {
                        None
                    }
                })
            }));
            Self::descent(objects, max_depth, max_objects)?;
            if results.len() > max_objects {
                return Err(FormatException::new("Exceeded maximum number of objects"));
            }
        }
        *objects = results.into_iter().map(Some).collect();
        Ok(())
    }

    fn process_slice(objects: &mut Vec<Option<JToken>>, max_depth: &mut i32, max_objects: usize, tokens: &mut VecDeque<JPathToken>, start: i32) -> Result<(), FormatException> {
        let token = tokens.pop_front().ok_or_else(|| FormatException::new("Unexpected end of JPath expression"))?;
        match token.token_type {
            JPathTokenType::Number => {
                if tokens.pop_front().map(|t| t.token_type) != Some(JPathTokenType::RightBracket) {
                    return Err(FormatException::new("Expected right bracket after slice end"));
                }
                let end = token.content.unwrap().parse::<i32>().map_err(|_| FormatException::new("Invalid number in slice"))?;
                Self::descent_range(objects, max_depth, max_objects, start, end)?;
            },
            JPathTokenType::RightBracket => Self::descent_range(objects, max_depth, max_objects, start, 0)?,
            _ => return Err(FormatException::new("Unexpected token in slice")),
        }
        Ok(())
    }

    fn process_union(objects: &mut Vec<Option<JToken>>, max_depth: &mut i32, max_objects: usize, tokens: &mut VecDeque<JPathToken>, first: JPathToken) -> Result<(), FormatException> {
        let mut items = vec![first];
        loop {
            let token = tokens.pop_front().ok_or_else(|| FormatException::new("Unexpected end of JPath expression"))?;
            if token.token_type != first.token_type {
                return Err(FormatException::new("Inconsistent types in union"));
            }
            items.push(token);
            let next = tokens.pop_front().ok_or_else(|| FormatException::new("Unexpected end of JPath expression"))?;
            match next.token_type {
                JPathTokenType::RightBracket => break,
                JPathTokenType::Comma => continue,
                _ => return Err(FormatException::new("Unexpected token in union")),
            }
        }
        match first.token_type {
            JPathTokenType::Number => {
                let indices: Result<Vec<i32>, _> = items.iter().map(|t| t.content.as_ref().unwrap().parse::<i32>()).collect();
                Self::descent_by_index(objects, max_depth, max_objects, &indices?)?;
            },
            JPathTokenType::String => {
                let keys: Vec<String> = items.iter().map(|t| t.content.as_ref().unwrap().trim_matches('\'').to_string()).collect();
                Self::descent_by_name(objects, max_depth, max_objects, &keys)?;
            },
            _ => return Err(FormatException::new("Unexpected token type in union")),
        }
        Ok(())
    }

    fn descent(objects: &mut Vec<Option<JToken>>, max_depth: &mut i32, max_objects: usize) -> Result<(), FormatException> {
        if *max_depth <= 0 {
            return Err(FormatException::new("Exceeded maximum depth"));
        }
        *max_depth -= 1;
        *objects = objects.iter().filter_map(|obj| {
            obj.as_ref().and_then(|j| {
                match j {
                    JToken::Array(arr) => Some(arr.iter().cloned().map(Some).collect::<Vec<_>>()),
                    JToken::Object(obj) => Some(obj.values().cloned().map(Some).collect::<Vec<_>>()),
                    _ => None,
                }
            })
        }).flatten().collect();
        if objects.len() > max_objects {
            return Err(FormatException::new("Exceeded maximum number of objects"));
        }
        Ok(())
    }

    fn descent_by_name(objects: &mut Vec<Option<JToken>>, max_depth: &mut i32, max_objects: usize, names: &[String]) -> Result<(), FormatException> {
        if *max_depth <= 0 {
            return Err(FormatException::new("Exceeded maximum depth"));
        }
        *max_depth -= 1;
        *objects = objects.iter().filter_map(|obj| {
            obj.as_ref().and_then(|j| {
                if let JToken::Object(o) = j {
                    Some(names.iter().filter_map(|name| o.get(name).cloned()).map(Some).collect::<Vec<_>>())
                } else {
                    None
                }
            })
        }).flatten().collect();
        if objects.len() > max_objects {
            return Err(FormatException::new("Exceeded maximum number of objects"));
        }
        Ok(())
    }

    fn descent_by_index(objects: &mut Vec<Option<JToken>>, max_depth: &mut i32, max_objects: usize, indices: &[i32]) -> Result<(), FormatException> {
        if *max_depth <= 0 {
            return Err(FormatException::new("Exceeded maximum depth"));
        }
        *max_depth -= 1;
        *objects = objects.iter().filter_map(|obj| {
            obj.as_ref().and_then(|j| {
                if let JToken::Array(arr) = j {
                    Some(indices.iter().filter_map(|&index| {
                        let i = if index >= 0 { index as usize } else { arr.len().saturating_sub((-index) as usize) };
                        arr.get(i).cloned()
                    }).map(Some).collect::<Vec<_>>())
                } else {
                    None
                }
            })
        }).flatten().collect();
        if objects.len() > max_objects {
            return Err(FormatException::new("Exceeded maximum number of objects"));
        }
        Ok(())
    }

    fn descent_range(objects: &mut Vec<Option<JToken>>, max_depth: &mut i32, max_objects: usize, start: i32, end: i32) -> Result<(), FormatException> {
        if *max_depth <= 0 {
            return Err(FormatException::new("Exceeded maximum depth"));
        }
        *max_depth -= 1;
        *objects = objects.iter().filter_map(|obj| {
            obj.as_ref().and_then(|j| {
namespace Neo.Json
{
    sealed class JPathToken
    {
        public JPathTokenType Type { get; private set; }
        public string? Content { get; private set; }

        public static IEnumerable<JPathToken> Parse(string expr)
        {
            for (int i = 0; i < expr.Length; i++)
            {
                JPathToken token = new();
                switch (expr[i])
                {
                    case '$':
                        token.Type = JPathTokenType.Root;
                        break;
                    case '.':
                        token.Type = JPathTokenType.Dot;
                        break;
                    case '[':
                        token.Type = JPathTokenType.LeftBracket;
                        break;
                    case ']':
                        token.Type = JPathTokenType.RightBracket;
                        break;
                    case '*':
                        token.Type = JPathTokenType.Asterisk;
                        break;
                    case ',':
                        token.Type = JPathTokenType.Comma;
                        break;
                    case ':':
                        token.Type = JPathTokenType.Colon;
                        break;
                    case '\'':
                        token.Type = JPathTokenType.String;
                        token.Content = ParseString(expr, i);
                        i += token.Content.Length - 1;
                        break;
                    case '_':
                    case >= 'a' and <= 'z':
                    case >= 'A' and <= 'Z':
                        token.Type = JPathTokenType.Identifier;
                        token.Content = ParseIdentifier(expr, i);
                        i += token.Content.Length - 1;
                        break;
                    case '-':
                    case >= '0' and <= '9':
                        token.Type = JPathTokenType.Number;
                        token.Content = ParseNumber(expr, i);
                        i += token.Content.Length - 1;
                        break;
                    default:
                        throw new FormatException();
                }
                yield return token;
            }
        }

        private static string ParseString(string expr, int start)
        {
            int end = start + 1;
            while (end < expr.Length)
            {
                char c = expr[end];
                end++;
                if (c == '\'') return expr[start..end];
            }
            throw new FormatException();
        }

        public static string ParseIdentifier(string expr, int start)
        {
            int end = start + 1;
            while (end < expr.Length)
            {
                char c = expr[end];
                if (c == '_' || c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9')
                    end++;
                else
                    break;
            }
            return expr[start..end];
        }

        private static string ParseNumber(string expr, int start)
        {
            int end = start + 1;
            while (end < expr.Length)
            {
                char c = expr[end];
                if (c >= '0' && c <= '9')
                    end++;
                else
                    break;
            }
            return expr[start..end];
        }

        private static JPathToken DequeueToken(Queue<JPathToken> tokens)
        {
            if (!tokens.TryDequeue(out JPathToken? token))
                throw new FormatException();
            return token;
        }

        public static void ProcessJsonPath(ref JToken?[] objects, Queue<JPathToken> tokens)
        {
            int maxDepth = 6;
            int maxObjects = 1024;
            while (tokens.Count > 0)
            {
                JPathToken token = DequeueToken(tokens);
                switch (token.Type)
                {
                    case JPathTokenType.Dot:
                        ProcessDot(ref objects, ref maxDepth, maxObjects, tokens);
                        break;
                    case JPathTokenType.LeftBracket:
                        ProcessBracket(ref objects, ref maxDepth, maxObjects, tokens);
                        break;
                    default:
                        throw new FormatException();
                }
            }
        }

        private static void ProcessDot(ref JToken?[] objects, ref int maxDepth, int maxObjects, Queue<JPathToken> tokens)
        {
            JPathToken token = DequeueToken(tokens);
            switch (token.Type)
            {
                case JPathTokenType.Asterisk:
                    Descent(ref objects, ref maxDepth, maxObjects);
                    break;
                case JPathTokenType.Dot:
                    ProcessRecursiveDescent(ref objects, ref maxDepth, maxObjects, tokens);
                    break;
                case JPathTokenType.Identifier:
                    Descent(ref objects, ref maxDepth, maxObjects, token.Content!);
                    break;
                default:
                    throw new FormatException();
            }
        }

        private static void ProcessBracket(ref JToken?[] objects, ref int maxDepth, int maxObjects, Queue<JPathToken> tokens)
        {
            JPathToken token = DequeueToken(tokens);
            switch (token.Type)
            {
                case JPathTokenType.Asterisk:
                    if (DequeueToken(tokens).Type != JPathTokenType.RightBracket)
                        throw new FormatException();
                    Descent(ref objects, ref maxDepth, maxObjects);
                    break;
                case JPathTokenType.Colon:
                    ProcessSlice(ref objects, ref maxDepth, maxObjects, tokens, 0);
                    break;
                case JPathTokenType.Number:
                    JPathToken next = DequeueToken(tokens);
                    switch (next.Type)
                    {
                        case JPathTokenType.Colon:
                            ProcessSlice(ref objects, ref maxDepth, maxObjects, tokens, int.Parse(token.Content!));
                            break;
                        case JPathTokenType.Comma:
                            ProcessUnion(ref objects, ref maxDepth, maxObjects, tokens, token);
                            break;
                        case JPathTokenType.RightBracket:
                            Descent(ref objects, ref maxDepth, maxObjects, int.Parse(token.Content!));
                            break;
                        default:
                            throw new FormatException();
                    }
                    break;
                case JPathTokenType.String:
                    next = DequeueToken(tokens);
                    switch (next.Type)
                    {
                        case JPathTokenType.Comma:
                            ProcessUnion(ref objects, ref maxDepth, maxObjects, tokens, token);
                            break;
                        case JPathTokenType.RightBracket:
                            Descent(ref objects, ref maxDepth, maxObjects, JToken.Parse($"\"{token.Content!.Trim('\'')}\"")!.GetString());
                            break;
                        default:
                            throw new FormatException();
                    }
                    break;
                default:
                    throw new FormatException();
            }
        }

        private static void ProcessRecursiveDescent(ref JToken?[] objects, ref int maxDepth, int maxObjects, Queue<JPathToken> tokens)
        {
            List<JToken?> results = new();
            JPathToken token = DequeueToken(tokens);
            if (token.Type != JPathTokenType.Identifier) throw new FormatException();
            while (objects.Length > 0)
            {
                results.AddRange(objects.OfType<JObject>().SelectMany(p => p.Properties).Where(p => p.Key == token.Content).Select(p => p.Value));
                Descent(ref objects, ref maxDepth, maxObjects);
                if (results.Count > maxObjects) throw new InvalidOperationException(nameof(maxObjects));
            }
            objects = results.ToArray();
        }

        private static void ProcessSlice(ref JToken?[] objects, ref int maxDepth, int maxObjects, Queue<JPathToken> tokens, int start)
        {
            JPathToken token = DequeueToken(tokens);
            switch (token.Type)
            {
                case JPathTokenType.Number:
                    if (DequeueToken(tokens).Type != JPathTokenType.RightBracket)
                        throw new FormatException();
                    DescentRange(ref objects, ref maxDepth, maxObjects, start, int.Parse(token.Content!));
                    break;
                case JPathTokenType.RightBracket:
                    DescentRange(ref objects, ref maxDepth, maxObjects, start, 0);
                    break;
                default:
                    throw new FormatException();
            }
        }

        private static void ProcessUnion(ref JToken?[] objects, ref int maxDepth, int maxObjects, Queue<JPathToken> tokens, JPathToken first)
        {
            List<JPathToken> items = new() { first };
            while (true)
            {
                JPathToken token = DequeueToken(tokens);
                if (token.Type != first.Type) throw new FormatException();
                items.Add(token);
                token = DequeueToken(tokens);
                if (token.Type == JPathTokenType.RightBracket)
                    break;
                if (token.Type != JPathTokenType.Comma)
                    throw new FormatException();
            }
            switch (first.Type)
            {
                case JPathTokenType.Number:
                    Descent(ref objects, ref maxDepth, maxObjects, items.Select(p => int.Parse(p.Content!)).ToArray());
                    break;
                case JPathTokenType.String:
                    Descent(ref objects, ref maxDepth, maxObjects, items.Select(p => JToken.Parse($"\"{p.Content!.Trim('\'')}\"")!.GetString()).ToArray());
                    break;
                default:
                    throw new FormatException();
            }
        }

        private static void Descent(ref JToken?[] objects, ref int maxDepth, int maxObjects)
        {
            if (maxDepth <= 0) throw new InvalidOperationException();
            --maxDepth;
            objects = objects.OfType<JContainer>().SelectMany(p => p.Children).ToArray();
            if (objects.Length > maxObjects) throw new InvalidOperationException(nameof(maxObjects));
        }

        private static void Descent(ref JToken?[] objects, ref int maxDepth, int maxObjects, params string[] names)
        {
            static IEnumerable<JToken?> GetProperties(JObject obj, string[] names)
            {
                foreach (string name in names)
                    if (obj.ContainsProperty(name))
                        yield return obj[name];
            }
            if (maxDepth <= 0) throw new InvalidOperationException();
            --maxDepth;
            objects = objects.OfType<JObject>().SelectMany(p => GetProperties(p, names)).ToArray();
            if (objects.Length > maxObjects) throw new InvalidOperationException(nameof(maxObjects));
        }

        private static void Descent(ref JToken?[] objects, ref int maxDepth, int maxObjects, params int[] indexes)
        {
            static IEnumerable<JToken?> GetElements(JArray array, int[] indexes)
            {
                foreach (int index in indexes)
                {
                    int i = index >= 0 ? index : index + array.Count;
                    if (i >= 0 && i < array.Count)
                        yield return array[i];
                }
            }
            if (maxDepth <= 0) throw new InvalidOperationException();
            --maxDepth;
            objects = objects.OfType<JArray>().SelectMany(p => GetElements(p, indexes)).ToArray();
            if (objects.Length > maxObjects) throw new InvalidOperationException(nameof(maxObjects));
        }

        private static void DescentRange(ref JToken?[] objects, ref int maxDepth, int maxObjects, int start, int end)
        {
            if (maxDepth <= 0) throw new InvalidOperationException();
            --maxDepth;
            objects = objects.OfType<JArray>().SelectMany(p =>
            {
                int iStart = start >= 0 ? start : start + p.Count;
                if (iStart < 0) iStart = 0;
                int iEnd = end > 0 ? end : end + p.Count;
                int count = iEnd - iStart;
                return p.Skip(iStart).Take(count);
            }).ToArray();
            if (objects.Length > maxObjects) throw new InvalidOperationException(nameof(maxObjects));
        }
    }
}
