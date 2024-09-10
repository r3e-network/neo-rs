use std::convert::TryInto;

pub struct Murmur128 {
    seed: u32,
    length: i32,
    h1: u64,
    h2: u64,
}

const C1: u64 = 0x87c37b91114253d5;
const C2: u64 = 0x4cf5ad432745937f;
const R1: u32 = 31;
const R2: u32 = 33;
const M: u32 = 5;
const N1: u32 = 0x52dce729;
const N2: u32 = 0x38495ab5;

impl Murmur128 {
    pub fn new(seed: u32) -> Self {
        let mut murmur = Murmur128 {
            seed,
            length: 0,
            h1: seed as u64,
            h2: seed as u64,
        };
        murmur.initialize();
        murmur
    }

    pub fn hash_size(&self) -> i32 {
        128
    }

    pub fn initialize(&mut self) {
        self.h1 = self.seed as u64;
        self.h2 = self.seed as u64;
        self.length = 0;
    }

    pub fn hash_core(&mut self, array: &[u8]) {
        let cb_size = array.len();
        self.length += cb_size as i32;
        let remainder = cb_size & 15;
        let aligned_length = cb_size - remainder;

        for i in (0..aligned_length).step_by(16) {
            let k1 = u64::from_le_bytes(array[i..i+8].try_into().unwrap());
            let mut k1 = k1.wrapping_mul(C1);
            k1 = k1.rotate_left(R1);
            k1 = k1.wrapping_mul(C2);
            self.h1 ^= k1;
            self.h1 = self.h1.rotate_left(27);
            self.h1 = self.h1.wrapping_add(self.h2);
            self.h1 = self.h1.wrapping_mul(M as u64).wrapping_add(N1 as u64);

            let k2 = u64::from_le_bytes(array[i+8..i+16].try_into().unwrap());
            let mut k2 = k2.wrapping_mul(C2);
            k2 = k2.rotate_left(R2);
            k2 = k2.wrapping_mul(C1);
            self.h2 ^= k2;
            self.h2 = self.h2.rotate_left(31);
            self.h2 = self.h2.wrapping_add(self.h1);
            self.h2 = self.h2.wrapping_mul(M as u64).wrapping_add(N2 as u64);
        }

        if remainder > 0 {
            let mut remaining_bytes_l = 0u64;
            let mut remaining_bytes_h = 0u64;

            match remainder {
                15 => { remaining_bytes_h ^= (array[aligned_length + 14] as u64) << 48; }
                14 => { remaining_bytes_h ^= (array[aligned_length + 13] as u64) << 40; }
                13 => { remaining_bytes_h ^= (array[aligned_length + 12] as u64) << 32; }
                12 => { remaining_bytes_h ^= (array[aligned_length + 11] as u64) << 24; }
                11 => { remaining_bytes_h ^= (array[aligned_length + 10] as u64) << 16; }
                10 => { remaining_bytes_h ^= (array[aligned_length + 9] as u64) << 8; }
                9 => { remaining_bytes_h ^= array[aligned_length + 8] as u64; }
                8 => { remaining_bytes_l ^= (array[aligned_length + 7] as u64) << 56; }
                7 => { remaining_bytes_l ^= (array[aligned_length + 6] as u64) << 48; }
                6 => { remaining_bytes_l ^= (array[aligned_length + 5] as u64) << 40; }
                5 => { remaining_bytes_l ^= (array[aligned_length + 4] as u64) << 32; }
                4 => { remaining_bytes_l ^= (array[aligned_length + 3] as u64) << 24; }
                3 => { remaining_bytes_l ^= (array[aligned_length + 2] as u64) << 16; }
                2 => { remaining_bytes_l ^= (array[aligned_length + 1] as u64) << 8; }
                1 => { remaining_bytes_l ^= array[aligned_length] as u64; }
                _ => {}
            }

            self.h2 ^= (remaining_bytes_h.wrapping_mul(C2).rotate_left(R2)).wrapping_mul(C1);
            self.h1 ^= (remaining_bytes_l.wrapping_mul(C1).rotate_left(R1)).wrapping_mul(C2);
        }
    }

    pub fn hash_final(&mut self) -> [u8; 16] {
        let len = self.length as u64;
        self.h1 ^= len;
        self.h2 ^= len;

        self.h1 = self.h1.wrapping_add(self.h2);
        self.h2 = self.h2.wrapping_add(self.h1);

        self.h1 = Self::f_mix(self.h1);
        self.h2 = Self::f_mix(self.h2);

        self.h1 = self.h1.wrapping_add(self.h2);
        self.h2 = self.h2.wrapping_add(self.h1);

        let mut result = [0u8; 16];
        result[..8].copy_from_slice(&self.h1.to_le_bytes());
        result[8..].copy_from_slice(&self.h2.to_le_bytes());
        result
    }

    #[inline(always)]
    fn f_mix(mut h: u64) -> u64 {
        h ^= h >> 33;
        h = h.wrapping_mul(0xff51afd7ed558ccd);
        h ^= h >> 33;
        h = h.wrapping_mul(0xc4ceb9fe1a85ec53);
        h ^= h >> 33;
        h
    }
}
