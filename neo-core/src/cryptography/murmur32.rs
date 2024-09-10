use std::convert::TryInto;

pub struct Murmur32 {
    seed: u32,
    hash: u32,
    length: usize,
}

impl Murmur32 {
    const C1: u32 = 0xcc9e2d51;
    const C2: u32 = 0x1b873593;
    const R1: u32 = 15;
    const R2: u32 = 13;
    const M: u32 = 5;
    const N: u32 = 0xe6546b64;

    pub fn new(seed: u32) -> Self {
        let mut hasher = Murmur32 {
            seed,
            hash: seed,
            length: 0,
        };
        hasher.initialize();
        hasher
    }

    pub fn hash_size(&self) -> usize {
        32
    }

    pub fn hash_core(&mut self, data: &[u8]) {
        self.length += data.len();
        let mut source = data;

        while source.len() >= 4 {
            let k = u32::from_le_bytes(source[..4].try_into().unwrap());
            let mut k = k.wrapping_mul(Self::C1);
            k = k.rotate_left(Self::R1);
            k = k.wrapping_mul(Self::C2);
            self.hash ^= k;
            self.hash = self.hash.rotate_left(Self::R2);
            self.hash = self.hash.wrapping_mul(Self::M).wrapping_add(Self::N);
            source = &source[4..];
        }

        if !source.is_empty() {
            let mut remaining_bytes = 0u32;
            match source.len() {
                3 => {
                    remaining_bytes ^= (source[2] as u32) << 16;
                    remaining_bytes ^= (source[1] as u32) << 8;
                    remaining_bytes ^= source[0] as u32;
                }
                2 => {
                    remaining_bytes ^= (source[1] as u32) << 8;
                    remaining_bytes ^= source[0] as u32;
                }
                1 => {
                    remaining_bytes ^= source[0] as u32;
                }
                _ => unreachable!(),
            }
            remaining_bytes = remaining_bytes.wrapping_mul(Self::C1);
            remaining_bytes = remaining_bytes.rotate_left(Self::R1);
            remaining_bytes = remaining_bytes.wrapping_mul(Self::C2);
            self.hash ^= remaining_bytes;
        }
    }

    pub fn hash_final(&mut self) -> [u8; 4] {
        self.hash ^= self.length as u32;
        self.hash ^= self.hash >> 16;
        self.hash = self.hash.wrapping_mul(0x85ebca6b);
        self.hash ^= self.hash >> 13;
        self.hash = self.hash.wrapping_mul(0xc2b2ae35);
        self.hash ^= self.hash >> 16;

        self.hash.to_le_bytes()
    }

    pub fn initialize(&mut self) {
        self.hash = self.seed;
        self.length = 0;
    }
}
