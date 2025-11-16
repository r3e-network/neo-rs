use super::{
    argument_parser::{ArgumentParser, ArgumentValue, ParameterDescriptor},
    command_tokenizer::tokenize,
    console_command_attribute::ConsoleCommandAttribute,
    console_command_method::ConsoleCommandMethod,
};
use anyhow::Result;
use std::sync::Arc;

pub type CommandHandler = Arc<dyn Fn(Vec<ArgumentValue>) -> Result<()> + Send + Sync + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseMode {
    Sequential,
    Indicator,
    Auto,
}

struct CommandDefinition {
    method: ConsoleCommandMethod,
    parser: ArgumentParser,
    mode: ParseMode,
    handler: CommandHandler,
}

#[derive(Default)]
pub struct CommandDispatcher {
    commands: Vec<CommandDefinition>,
}

impl CommandDispatcher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_command(
        &mut self,
        attribute: ConsoleCommandAttribute,
        parameters: Vec<ParameterDescriptor>,
        mode: ParseMode,
        handler: CommandHandler,
    ) {
        let method = ConsoleCommandMethod::from_attribute(attribute);
        let parser = ArgumentParser::new(parameters);
        self.commands.push(CommandDefinition {
            method,
            parser,
            mode,
            handler,
        });
    }

    /// Attempts to execute a command line. Returns `Ok(true)` when a handler
    /// ran successfully, `Ok(false)` when no command matched, or an error when
    /// parsing/handler execution failed.
    pub fn execute(&self, command_line: &str) -> Result<bool> {
        if command_line.trim().is_empty() {
            return Ok(false);
        }

        let tokens = tokenize(command_line)?;
        for command in &self.commands {
            let consumed = command.method.is_this_command(&tokens);
            if consumed == 0 {
                continue;
            }
            let args_tokens = tokens[consumed..].to_vec();
            let args = match command.mode {
                ParseMode::Sequential => command.parser.parse_sequential(&args_tokens)?,
                ParseMode::Indicator => command.parser.parse_indicator(&args_tokens)?,
                ParseMode::Auto => {
                    if args_tokens.iter().any(|token| token.is_indicator()) {
                        command.parser.parse_indicator(&args_tokens)?
                    } else {
                        command.parser.parse_sequential(&args_tokens)?
                    }
                }
            };
            (command.handler)(args)?;
            return Ok(true);
        }
        Ok(false)
    }

    pub fn list_commands(&self) -> Vec<(String, String)> {
        self.commands
            .iter()
            .map(|command| {
                (
                    command.method.key(),
                    command.method.help_message().to_string(),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console_service::argument_parser::{ArgumentValue, ParameterKind};

    fn handler_collector(output: Arc<std::sync::Mutex<Vec<Vec<ArgumentValue>>>>) -> CommandHandler {
        Arc::new(move |args| {
            output.lock().unwrap().push(args);
            Ok(())
        })
    }

    #[test]
    fn sequential_command_executes_handler() {
        let mut dispatcher = CommandDispatcher::new();
        let collected = Arc::new(std::sync::Mutex::new(Vec::new()));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("open wallet"),
            vec![
                ParameterDescriptor::new("path", ParameterKind::String),
                ParameterDescriptor::new("password", ParameterKind::String),
            ],
            ParseMode::Sequential,
            handler_collector(collected.clone()),
        );

        let executed = dispatcher.execute("open wallet foo.json secret").unwrap();
        assert!(executed);
        let records = collected.lock().unwrap();
        assert_eq!(
            records[0],
            vec![
                ArgumentValue::String("foo.json".into()),
                ArgumentValue::String("secret".into())
            ]
        );
    }

    #[test]
    fn indicator_command_handles_flags() {
        let mut dispatcher = CommandDispatcher::new();
        let collected = Arc::new(std::sync::Mutex::new(Vec::new()));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("open wallet"),
            vec![
                ParameterDescriptor::new("path", ParameterKind::String),
                ParameterDescriptor::new("password", ParameterKind::String),
                ParameterDescriptor::new("verbose", ParameterKind::Bool)
                    .with_default(ArgumentValue::Bool(false)),
            ],
            ParseMode::Indicator,
            handler_collector(collected.clone()),
        );

        let executed = dispatcher
            .execute("open wallet --path foo.json --password secret --verbose")
            .unwrap();
        assert!(executed);
        let records = collected.lock().unwrap();
        assert_eq!(
            records[0],
            vec![
                ArgumentValue::String("foo.json".into()),
                ArgumentValue::String("secret".into()),
                ArgumentValue::Bool(true)
            ]
        );
    }

    #[test]
    fn auto_mode_switches_based_on_tokens() {
        let mut dispatcher = CommandDispatcher::new();
        let collected = Arc::new(std::sync::Mutex::new(Vec::new()));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("open wallet"),
            vec![
                ParameterDescriptor::new("path", ParameterKind::String),
                ParameterDescriptor::new("password", ParameterKind::String),
            ],
            ParseMode::Auto,
            handler_collector(collected.clone()),
        );

        dispatcher.execute("open wallet foo.json secret").unwrap();
        dispatcher
            .execute("open wallet --path foo.json --password secret")
            .unwrap();

        let records = collected.lock().unwrap();
        assert_eq!(records.len(), 2);
    }
}
