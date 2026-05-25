//
// helpers.rs - Helper functions for local node
//

use super::*;

pub(super) fn current_unix_timestamp() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs(),
        Err(error) => {
            // System time is before UNIX_EPOCH; fall back to zero to preserve monotonicity.
            let duration: Duration = error.duration();
            duration.as_secs()
        }
    }
}
