//! GAS token native contract implementation.

use crate::application_engine::ApplicationEngine;
use crate::native::{NativeContract, NativeMethod};
use crate::{Error, Result};
use neo_config::{ADDRESS_SIZE, SECONDS_PER_BLOCK};
use neo_core::UInt160;

/// The GAS token native contract.
pub struct GasToken {
    hash: UInt160,
    methods: Vec<NativeMethod>,
}

impl GasToken {
    /// Creates a new GAS token contract.
    pub fn new() -> Self {
        let hash = UInt160::from_bytes(&[
            0xd2, 0xa4, 0xcf, 0xf3, 0x1f, 0x56, 0xb6, 0xd5, 0x18, 0x4c, 0x19, 0xf2, 0xc0, 0xeb,
            0xb3, 0x77, 0xd3, 0x1a, 0x8c, 0x16,
        ])
        .expect("Operation failed");

        let methods = vec![
            NativeMethod::safe("symbol".to_string(), 0),
            NativeMethod::safe("decimals".to_string(), 0),
            NativeMethod::safe("totalSupply".to_string(), 1 << SECONDS_PER_BLOCK),
            NativeMethod::safe("balanceOf".to_string(), 1 << SECONDS_PER_BLOCK),
            NativeMethod::unsafe_method("transfer".to_string(), 1 << 17, 0x01),
        ];

        Self { hash, methods }
    }

    /// Invokes a method on the GAS token contract.
    pub fn invoke_method(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        match method {
            "symbol" => self.symbol(),
            "decimals" => self.decimals(),
            "totalSupply" => self.total_supply(engine),
            "balanceOf" => self.balance_of(engine, args),
            "transfer" => self.transfer(engine, args),
            _ => Err(Error::NativeContractError(format!(
                "Unknown method: {}",
                method
            ))),
        }
    }

    fn symbol(&self) -> Result<Vec<u8>> {
        Ok(b"GAS".to_vec())
    }

    fn decimals(&self) -> Result<Vec<u8>> {
        Ok(vec![8]) // GAS has 8 decimals
    }

    fn total_supply(&self, engine: &mut ApplicationEngine) -> Result<Vec<u8>> {
        let context = engine.get_native_storage_context(&self.hash)?;

        let storage_key = vec![0x0B]; // Prefix_TotalSupply = 0x0B (matches C# exactly)

        let total_supply = match engine.get_storage_item(&context, &storage_key) {
            Some(value) => {
                if value.len() == 8 {
                    i64::from_le_bytes(value.try_into().unwrap_or([0u8; 8]))
                } else {
                    0i64 // Default to 0 if invalid data
                }
            }
            None => 0i64, // Default to 0 if no total supply stored yet
        };

        Ok(total_supply.to_le_bytes().to_vec())
    }

    fn balance_of(&self, engine: &mut ApplicationEngine, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::NativeContractError(
                "balanceOf requires account argument".to_string(),
            ));
        }

        let account = &args[0];
        if account.len() != ADDRESS_SIZE {
            return Err(Error::NativeContractError(
                "Invalid account length".to_string(),
            ));
        }

        let context = engine.get_native_storage_context(&self.hash)?;

        let storage_key = account.to_vec();

        let balance = match engine.get_storage_item(&context, &storage_key) {
            Some(value) => {
                if value.len() == 8 {
                    i64::from_le_bytes(value.try_into().unwrap_or([0u8; 8]))
                } else {
                    0i64
                }
            }
            None => 0i64,
        };

        Ok(balance.to_le_bytes().to_vec())
    }

    fn transfer(&self, engine: &mut ApplicationEngine, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() < 3 {
            return Err(Error::NativeContractError(
                "transfer requires from, to, and amount arguments".to_string(),
            ));
        }

        let from = &args[0];
        let to = &args[1];
        let amount_bytes = &args[2];

        // Validate addresses
        if from.len() != ADDRESS_SIZE || to.len() != ADDRESS_SIZE {
            return Err(Error::NativeContractError(
                "Invalid address length".to_string(),
            ));
        }

        // Parse amount
        let amount = if amount_bytes.len() >= 8 {
            let array: [u8; 8] = amount_bytes[0..8].try_into().unwrap_or([0u8; 8]);
            i64::from_le_bytes(array)
        } else {
            0
        };

        if amount < 0 {
            return Err(Error::NativeContractError(
                "Amount cannot be negative".to_string(),
            ));
        }

        if from == to {
            return Ok(vec![1]); // Transfer to self is always successful
        }

        let context = engine.get_native_storage_context(&self.hash)?;

        // Create storage keys
        let from_key = from.to_vec();
        let to_key = to.to_vec();

        // Get current balances
        let from_balance = match engine.get_storage_item(&context, &from_key) {
            Some(value) => {
                if value.len() == 8 {
                    i64::from_le_bytes(value.try_into().unwrap_or([0u8; 8]))
                } else {
                    0i64
                }
            }
            None => 0i64,
        };

        let to_balance = match engine.get_storage_item(&context, &to_key) {
            Some(value) => {
                if value.len() == 8 {
                    i64::from_le_bytes(value.try_into().unwrap_or([0u8; 8]))
                } else {
                    0i64
                }
            }
            None => 0i64,
        };

        // Check sufficient balance
        if from_balance < amount {
            return Ok(vec![0]); // Insufficient balance
        }

        // Calculate new balances
        let new_from_balance = from_balance - amount;
        let new_to_balance = to_balance + amount;

        // Update storage
        engine.put_storage_item(&context, &from_key, &new_from_balance.to_le_bytes())?;
        engine.put_storage_item(&context, &to_key, &new_to_balance.to_le_bytes())?;

        // In production implementation, this would emit a proper Transfer event
        Ok(vec![1]) // Return true for success
    }
}

impl NativeContract for GasToken {
    fn hash(&self) -> UInt160 {
        self.hash
    }

    fn name(&self) -> &str {
        "GasToken"
    }

    fn methods(&self) -> &[NativeMethod] {
        &self.methods
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        self.invoke_method(engine, method, args)
    }
}

impl Default for GasToken {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{Error, Result};
    use neo_vm::TriggerType;

    #[test]
    fn test_gas_token_creation() {
        let gas = GasToken::new();
        assert_eq!(gas.name(), "GasToken");
        assert!(!gas.methods().is_empty());
    }

    #[test]
    fn test_gas_token_symbol() {
        let gas = GasToken::new();
        let result = gas.symbol().unwrap();
        assert_eq!(result, b"GAS");
    }

    #[test]
    fn test_gas_token_decimals() {
        let gas = GasToken::new();
        let result = gas.decimals().unwrap();
        assert_eq!(result, vec![8]);
    }

    #[test]
    fn test_gas_token_total_supply() {
        let gas = GasToken::new();
        let mut engine = ApplicationEngine::new(TriggerType::Application, 10_000_000);
        let result = gas.total_supply(&mut engine).unwrap();
        assert_eq!(result.len(), 8); // i64 total supply
    }

    #[test]
    fn test_gas_token_balance_of() {
        let gas = GasToken::new();
        let mut engine = ApplicationEngine::new(TriggerType::Application, 10_000_000);
        let args = vec![vec![0u8; ADDRESS_SIZE]]; // Dummy account
        let result = gas.balance_of(&mut engine, &args).unwrap();
        assert_eq!(result.len(), 8); // i64 balance
    }

    #[test]
    fn test_gas_token_transfer() {
        let gas = GasToken::new();
        let mut engine = ApplicationEngine::new(TriggerType::Application, 10_000_000);

        let from_account = vec![0u8; ADDRESS_SIZE];
        let to_account = vec![1u8; ADDRESS_SIZE];
        let context = engine.get_native_storage_context(&gas.hash).unwrap();

        // Give the from account 5000 GAS initial balance
        let initial_balance = 5000i64;
        engine
            .put_storage_item(&context, &from_account, &initial_balance.to_le_bytes())
            .unwrap();

        let args = vec![
            from_account.clone(),
            to_account.clone(),
            1000i64.to_le_bytes().to_vec(),
        ];

        let result = gas.transfer(&mut engine, &args).unwrap();
        assert_eq!(result, vec![1]); // Success

        // Verify balances after transfer
        let from_balance_args = vec![from_account];
        let from_balance_result = gas
            .balance_of(&mut engine, &from_balance_args)
            .expect("Operation failed");
        let from_balance = i64::from_le_bytes(
            from_balance_result
                .try_into()
                .expect("Conversion should succeed"),
        );
        assert_eq!(from_balance, 4000); // 5000 - 1000

        let to_balance_args = vec![to_account];
        let to_balance_result = gas
            .balance_of(&mut engine, &to_balance_args)
            .expect("Operation failed");
        let to_balance = i64::from_le_bytes(
            to_balance_result
                .try_into()
                .expect("Conversion should succeed"),
        );
        assert_eq!(to_balance, 1000); // Received 1000
    }
}
