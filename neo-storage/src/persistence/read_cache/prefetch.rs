/// Pre-fetch hint for sequential access patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefetchHint {
    /// No pre-fetching.
    None,
    /// Pre-fetch forward (next keys).
    Forward,
    /// Pre-fetch backward (previous keys).
    Backward,
    /// Pre-fetch both directions.
    Both,
}
