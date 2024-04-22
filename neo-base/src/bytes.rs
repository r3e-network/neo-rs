// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

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


pub fn xor_array<const N: usize>(left: &[u8], right: &[u8]) -> [u8; N] {
    let mut d = [0u8; N];
    if left.len() != right.len() {
        panic!("left length {} != right length {}", left.len(), right.len());
    }

    if left.len() != d.len() {
        panic!("source length {} != dest length {}", left.len(), N);
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