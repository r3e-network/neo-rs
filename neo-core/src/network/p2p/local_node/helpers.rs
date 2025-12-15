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

pub(super) fn parse_seed_entry(entry: &str) -> Option<(String, u16)> {
    let trimmed = entry.trim();
    if trimmed.is_empty() {
        return None;
    }

    let (host, port_str) = trimmed.rsplit_once(':')?;

    if host.is_empty() {
        return None;
    }

    let port: u16 = port_str.parse().ok()?;
    Some((host.to_string(), port))
}
