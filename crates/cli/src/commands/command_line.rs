use super::{not_implemented, CommandResult};
use crate::console_service::{
    ArgumentValue, CommandDispatcher, CommandHandler, ConsoleCommandAttribute, ConsoleHelper,
    ParameterDescriptor, ParameterKind, ParseMode,
};
use anyhow::{anyhow, Result};
use std::sync::Arc;

use super::wallet::WalletCommands;

/// Command routing infrastructure (`MainService.CommandLine`).
pub struct CommandLine {
    dispatcher: CommandDispatcher,
}

impl CommandLine {
    pub fn new(wallet_commands: Arc<WalletCommands>) -> Self {
        let mut dispatcher = CommandDispatcher::new();
        Self::register_wallet_commands(&mut dispatcher, wallet_commands);
        Self::register_help_command(&mut dispatcher);
        Self { dispatcher }
    }

    pub fn execute(&self, command_line: &str) -> CommandResult {
        if self.dispatcher.execute(command_line)? {
            Ok(())
        } else {
            Err(anyhow!("unknown command: {}", command_line.trim()))
        }
    }

    /// Placeholder until all commands are ported.
    pub fn register_commands(&self) -> CommandResult {
        not_implemented("register commands")
    }

    /// Runs an interactive shell that reads commands from stdin.
    pub fn run_shell(&self) -> CommandResult {
        loop {
            let line = match ConsoleHelper::read_user_input("neo", false) {
                Ok(line) => line,
                Err(err) => return Err(err),
            };

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if matches!(trimmed.to_ascii_lowercase().as_str(), "exit" | "quit") {
                break;
            }

            if let Err(err) = self.execute(trimmed) {
                ConsoleHelper::error(err.to_string());
            }
        }
        Ok(())
    }

    fn register_wallet_commands(dispatcher: &mut CommandDispatcher, wallet: Arc<WalletCommands>) {
        let open_handler = wallet_open_handler(Arc::clone(&wallet));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("open wallet"),
            vec![
                ParameterDescriptor::new("path", ParameterKind::String),
                ParameterDescriptor::new("password", ParameterKind::String),
            ],
            ParseMode::Auto,
            open_handler,
        );

        let close_handler = wallet_close_handler(Arc::clone(&wallet));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("close wallet"),
            Vec::new(),
            ParseMode::Sequential,
            close_handler,
        );

        let create_handler = wallet_create_handler(Arc::clone(&wallet));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("create wallet"),
            vec![
                ParameterDescriptor::new("path", ParameterKind::String),
                ParameterDescriptor::new("password", ParameterKind::String),
            ],
            ParseMode::Auto,
            create_handler,
        );

        let create_address_handler = wallet_create_address_handler(Arc::clone(&wallet));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("create address"),
            vec![ParameterDescriptor::new("count", ParameterKind::Int)
                .with_default(ArgumentValue::Int(1))],
            ParseMode::Auto,
            create_address_handler,
        );

        let list_handler = wallet_list_handler(Arc::clone(&wallet));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("list address"),
            Vec::new(),
            ParseMode::Sequential,
            list_handler,
        );

        let asset_handler = wallet_asset_handler(Arc::clone(&wallet));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("list asset"),
            Vec::new(),
            ParseMode::Sequential,
            asset_handler,
        );

        let key_handler = wallet_key_handler(Arc::clone(&wallet));
        dispatcher.register_command(
            ConsoleCommandAttribute::new("list key"),
            Vec::new(),
            ParseMode::Sequential,
            key_handler,
        );

        let delete_handler = wallet_delete_handler(wallet);
        dispatcher.register_command(
            ConsoleCommandAttribute::new("delete address"),
            vec![ParameterDescriptor::new("address", ParameterKind::String)],
            ParseMode::Auto,
            delete_handler,
        );
    }

    fn register_help_command(dispatcher: &mut CommandDispatcher) {
        let handler = help_handler(dispatcher.list_commands());
        dispatcher.register_command(
            ConsoleCommandAttribute::new("help"),
            Vec::new(),
            ParseMode::Sequential,
            handler,
        );
    }
}

fn wallet_open_handler(wallet: Arc<WalletCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.len() < 2 {
            return Err(anyhow!("open wallet requires <path> <password>"));
        }
        let path = expect_string(&args[0], "path")?;
        let password = expect_string(&args[1], "password")?;
        wallet.open_wallet(path, &password)?;
        Ok(())
    })
}

fn wallet_create_handler(wallet: Arc<WalletCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.len() < 2 {
            return Err(anyhow!("create wallet requires <path> <password>"));
        }
        let path = expect_string(&args[0], "path")?;
        let password = expect_string(&args[1], "password")?;
        wallet.create_wallet(path, &password)?;
        Ok(())
    })
}

fn wallet_create_address_handler(wallet: Arc<WalletCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        let count = if args.is_empty() {
            1
        } else {
            let value = expect_int(&args[0], "count")?;
            if value <= 0 {
                return Err(anyhow!("count must be greater than zero"));
            }
            u16::try_from(value).map_err(|_| anyhow!("count is too large"))?
        };
        wallet.create_addresses(count)?;
        Ok(())
    })
}

fn wallet_list_handler(wallet: Arc<WalletCommands>) -> CommandHandler {
    Arc::new(move |_args: Vec<ArgumentValue>| {
        wallet.list_addresses()?;
        Ok(())
    })
}

fn wallet_asset_handler(wallet: Arc<WalletCommands>) -> CommandHandler {
    Arc::new(move |_args: Vec<ArgumentValue>| {
        wallet.list_assets()?;
        Ok(())
    })
}

fn wallet_key_handler(wallet: Arc<WalletCommands>) -> CommandHandler {
    Arc::new(move |_args: Vec<ArgumentValue>| {
        wallet.list_keys()?;
        Ok(())
    })
}

fn wallet_delete_handler(wallet: Arc<WalletCommands>) -> CommandHandler {
    Arc::new(move |args: Vec<ArgumentValue>| {
        if args.is_empty() {
            return Err(anyhow!("delete address requires <address>"));
        }
        let address = expect_string(&args[0], "address")?;
        wallet.delete_address(&address)?;
        Ok(())
    })
}

fn wallet_close_handler(wallet: Arc<WalletCommands>) -> CommandHandler {
    Arc::new(move |_args: Vec<ArgumentValue>| {
        wallet.close_wallet()?;
        Ok(())
    })
}

fn expect_string(value: &ArgumentValue, name: &str) -> Result<String> {
    match value {
        ArgumentValue::String(text) => Ok(text.clone()),
        other => Err(anyhow!("{name} expects a string argument, got {:?}", other)),
    }
}

fn expect_int(value: &ArgumentValue, name: &str) -> Result<i64> {
    match value {
        ArgumentValue::Int(num) => Ok(*num),
        other => Err(anyhow!(
            "{name} expects an integer argument, got {:?}",
            other
        )),
    }
}

fn help_handler(commands: Vec<(String, String)>) -> CommandHandler {
    Arc::new(move |_args: Vec<ArgumentValue>| {
        ConsoleHelper::info(["Available commands:"]);
        for (command, description) in &commands {
            if description.is_empty() {
                ConsoleHelper::info([" - ", command]);
            } else {
                ConsoleHelper::info([" - ", command, ": ", description]);
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_core::protocol_settings::ProtocolSettings;
    use std::sync::Arc;

    fn command_line() -> CommandLine {
        let wallet = Arc::new(WalletCommands::new(Arc::new(ProtocolSettings::default())));
        CommandLine::new(wallet)
    }

    #[test]
    fn help_command_succeeds() {
        let cli = command_line();
        assert!(cli.execute("help").is_ok());
    }

    #[test]
    fn unknown_command_errors() {
        let cli = command_line();
        assert!(cli.execute("unknown command").is_err());
    }
}
