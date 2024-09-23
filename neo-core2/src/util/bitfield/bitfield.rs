//! This module provides a simple and efficient arbitrary size bit field implementation.
//! It doesn't attempt to cover everything that could be done with bit fields,
//! providing only things used by neo-go.

use std::cmp::min;

/// Field is a bit field represented as a vector of u64 values.
#[derive(Clone, PartialEq, Eq)]
pub struct Field(Vec<u64>);

/// Bits count in a basic element of Field.
const ELEM_BITS: usize = 64;

impl Field {
    /// Creates a new bit field of the specified length. Actual field length
    /// can be rounded to the next multiple of 64, so it's a responsibility
    /// of the user to deal with that.
    pub fn new(n: usize) -> Self {
        Field(vec![0; 1 + (n - 1) / ELEM_BITS])
    }

    /// Sets one bit at the specified offset. No bounds checking is done.
    pub fn set(&mut self, i: usize) {
        let (addr, offset) = (i / ELEM_BITS, i % ELEM_BITS);
        self.0[addr] |= 1 << offset;
    }

    /// Returns true if the bit with the specified offset is set.
    pub fn is_set(&self, i: usize) -> bool {
        let (addr, offset) = (i / ELEM_BITS, i % ELEM_BITS);
        (self.0[addr] & (1 << offset)) != 0
    }

    /// Makes a copy of the current Field.
    pub fn copy(&self) -> Self {
        self.clone()
    }

    /// Implements logical AND between self's and m's bits saving the result into self.
    pub fn and(&mut self, m: &Field) {
        let l = m.0.len();
        for (i, val) in self.0.iter_mut().enumerate() {
            if i >= l {
                *val = 0;
            } else {
                *val &= m.0[i];
            }
        }
    }

    /// Compares two Fields and returns true if they're equal.
    pub fn equals(&self, o: &Field) -> bool {
        self == o
    }

    /// Returns true when self is a subset of o (only has bits set that are
    /// set in o).
    pub fn is_subset(&self, o: &Field) -> bool {
        if self.0.len() > o.0.len() {
            return false;
        }
        for i in 0..min(self.0.len(), o.0.len()) {
            let r = self.0[i] & o.0[i];
            if r != self.0[i] {
                return false;
            }
        }
        true
    }
}
