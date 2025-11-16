use super::command_token::CommandToken;
use anyhow::{anyhow, Result};

/// Tokenizes a CLI command string following the C# `CommandTokenizer` semantics.
pub fn tokenize(command_line: &str) -> Result<Vec<CommandToken>> {
    let chars: Vec<char> = command_line.chars().collect();
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut quote_char = CommandToken::NO_QUOTE_CHAR;

    let mut index = 0usize;
    while index < chars.len() {
        let ch = chars[index];
        if ch == '\\' && quote_char != CommandToken::NO_ESCAPED_CHAR {
            let (escaped, consumed) = escaped_char(&chars, index)?;
            token.push(escaped);
            index += consumed;
        } else if quote_char != CommandToken::NO_QUOTE_CHAR {
            if ch == quote_char {
                add_token(&mut tokens, &mut token, index, quote_char);
                quote_char = CommandToken::NO_QUOTE_CHAR;
            } else {
                token.push(ch);
            }
        } else if is_quote(ch) {
            if token.is_empty() {
                quote_char = ch;
            } else {
                token.push(ch);
            }
        } else if ch.is_whitespace() {
            if !token.is_empty() {
                add_token(&mut tokens, &mut token, index, quote_char);
            }

            token.push(ch);
            while index + 1 < chars.len() && chars[index + 1].is_whitespace() {
                index += 1;
                token.push(chars[index]);
            }
            add_token(&mut tokens, &mut token, index, quote_char);
        } else {
            token.push(ch);
        }

        index += 1;
    }

    if quote_char != CommandToken::NO_QUOTE_CHAR {
        return Err(anyhow!("Unmatched quote({})", quote_char));
    }

    if !token.is_empty() {
        add_token(&mut tokens, &mut token, chars.len(), quote_char);
    }

    Ok(tokens)
}

fn escaped_char(chars: &[char], index: usize) -> Result<(char, usize)> {
    let next = index + 1;
    if next >= chars.len() {
        return Err(anyhow!(
            "Invalid escape sequence. The command line ends with a backslash character."
        ));
    }

    match chars[next] {
        'x' => {
            if next + 2 >= chars.len() {
                return Err(anyhow!("Invalid escape sequence. Too few hex digits after \\x"));
            }
            let hex = chars[next + 1].to_string() + &chars[next + 2].to_string();
            let value = u8::from_str_radix(&hex, 16).map_err(|_| {
                anyhow!(
                    "Invalid hex digits after \\x. If you don't want to use escape character, please use backtick(`) to wrap the string."
                )
            })?;
            Ok((value as char, 1 + 2))
        }
        'u' => {
            if next + 4 >= chars.len() {
                return Err(anyhow!("Invalid escape sequence. Too few hex digits after \\u"));
            }
            let mut hex = String::new();
            for offset in 1..=4 {
                hex.push(chars[next + offset]);
            }
            let value = u16::from_str_radix(&hex, 16).map_err(|_| {
                anyhow!(
                    "Invalid hex digits after \\u. If you don't want to use escape character, please use backtick(`) to wrap the string."
                )
            })?;
            Ok((char::from_u32(value as u32).unwrap_or('\0'), 1 + 4))
        }
        other => Ok((match other {
            '\\' => '\\',
            '"' => '"',
            '\'' => '\'',
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            'v' => '\u{000B}',
            'b' => '\u{0008}',
            'f' => '\u{000C}',
            'a' => '\u{0007}',
            'e' => '\u{001B}',
            '0' => '\0',
            ' ' => ' ',
            _ => {
                return Err(anyhow!(
                    "Invalid escaped character: \\{}. If you don't want to use escape character, please use backtick(`) to wrap the string.",
                    other
                ))
            }
        }, 1)),
    }
}

fn add_token(
    tokens: &mut Vec<CommandToken>,
    buffer: &mut String,
    end_index: usize,
    quote_char: char,
) {
    if buffer.is_empty() {
        return;
    }
    let value = buffer.clone();
    let offset = end_index.saturating_sub(value.chars().count());
    tokens.push(CommandToken::new(offset, value, quote_char));
    buffer.clear();
}

fn is_quote(ch: char) -> bool {
    matches!(ch, '"' | '\'' | CommandToken::NO_ESCAPED_CHAR)
}

pub fn trim(tokens: &mut Vec<CommandToken>) {
    while tokens.first().map_or(false, CommandToken::is_white_space) {
        tokens.remove(0);
    }
    while tokens.last().map_or(false, CommandToken::is_white_space) {
        tokens.pop();
    }
}

pub fn join_raw(tokens: &mut Vec<CommandToken>) -> String {
    trim(tokens);
    tokens.iter().map(CommandToken::raw_value).collect()
}

pub fn consume(tokens: &mut Vec<CommandToken>) -> String {
    trim(tokens);
    if tokens.is_empty() {
        return String::new();
    }
    tokens.remove(0).value().to_string()
}

pub fn consume_all(tokens: &mut Vec<CommandToken>) -> String {
    let result = join_raw(tokens);
    tokens.clear();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_handles_simple_verbs() {
        let tokens = tokenize("show state").unwrap();
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0].value(), "show");
        assert!(tokens[1].is_white_space());
        assert_eq!(tokens[2].value(), "state");
    }

    #[test]
    fn tokenize_supports_quotes_and_spaces() {
        let tokens = tokenize("invoke \"My Contract\"").unwrap();
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[2].value(), "My Contract");
        assert_eq!(tokens[2].quote_char(), '"');
    }

    #[test]
    fn consume_helpers_trim_whitespace() {
        let mut tokens = tokenize("  open   wallet  ").unwrap();
        assert_eq!(consume(&mut tokens), "open");
        assert_eq!(consume(&mut tokens), "wallet");
        assert!(consume(&mut tokens).is_empty());
    }

    #[test]
    fn join_raw_preserves_quotes() {
        let mut tokens = tokenize("`two words` \"quoted\"").unwrap();
        assert_eq!(
            join_raw(&mut tokens),
            "`two words` \"quoted\"",
            "JoinRaw concatenates raw values without trimming internal whitespace"
        );
    }

    #[test]
    fn unmatched_quote_errors() {
        let err = tokenize("\"unterminated").unwrap_err();
        assert!(err.to_string().contains("Unmatched quote"));
    }
}
