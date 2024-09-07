// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use crate::math::U256;


pub trait ToArray<T: Copy, const N: usize> {
    /// slice to array. slice.len() must be constant
    fn to_array(&self) -> [T; N];
}

impl<T: Copy + Default, const N: usize> ToArray<T, N> for [T] {
    /// slice to array. slice.len() must be constant
    #[inline]
    fn to_array(&self) -> [T; N] {
        let mut d = [Default::default(); N];
        d.copy_from_slice(self);
        d
    }
}

pub trait ToRevArray<T: Copy, const N: usize> {
    fn to_rev_array(&self) -> [T; N];
}

impl<T: Copy + Default, const N: usize> ToRevArray<T, N> for [T] {
    /// slice to revered array(for endian transition). slice.len() must be constant
    #[inline]
    fn to_rev_array(&self) -> [T; N] {
        let mut d = [Default::default(); N];
        d.copy_from_slice(self);
        d.reverse();
        d
    }
}


impl<T: Copy + Default, const N: usize> ToRevArray<T, N> for [T; N] {
    #[inline]
    fn to_rev_array(&self) -> [T; N] {
        let mut b = self.clone();
        b.reverse();
        b
    }
}


pub fn xor_array<const N: usize>(left: &[u8], right: &[u8]) -> [u8; N] {
    let mut d = [0u8; N];
    if left.len() != right.len() {
        core::panic!("left length {} != right length {}", left.len(), right.len());
    }

    if left.len() != d.len() {
        core::panic!("source length {} != dest length {}", left.len(), N);
    }

    left.into_iter()
        .enumerate()
        .for_each(|(idx, v)| d[idx] = v ^ right[idx]);
    d
}

pub trait LeadingZeroBytes {
    fn leading_zero_bytes(&self) -> usize;
}

impl<T: AsRef<[u8]>> LeadingZeroBytes for T {
    /// Self must be big endian
    fn leading_zero_bytes(&self) -> usize {
        let mut count = 0;
        for b in self.as_ref().iter() {
            if *b != 0 {
                return count;
            }
            count += 1;
        }

        count
    }
}


pub trait PickU16 {
    fn pick_le_u16(&self) -> u16;
}

impl<const N: usize> PickU16 for [u8; N] {
    #[inline]
    fn pick_le_u16(&self) -> u16 {
        u16::from_le_bytes([self[0], self[1]])
    }
}

pub trait PickU32 {
    fn pick_le_u32(&self) -> u32;
}

impl<const N: usize> PickU32 for [u8; N] {
    #[inline]
    fn pick_le_u32(&self) -> u32 {
        u32::from_le_bytes([self[0], self[1], self[2], self[3]])
    }
}

pub trait PickU64 {
    fn pick_le_u64(&self) -> u64;
}

impl<const N: usize> PickU64 for [u8; N] {
    #[inline]
    fn pick_le_u64(&self) -> u64 {
        u64::from_le_bytes([self[0], self[1], self[2], self[3], self[4], self[5], self[6], self[7]])
    }
}

impl PickU16 for [u8] {
    #[inline]
    fn pick_le_u16(&self) -> u16 {
        let _ = self[1];
        u16::from_le_bytes([self[0], self[1]])
    }
}

impl PickU32 for [u8] {
    #[inline]
    fn pick_le_u32(&self) -> u32 {
        let _ = self[3];
        u32::from_le_bytes([self[0], self[1], self[2], self[3]])
    }
}

impl PickU64 for [u8] {
    #[inline]
    fn pick_le_u64(&self) -> u64 {
        let _ = self[7];
        u64::from_le_bytes([self[0], self[1], self[2], self[3], self[4], self[5], self[6], self[7]])
    }
}

pub trait PickU128 {
    fn pick_le_u128(&self) -> u128;
}

impl<const N: usize> PickU128 for [u8; N] {
    #[inline]
    fn pick_le_u128(&self) -> u128 {
        u128::from_le_bytes(self.to_array())
    }
}


impl PickU128 for [u8] {
    #[inline]
    fn pick_le_u128(&self) -> u128 {
        u128::from_le_bytes(self.to_array())
    }
}

pub trait PickU256 {
    fn pick_le_u256(&self) -> U256;
}

impl<const N: usize> PickU256 for [u8; N] {
    #[inline]
    fn pick_le_u256(&self) -> U256 {
        U256::from_le_bytes(&self.to_array())
    }
}

impl PickU256 for [u8] {
    #[inline]
    fn pick_le_u256(&self) -> U256 {
        U256::from_le_bytes(&self.to_array())
    }
}


pub trait PickAtMost<const N: usize> {
    fn pick_at_most(&self) -> [u8; N];
}

impl<const N: usize> PickAtMost<N> for [u8] {
    #[inline]
    fn pick_at_most(&self) -> [u8; N] {
        let mut buf = [0u8; N];
        let n = core::cmp::min(N, self.len());
        buf[..n].copy_from_slice(&self[..n]);
        buf
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_pick_uint() {
        let u = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88u8];
        assert_eq!(u.pick_le_u16(), 0x2211);
        assert_eq!(u.pick_le_u32(), 0x44332211);
        assert_eq!(u.pick_le_u64(), 0x8877665544332211);

        assert_eq!(u.as_slice().pick_le_u16(), 0x2211);
        assert_eq!(u.as_slice().pick_le_u32(), 0x44332211);
        assert_eq!(u.as_slice().pick_le_u64(), 0x8877665544332211);
    }
}