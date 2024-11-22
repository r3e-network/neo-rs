use chrono::{DateTime, TimeZone, Utc};

pub trait DateTimeExtensions {
    fn to_timestamp(&self) -> u32;
    fn to_timestamp_ms(&self) -> u64;
}

impl<Tz: TimeZone> DateTimeExtensions for DateTime<Tz> {
    /// Converts a DateTime to timestamp.
    ///
    /// # Arguments
    ///
    /// * `self` - The DateTime to convert.
    ///
    /// # Returns
    ///
    /// The converted timestamp as a u32.
    fn to_timestamp(&self) -> u32 {
        self.with_timezone(&Utc).timestamp() as u32
    }

    /// Converts a DateTime to timestamp in milliseconds.
    ///
    /// # Arguments
    ///
    /// * `self` - The DateTime to convert.
    ///
    /// # Returns
    ///
    /// The converted timestamp in milliseconds as a u64.
    fn to_timestamp_ms(&self) -> u64 {
        self.with_timezone(&Utc).timestamp_millis() as u64
    }
}
