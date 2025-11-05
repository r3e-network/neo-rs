/// Dynamic fee calculator used by the runtime to estimate transaction costs.
#[derive(Debug, Clone)]
pub struct FeeCalculator {
    base_fee: u64,
    byte_fee: u64,
    surge_multiplier: f32,
}

impl FeeCalculator {
    pub fn new(base_fee: u64, byte_fee: u64) -> Self {
        Self {
            base_fee,
            byte_fee,
            surge_multiplier: 1.0,
        }
    }

    pub fn base_fee(&self) -> u64 {
        self.base_fee
    }

    pub fn byte_fee(&self) -> u64 {
        self.byte_fee
    }

    pub fn surge_multiplier(&self) -> f32 {
        self.surge_multiplier
    }

    pub fn update_base_fee(&mut self, value: u64) {
        self.base_fee = value;
    }

    pub fn update_byte_fee(&mut self, value: u64) {
        self.byte_fee = value;
    }

    pub fn set_surge_multiplier(&mut self, multiplier: f32) {
        self.surge_multiplier = multiplier.max(0.0);
    }

    pub fn estimate(&self, size_bytes: u32) -> u64 {
        let fee = self.base_fee + self.byte_fee.saturating_mul(size_bytes as u64);
        (fee as f32 * self.surge_multiplier).ceil() as u64
    }

    /// Adjust the surge multiplier based on current mempool load.
    pub fn adjust_for_load(&mut self, pending_transactions: usize, capacity: usize) {
        if capacity == 0 {
            self.surge_multiplier = 1.0;
            return;
        }
        let load = pending_transactions as f32 / capacity as f32;
        self.surge_multiplier = if load <= 0.6 {
            1.0
        } else if load <= 0.9 {
            1.2
        } else {
            1.5
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_applies_multiplier() {
        let mut calc = FeeCalculator::new(100, 2);
        assert_eq!(calc.estimate(10), 120);
        calc.adjust_for_load(90, 100);
        assert!(calc.estimate(10) > 120);
    }
}
