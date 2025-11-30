//! IEventHandlers module for Neo blockchain
//!
//! This module provides event handler interfaces matching the C# Neo.IEventHandlers namespace.

pub mod i_committed_handler;
pub mod i_committing_handler;
pub mod i_log_handler;
pub mod i_logging_handler;
pub mod i_message_received_handler;
pub mod i_notify_handler;
pub mod i_service_added_handler;
pub mod i_transaction_added_handler;
pub mod i_transaction_removed_handler;
pub mod i_wallet_changed_handler;

// Re-export commonly used types
pub use i_committed_handler::ICommittedHandler;
pub use i_committing_handler::ICommittingHandler;
pub use i_log_handler::ILogHandler;
pub use i_logging_handler::ILoggingHandler;
pub use i_message_received_handler::IMessageReceivedHandler;
pub use i_notify_handler::INotifyHandler;
pub use i_service_added_handler::IServiceAddedHandler;
pub use i_transaction_added_handler::ITransactionAddedHandler;
pub use i_transaction_removed_handler::ITransactionRemovedHandler;
pub use i_wallet_changed_handler::IWalletChangedHandler;
