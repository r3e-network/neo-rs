use crate::error::ContractError;

#[derive(Debug, Clone, PartialEq)]
pub struct GasMeter {
    limit: u64,
    consumed: u64,
}

impl GasMeter {
    pub fn new(limit: u64) -> Self {
        Self { limit, consumed: 0 }
    }

    pub fn remaining(&self) -> u64 {
        self.limit.saturating_sub(self.consumed)
    }

    pub fn charge(&mut self, amount: u64) -> Result<(), ContractError> {
        self.consumed = self.consumed.saturating_add(amount);
        if self.consumed > self.limit {
            Err(ContractError::Runtime("out of gas"))
        } else {
            Ok(())
        }
    }

    pub fn consumed(&self) -> u64 {
        self.consumed
    }
}
