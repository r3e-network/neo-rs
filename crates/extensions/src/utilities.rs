use chrono::{DateTime, TimeZone, Utc};

/// Get the current Unix timestamp in seconds.
pub fn current_timestamp() -> u64 {
    Utc::now().timestamp() as u64
}

/// Convert a Unix timestamp to a UTC DateTime.
pub fn timestamp_to_datetime(timestamp: u64) -> DateTime<Utc> {
    Utc.timestamp_opt(timestamp as i64, 0)
        .single()
        .unwrap_or_else(Utc::now)
}

/// Convert a UTC DateTime back to a Unix timestamp.
pub fn datetime_to_timestamp(datetime: &DateTime<Utc>) -> u64 {
    datetime.timestamp() as u64
}

/// Render a byte count into a human-readable string.
pub fn bytes_to_human_readable(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    if bytes < 1024 {
        format!("{} B", bytes)
    } else if (bytes as f64) < MB {
        format!("{:.2} KB", bytes as f64 / KB)
    } else if (bytes as f64) < GB {
        format!("{:.2} MB", bytes as f64 / MB)
    } else {
        format!("{:.2} GB", bytes as f64 / GB)
    }
}

/// Clamp a value between `min` and `max`.
pub fn clamp<T>(value: T, min: T, max: T) -> T
where
    T: PartialOrd + Copy,
{
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

/// Safe division that returns 0.0 on divide-by-zero.
pub fn safe_divide(numerator: f64, denominator: f64) -> f64 {
    if denominator == 0.0 {
        0.0
    } else {
        numerator / denominator
    }
}
