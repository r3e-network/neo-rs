use alloc::vec::Vec;

pub(crate) struct SeedDeriver {
    multiplier: u32,
    tweak: u32,
}

impl SeedDeriver {
    pub fn new(multiplier: u32, tweak: u32) -> Self {
        Self { multiplier, tweak }
    }

    pub fn derive(&self, count: usize) -> Vec<u32> {
        (0..count)
            .map(|i| {
                (i as u32)
                    .wrapping_mul(self.multiplier)
                    .wrapping_add(self.tweak)
            })
            .collect()
    }
}
