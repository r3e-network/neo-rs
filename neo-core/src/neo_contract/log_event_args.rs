use crate::network::Payloads::IVerifiable;
use crate::uint160::UInt160;

/// The `EventArgs` of `ApplicationEngine.Log`.
pub struct LogEventArgs {
    /// The container that containing the executed script.
    pub script_container: Box<dyn IVerifiable>,

    /// The script hash of the contract that sends the log.
    pub script_hash: UInt160,

    /// The message of the log.
    pub message: String,
}

impl LogEventArgs {
    /// Initializes a new instance of the `LogEventArgs` struct.
    ///
    /// # Arguments
    ///
    /// * `container` - The container that containing the executed script.
    /// * `script_hash` - The script hash of the contract that sends the log.
    /// * `message` - The message of the log.
    pub fn new(container: Box<dyn IVerifiable>, script_hash: UInt160, message: String) -> Self {
        Self {
            script_container: container,
            script_hash,
            message,
        }
    }
}
