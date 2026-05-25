use super::super::{OracleService, OracleServiceError};
use tracing::error;

impl OracleService {
    pub(crate) fn handle_error(&self, err: &OracleServiceError) {
        error!(target: "neo::oracle", error = %err, "oracle service error");
        self.settings.exception_policy.apply(|| self.stop());
    }
}
