use num_bigint::BigInt;
use rand::Rng;

pub trait RandomExtensions {
    fn next_big_integer(&mut self, size_in_bits: usize) -> BigInt;
}

impl<R: Rng> RandomExtensions for R {
    fn next_big_integer(&mut self, size_in_bits: usize) -> BigInt {
        if size_in_bits == 0 {
            return BigInt::from(0);
        }

        let mut bytes = vec![0u8; (size_in_bits + 7) / 8];
        self.fill_bytes(&mut bytes);

        let last_idx = bytes.len() - 1;
        if size_in_bits % 8 == 0 {
            bytes[last_idx] = 0;
        } else {
            bytes[last_idx] &= (1 << (size_in_bits % 8)) - 1;
        }

        BigInt::from_bytes_le(num_bigint::Sign::Plus, &bytes)
    }
}
