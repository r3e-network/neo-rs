use super::{command_token::CommandToken, console_command_attribute::ConsoleCommandAttribute};

/// Rust port of `Neo.ConsoleService.ConsoleCommandMethod`.
#[derive(Debug, Clone)]
pub struct ConsoleCommandMethod {
    verbs: Vec<String>,
    help_category: String,
    help_message: String,
}

impl ConsoleCommandMethod {
    pub fn from_attribute(attribute: ConsoleCommandAttribute) -> Self {
        Self {
            verbs: attribute.verbs().to_vec(),
            help_category: attribute.category().to_string(),
            help_message: attribute.description().to_string(),
        }
    }

    pub fn key(&self) -> String {
        self.verbs.join(" ")
    }

    pub fn help_category(&self) -> &str {
        &self.help_category
    }

    pub fn help_message(&self) -> &str {
        &self.help_message
    }

    /// Returns the number of consumed tokens when this command matches the provided stream.
    /// Returns zero when no match occurs.
    pub fn is_this_command(&self, tokens: &[CommandToken]) -> usize {
        let mut matched = 0usize;
        let mut consumed = 0usize;
        while matched < self.verbs.len() && consumed < tokens.len() {
            let token = &tokens[consumed];
            if token.is_white_space() {
                consumed += 1;
                continue;
            }
            if token.value() != self.verbs[matched] {
                return 0;
            }
            matched += 1;
            consumed += 1;
        }

        if matched == self.verbs.len() {
            consumed
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console_service::command_tokenizer::tokenize;

    #[test]
    fn matching_skips_whitespace_tokens() {
        let attr = ConsoleCommandAttribute::new("show state");
        let method = ConsoleCommandMethod::from_attribute(attr);
        let tokens = tokenize("show   state args").unwrap();
        assert_eq!(method.is_this_command(&tokens), 3);
    }

    #[test]
    fn non_matching_returns_zero() {
        let attr = ConsoleCommandAttribute::new("open wallet");
        let method = ConsoleCommandMethod::from_attribute(attr);
        let tokens = tokenize("close wallet").unwrap();
        assert_eq!(method.is_this_command(&tokens), 0);
    }
}
