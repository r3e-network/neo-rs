mod file;
mod flags;
mod token;
mod util;

pub use file::NefFile;
pub use flags::CallFlags;
pub use token::MethodToken;

pub(crate) const NEF_MAGIC: u32 = 0x3346_454E; // "NEF3"
pub(crate) const COMPILER_FIELD_SIZE: usize = 64;
pub(crate) const METHOD_NAME_MAX: usize = 32;
pub(crate) const SOURCE_URL_MAX: usize = 256;
pub(crate) const TOKENS_MAX: usize = 128;
pub(crate) const MAX_SCRIPT_SIZE: u64 = 1_048_576;
