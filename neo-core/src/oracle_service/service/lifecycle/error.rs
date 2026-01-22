use super::super::{OracleService, OracleServiceError};
use tracing::error;

impl OracleService {
    pub(crate) fn handle_error(&self, err: &OracleServiceError) {
        error!(target: "neo::oracle", error = %err, "oracle service error");
        match self.settings.exception_policy {
            crate::UnhandledExceptionPolicy::StopPlugin => self.stop(),
            crate::UnhandledExceptionPolicy::StopNode => std::process::exit(1),
            crate::UnhandledExceptionPolicy::Terminate => std::process::abort(),
            crate::UnhandledExceptionPolicy::Ignore | crate::UnhandledExceptionPolicy::Continue => {
            }
        }
    }
}
