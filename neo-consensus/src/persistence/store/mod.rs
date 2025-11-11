mod api;
mod error;

#[cfg(test)]
mod tests;

pub use api::{clear_snapshot, load_engine, persist_engine};
pub use error::PersistenceError;
