//! Neo.ConsoleService port (partial).
//!
//! This module gradually mirrors the C# console infrastructure so the CLI can
//! reuse the same prompting/logging patterns while the rest of the command
//! system is being ported.

pub mod argument_parser;
pub mod color_set;
pub mod command_dispatcher;
pub mod command_token;
pub mod command_tokenizer;
pub mod console_command_attribute;
pub mod console_command_method;
pub mod console_helper;

#[allow(unused_imports)]
pub use argument_parser::{ArgumentParser, ArgumentValue, ParameterDescriptor, ParameterKind};
#[allow(unused_imports)]
pub use color_set::ConsoleColorSet;
#[allow(unused_imports)]
pub use command_dispatcher::{CommandDispatcher, CommandHandler, ParseMode};
#[allow(unused_imports)]
pub use command_token::CommandToken;
#[allow(unused_imports)]
pub use command_tokenizer::{consume, consume_all, join_raw, tokenize, trim};
#[allow(unused_imports)]
pub use console_command_attribute::ConsoleCommandAttribute;
#[allow(unused_imports)]
pub use console_command_method::ConsoleCommandMethod;
#[allow(unused_imports)]
pub use console_helper::ConsoleHelper;
