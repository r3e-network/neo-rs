#![allow(dead_code)]
//! Read-Write Set Tracker for Optimistic Parallel Transaction Execution
//! Part of Wave 1 Tier 3 optimization (Frank's implementation)
//! Tracks which keys each transaction reads/writes to detect conflicts

use neo_primitives::UInt256;
// Simplified version - removed unused event tracking that requires non-existent types

/// Tracks read/write sets during VM execution for optimistic parallel execution
#[derive(Debug, Clone)]
pub struct RwSetTracker {
    /// All keys read during transaction execution
    pub read_keys: Vec<UInt256>,
    /// All keys written during transaction execution  
    pub write_keys: Vec<UInt256>,
    /// Keys of contract `Notify` events emitted during execution (read by ecosystem/Oracle).
    pub notify_events: Vec<UInt256>,
    /// Keys of contract `Log` events emitted during execution.
    pub log_events: Vec<UInt256>,
    /// Gas consumed during execution
    pub gas_used: i64,
}

impl RwSetTracker {
    /// Create new empty tracker
    pub fn new() -> Self {
        Self {
            read_keys: Vec::new(),
            write_keys: Vec::new(),
            notify_events: Vec::new(),
            log_events: Vec::new(),
            gas_used: 0,
        }
    }

    /// Record a contract notification key (for conflict detection).
    pub fn record_notify_event(&mut self, key: UInt256) {
        self.notify_events.push(key);
    }

    /// Record a contract log key (for conflict detection).
    pub fn record_log_event(&mut self, key: UInt256) {
        self.log_events.push(key);
    }
    
    /// Record a key read (for conflict detection)
    pub fn record_read(&mut self, key: UInt256) {
        self.read_keys.push(key);
    }
    
    /// Record a key write (triggers conflict detection)
    pub fn record_write(&mut self, key: UInt256) {
        self.write_keys.push(key);
    }
    
    /// Check if this tracker conflicts with another
    /// Write-write: both write same key → CONFLICT
    /// Write-read: we write what other reads → CONFLICT  
    /// Read-write: we read what other writes → OK (our read is from stale state)
    pub fn conflicts_with(&self, other: &RwSetTracker) -> bool {
        // Check if our writes intersect with other's reads OR writes
        for write_key in &self.write_keys {
            if other.read_keys.contains(write_key) || other.write_keys.contains(write_key) {
                return true;
            }
        }
        false
    }
    
    /// Set gas used
    pub fn set_gas_used(&mut self, gas: i64) {
        self.gas_used = gas;
    }
    
    /// Clear all tracked data
    pub fn clear(&mut self) {
        self.read_keys.clear();
        self.write_keys.clear();
        self.notify_events.clear();
        self.log_events.clear();
        self.gas_used = 0;
    }

    /// Returns whether this tracker recorded any writes.
    pub fn has_writes(&self) -> bool {
        !self.write_keys.is_empty()
    }

    /// Returns whether this tracker recorded any reads.
    pub fn has_reads(&self) -> bool {
        !self.read_keys.is_empty()
    }

    /// Total number of read/write accesses recorded so far.
    pub fn total_accesses(&self) -> usize {
        self.read_keys.len() + self.write_keys.len()
    }

    /// Reserves read-key capacity to avoid reallocation during execution.
    pub fn reserve_read_capacity(&mut self, additional: usize) {
        self.read_keys.reserve(additional);
    }
}

impl Default for RwSetTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_no_conflict_disjoint_keys() {
        let mut tracker1 = RwSetTracker::new();
        let mut tracker2 = RwSetTracker::new();
        
        tracker1.record_write(UInt256::from_bytes(&[1u8; 32]).unwrap());
        tracker2.record_write(UInt256::from_bytes(&[2u8; 32]).unwrap());
        
        assert!(!tracker1.conflicts_with(&tracker2));
        assert!(!tracker2.conflicts_with(&tracker1));
    }
    
    #[test]
    fn test_write_write_conflict() {
        let mut tracker1 = RwSetTracker::new();
        let mut tracker2 = RwSetTracker::new();
        
        let key = UInt256::from_bytes(&[42u8; 32]).unwrap();
        tracker1.record_write(key);
        tracker2.record_write(key);
        
        assert!(tracker1.conflicts_with(&tracker2));
    }
    
    #[test]
    fn test_write_read_conflict() {
        let mut tracker1 = RwSetTracker::new();
        let mut tracker2 = RwSetTracker::new();
        
        let key = UInt256::from_bytes(&[42u8; 32]).unwrap();
        tracker1.record_write(key);
        tracker2.record_read(key);
        
        assert!(tracker1.conflicts_with(&tracker2));
    }
}
