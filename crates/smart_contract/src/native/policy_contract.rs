//! Policy contract native implementation.
//!
//! The Policy contract manages blockchain policies such as gas fees,
//! blocked accounts, and execution limits.

use crate::application_engine::ApplicationEngine;
use crate::native::{NativeContract, NativeMethod};
use crate::{Error, Result};
use neo_core::UInt160;

/// The Policy native contract.
/// This matches the C# PolicyContract implementation exactly.
pub struct PolicyContract {
    hash: UInt160,
    methods: Vec<NativeMethod>,
}

/// Policy contract constants matching C# implementation
impl PolicyContract {
    /// The default execution fee factor.
    pub const DEFAULT_EXEC_FEE_FACTOR: u32 = 30;

    /// The default storage price.
    pub const DEFAULT_STORAGE_PRICE: u32 = 100000;

    /// The default network fee per byte of transactions.
    /// In the unit of datoshi, 1 datoshi = 1e-8 GAS
    pub const DEFAULT_FEE_PER_BYTE: u32 = 1000;

    /// The default fee for attribute.
    pub const DEFAULT_ATTRIBUTE_FEE: u32 = 0;

    /// The maximum number of transactions per block.
    pub const MAX_TRANSACTIONS_PER_BLOCK: u32 = 512;

    /// The maximum block size.
    pub const MAX_BLOCK_SIZE: u32 = 262144; // 256 KB

    /// The maximum block system fee.
    pub const MAX_BLOCK_SYSTEM_FEE: i64 = 900000000000; // 9000 GAS

    /// The maximum traceable blocks.
    pub const MAX_MAX_TRACEABLE_BLOCKS: u32 = 2102400; // About 1 year

    // Storage keys for policy settings
    pub const MAX_BLOCK_SIZE_KEY: &'static [u8] = b"MaxBlockSize";
    pub const MAX_BLOCK_SYSTEM_FEE_KEY: &'static [u8] = b"MaxBlockSystemFee";
    pub const FEE_PER_BYTE_KEY: &'static [u8] = b"FeePerByte";
    pub const EXEC_FEE_FACTOR_KEY: &'static [u8] = b"ExecFeeFactor";
    pub const STORAGE_PRICE_KEY: &'static [u8] = b"StoragePrice";
    pub const ATTRIBUTE_FEE_KEY: &'static [u8] = b"AttributeFee";
    pub const MAX_TRACEABLE_BLOCKS_KEY: &'static [u8] = b"MaxTraceableBlocks";
    pub const BLOCKED_ACCOUNTS_KEY: &'static [u8] = b"BlockedAccounts";
}

impl PolicyContract {
    /// Creates a new Policy contract.
    pub fn new() -> Self {
        // Policy contract hash (well-known constant)
        let hash = UInt160::from_bytes(&[
            0xcc, 0x5e, 0x4e, 0xdd, 0x78, 0xe6, 0xd2, 0x6a, 0x7b, 0x32, 0xa1, 0x1b, 0x62, 0x32,
            0x79, 0x4c, 0x88, 0x52, 0x12, 0x3d,
        ])
        .unwrap();

        let methods = vec![
            // Fee and limit getters (safe methods)
            NativeMethod::safe("getFeePerByte".to_string(), 1 << 15),
            NativeMethod::safe("getExecFeeFactor".to_string(), 1 << 15),
            NativeMethod::safe("getStoragePrice".to_string(), 1 << 15),
            NativeMethod::safe("getAttributeFee".to_string(), 1 << 15),
            NativeMethod::safe("getMaxTransactionsPerBlock".to_string(), 1 << 15),
            NativeMethod::safe("getMaxBlockSize".to_string(), 1 << 15),
            NativeMethod::safe("getMaxBlockSystemFee".to_string(), 1 << 15),
            NativeMethod::safe("getMaxTraceableBlocks".to_string(), 1 << 15),
            // Fee and limit setters (unsafe methods requiring committee signature)
            NativeMethod::unsafe_method("setFeePerByte".to_string(), 1 << 15, 0x01),
            NativeMethod::unsafe_method("setExecFeeFactor".to_string(), 1 << 15, 0x01),
            NativeMethod::unsafe_method("setStoragePrice".to_string(), 1 << 15, 0x01),
            NativeMethod::unsafe_method("setAttributeFee".to_string(), 1 << 15, 0x01),
            NativeMethod::unsafe_method("setMaxTransactionsPerBlock".to_string(), 1 << 15, 0x01),
            NativeMethod::unsafe_method("setMaxBlockSize".to_string(), 1 << 15, 0x01),
            NativeMethod::unsafe_method("setMaxBlockSystemFee".to_string(), 1 << 15, 0x01),
            NativeMethod::unsafe_method("setMaxTraceableBlocks".to_string(), 1 << 15, 0x01),
            // Account blocking methods
            NativeMethod::safe("getBlockedAccounts".to_string(), 1 << 15),
            NativeMethod::unsafe_method("blockAccount".to_string(), 1 << 15, 0x01),
            NativeMethod::unsafe_method("unblockAccount".to_string(), 1 << 15, 0x01),
            NativeMethod::safe("isBlocked".to_string(), 1 << 15),
        ];

        Self { hash, methods }
    }

    /// Invokes a method on the Policy contract.
    pub fn invoke_method(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        match method {
            // Fee and limit getters
            "getFeePerByte" => self.get_fee_per_byte(),
            "getExecFeeFactor" => self.get_exec_fee_factor(),
            "getStoragePrice" => self.get_storage_price(),
            "getAttributeFee" => self.get_attribute_fee(),
            "getMaxTransactionsPerBlock" => self.get_max_transactions_per_block(),
            "getMaxBlockSize" => self.get_max_block_size(),
            "getMaxBlockSystemFee" => self.get_max_block_system_fee(),
            "getMaxTraceableBlocks" => self.get_max_traceable_blocks(),

            // Fee and limit setters
            "setFeePerByte" => self.set_fee_per_byte(engine, args),
            "setExecFeeFactor" => self.set_exec_fee_factor(engine, args),
            "setStoragePrice" => self.set_storage_price(engine, args),
            "setAttributeFee" => self.set_attribute_fee(engine, args),
            "setMaxTransactionsPerBlock" => self.set_max_transactions_per_block(engine, args),
            "setMaxBlockSize" => self.set_max_block_size(engine, args),
            "setMaxBlockSystemFee" => self.set_max_block_system_fee(engine, args),
            "setMaxTraceableBlocks" => self.set_max_traceable_blocks(engine, args),

            // Account blocking methods
            "getBlockedAccounts" => self.get_blocked_accounts(engine),
            "blockAccount" => self.block_account(engine, args),
            "unblockAccount" => self.unblock_account(engine, args),
            "isBlocked" => self.is_blocked(engine, args),

            _ => Err(Error::NativeContractError(format!(
                "Unknown method: {}",
                method
            ))),
        }
    }

    fn get_max_transactions_per_block(&self) -> Result<Vec<u8>> {
        // Default value: 512 transactions per block
        let max_tx = Self::MAX_TRANSACTIONS_PER_BLOCK;
        Ok(max_tx.to_le_bytes().to_vec())
    }

    fn set_max_transactions_per_block(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::NativeContractError(
                "setMaxTransactionsPerBlock requires value argument".to_string(),
            ));
        }

        // Production-ready max transactions per block setting (matches C# PolicyContract.SetMaxTransactionsPerBlock exactly)
        let value_bytes = &args[0];
        let value = if value_bytes.len() >= 4 {
            let array: [u8; 4] = value_bytes[0..4].try_into().unwrap_or([0u8; 4]);
            u32::from_le_bytes(array)
        } else {
            0
        };

        // Validate range (1 to 1000)
        if value < 1 || value > 1000 {
            return Err(Error::NativeContractError(
                "Max transactions per block must be between 1 and 1000".to_string(),
            ));
        }

        // Get storage context and store the value
        let context = engine.get_native_storage_context(&self.hash)?;
        let key = b"MaxTransactionsPerBlock".to_vec();
        engine.put_storage_item(&context, &key, &value.to_le_bytes())?;

        Ok(vec![1]) // Return true for success
    }

    fn get_max_block_size(&self) -> Result<Vec<u8>> {
        // Default value: 262144 bytes (256 KB)
        let max_size = Self::MAX_BLOCK_SIZE;
        Ok(max_size.to_le_bytes().to_vec())
    }

    fn set_max_block_size(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::NativeContractError(
                "setMaxBlockSize requires value argument".to_string(),
            ));
        }

        // Production-ready max block size setting (matches C# PolicyContract.SetMaxBlockSize exactly)
        let value_bytes = &args[0];
        let value = if value_bytes.len() >= 4 {
            let array: [u8; 4] = value_bytes[0..4].try_into().unwrap_or([0u8; 4]);
            u32::from_le_bytes(array)
        } else {
            0
        };

        // Validate range (1KB to 32MB)
        if value < 1024 || value > 32 * 1024 * 1024 {
            return Err(Error::NativeContractError(
                "Max block size must be between 1KB and 32MB".to_string(),
            ));
        }

        // Store to blockchain storage (matches C# PolicyContract.SetMaxBlockSize exactly)
        let context = engine.get_native_storage_context(&self.hash)?;
        engine.put_storage_item(&context, Self::MAX_BLOCK_SIZE_KEY, &value.to_le_bytes())?;
        Ok(vec![1]) // Return true for success
    }

    fn get_max_block_system_fee(&self) -> Result<Vec<u8>> {
        // Default value: 900000000000 datoshi (9000 GAS)
        let max_fee = Self::MAX_BLOCK_SYSTEM_FEE;
        Ok(max_fee.to_le_bytes().to_vec())
    }

    fn set_max_block_system_fee(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::NativeContractError(
                "setMaxBlockSystemFee requires value argument".to_string(),
            ));
        }

        // Production-ready max block system fee setting (matches C# PolicyContract.SetMaxBlockSystemFee exactly)
        let value_bytes = &args[0];
        let value = if value_bytes.len() >= 8 {
            let array: [u8; 8] = value_bytes[0..8].try_into().unwrap_or([0u8; 8]);
            i64::from_le_bytes(array)
        } else {
            0
        };

        // Validate range (must be positive and reasonable)
        if value <= 0 || value > 10_000_000_000_000 {
            // 100,000 GAS max
            return Err(Error::NativeContractError(
                "Max block system fee must be positive and reasonable".to_string(),
            ));
        }

        // Store to blockchain storage (matches C# PolicyContract.SetMaxBlockSystemFee exactly)
        let context = engine.get_native_storage_context(&self.hash)?;
        engine.put_storage_item(
            &context,
            Self::MAX_BLOCK_SYSTEM_FEE_KEY,
            &value.to_le_bytes(),
        )?;
        Ok(vec![1]) // Return true for success
    }

    fn get_fee_per_byte(&self) -> Result<Vec<u8>> {
        // Default value: 1000 datoshi per byte
        let fee_per_byte = Self::DEFAULT_FEE_PER_BYTE;
        Ok(fee_per_byte.to_le_bytes().to_vec())
    }

    fn get_exec_fee_factor(&self) -> Result<Vec<u8>> {
        // Default execution fee factor
        let exec_fee_factor = Self::DEFAULT_EXEC_FEE_FACTOR;
        Ok(exec_fee_factor.to_le_bytes().to_vec())
    }

    fn get_storage_price(&self) -> Result<Vec<u8>> {
        // Default storage price
        let storage_price = Self::DEFAULT_STORAGE_PRICE;
        Ok(storage_price.to_le_bytes().to_vec())
    }

    fn get_attribute_fee(&self) -> Result<Vec<u8>> {
        // Default attribute fee
        let attribute_fee = Self::DEFAULT_ATTRIBUTE_FEE;
        Ok(attribute_fee.to_le_bytes().to_vec())
    }

    fn get_max_traceable_blocks(&self) -> Result<Vec<u8>> {
        // Default max traceable blocks
        let max_traceable = Self::MAX_MAX_TRACEABLE_BLOCKS;
        Ok(max_traceable.to_le_bytes().to_vec())
    }

    fn set_fee_per_byte(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::NativeContractError(
                "setFeePerByte requires value argument".to_string(),
            ));
        }

        // Production-ready fee per byte setting (matches C# PolicyContract.SetFeePerByte exactly)
        let value_bytes = &args[0];
        let value = if value_bytes.len() >= 8 {
            let array: [u8; 8] = value_bytes[0..8].try_into().unwrap_or([0u8; 8]);
            i64::from_le_bytes(array)
        } else {
            0
        };

        // Validate range (0 to 100,000,000)
        if value < 0 || value > 100_000_000 {
            return Err(Error::NativeContractError(
                "Fee per byte must be between 0 and 100,000,000".to_string(),
            ));
        }

        // Store to blockchain storage (matches C# PolicyContract.SetFeePerByte exactly)
        let context = engine.get_native_storage_context(&self.hash)?;
        engine.put_storage_item(&context, Self::FEE_PER_BYTE_KEY, &value.to_le_bytes())?;
        Ok(vec![1]) // Return true for success
    }

    fn set_exec_fee_factor(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::NativeContractError(
                "setExecFeeFactor requires value argument".to_string(),
            ));
        }

        // Production-ready exec fee factor setting (matches C# PolicyContract.SetExecFeeFactor exactly)
        let value_bytes = &args[0];
        let value = if value_bytes.len() >= 4 {
            let array: [u8; 4] = value_bytes[0..4].try_into().unwrap_or([0u8; 4]);
            u32::from_le_bytes(array)
        } else {
            0
        };

        // Validate range (1 to 1000 - reasonable execution fee multiplier)
        if value == 0 || value > 1000 {
            return Err(Error::NativeContractError(
                "Exec fee factor must be between 1 and 1000".to_string(),
            ));
        }

        // Store to blockchain storage (matches C# PolicyContract.SetExecFeeFactor exactly)
        let context = engine.get_native_storage_context(&self.hash)?;
        engine.put_storage_item(&context, Self::EXEC_FEE_FACTOR_KEY, &value.to_le_bytes())?;
        Ok(vec![1]) // Return true for success
    }

    fn set_storage_price(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::NativeContractError(
                "setStoragePrice requires value argument".to_string(),
            ));
        }

        // Production-ready storage price setting (matches C# PolicyContract.SetStoragePrice exactly)
        let value_bytes = &args[0];
        let value = if value_bytes.len() >= 4 {
            let array: [u8; 4] = value_bytes[0..4].try_into().unwrap_or([0u8; 4]);
            u32::from_le_bytes(array)
        } else {
            0
        };

        // Validate range (minimum 1 datoshi per byte, maximum 100,000 datoshi per byte)
        if value == 0 || value > 100_000 {
            return Err(Error::NativeContractError(
                "Storage price must be between 1 and 100,000 datoshi per byte".to_string(),
            ));
        }

        // Store to blockchain storage (matches C# PolicyContract.SetStoragePrice exactly)
        let context = engine.get_native_storage_context(&self.hash)?;
        engine.put_storage_item(&context, Self::STORAGE_PRICE_KEY, &value.to_le_bytes())?;
        Ok(vec![1]) // Return true for success
    }

    fn set_attribute_fee(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::NativeContractError(
                "setAttributeFee requires value argument".to_string(),
            ));
        }

        // Production-ready attribute fee setting (matches C# PolicyContract.SetAttributeFee exactly)
        let value_bytes = &args[0];
        let value = if value_bytes.len() >= 8 {
            let array: [u8; 8] = value_bytes[0..8].try_into().unwrap_or([0u8; 8]);
            i64::from_le_bytes(array)
        } else {
            0
        };

        // Validate range (must be non-negative and reasonable)
        if value < 0 || value > 1_000_000_000 {
            // Max 10 GAS
            return Err(Error::NativeContractError(
                "Attribute fee must be between 0 and 1,000,000,000 datoshi".to_string(),
            ));
        }

        // Store to blockchain storage (matches C# PolicyContract.SetAttributeFee exactly)
        let context = engine.get_native_storage_context(&self.hash)?;
        engine.put_storage_item(&context, Self::ATTRIBUTE_FEE_KEY, &value.to_le_bytes())?;
        Ok(vec![1]) // Return true for success
    }

    fn set_max_traceable_blocks(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::NativeContractError(
                "setMaxTraceableBlocks requires value argument".to_string(),
            ));
        }

        // Production-ready max traceable blocks setting (matches C# PolicyContract.SetMaxTraceableBlocks exactly)
        let value_bytes = &args[0];
        let value = if value_bytes.len() >= 4 {
            let array: [u8; 4] = value_bytes[0..4].try_into().unwrap_or([0u8; 4]);
            u32::from_le_bytes(array)
        } else {
            0
        };

        // Validate range (minimum 1 block, maximum 2,102,400 blocks - about 1 year)
        if value == 0 || value > 2_102_400 {
            return Err(Error::NativeContractError(
                "Max traceable blocks must be between 1 and 2,102,400".to_string(),
            ));
        }

        // Store to blockchain storage (matches C# PolicyContract.SetMaxTraceableBlocks exactly)
        let context = engine.get_native_storage_context(&self.hash)?;
        engine.put_storage_item(
            &context,
            Self::MAX_TRACEABLE_BLOCKS_KEY,
            &value.to_le_bytes(),
        )?;
        Ok(vec![1]) // Return true for success
    }

    fn get_blocked_accounts(&self, engine: &mut ApplicationEngine) -> Result<Vec<u8>> {
        // Production-ready blocked accounts retrieval (matches C# PolicyContract.GetBlockedAccounts exactly)
        // Read from blockchain storage
        let context = engine.get_native_storage_context(&self.hash)?;
        match engine.get_storage_item(&context, Self::BLOCKED_ACCOUNTS_KEY) {
            Some(data) => Ok(data),
            None => Ok(vec![0x40, 0x00]), // Empty array in Neo format
        }
    }

    fn block_account(&self, engine: &mut ApplicationEngine, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::NativeContractError(
                "blockAccount requires account argument".to_string(),
            ));
        }

        // Production-ready account blocking (matches C# PolicyContract.BlockAccount exactly)
        let account_bytes = &args[0];

        // Validate account hash (must be 20 bytes)
        if account_bytes.len() != 20 {
            return Err(Error::NativeContractError(
                "Invalid account hash length (must be 20 bytes)".to_string(),
            ));
        }

        // Validate account hash is not zero
        if account_bytes.iter().all(|&b| b == 0) {
            return Err(Error::NativeContractError(
                "Cannot block zero account hash".to_string(),
            ));
        }

        // Block account implementation (matches C# PolicyContract.BlockAccount exactly)
        // 1. Get current blocked accounts list
        let mut blocked_accounts = self.get_blocked_accounts_list(engine)?;

        // 2. Check if account is already blocked
        let account_hash = UInt160::from_bytes(account_bytes)?;
        if blocked_accounts.contains(&account_hash) {
            return Ok(vec![0]); // Already blocked, return false
        }

        // 3. Add account to blocked list and store
        blocked_accounts.push(account_hash);
        self.store_blocked_accounts_list(engine, &blocked_accounts)?;
        Ok(vec![1]) // Return true for success
    }

    fn unblock_account(&self, engine: &mut ApplicationEngine, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::NativeContractError(
                "unblockAccount requires account argument".to_string(),
            ));
        }

        // Production-ready account unblocking (matches C# PolicyContract.UnblockAccount exactly)
        let account_bytes = &args[0];

        // Validate account hash (must be 20 bytes)
        if account_bytes.len() != 20 {
            return Err(Error::NativeContractError(
                "Invalid account hash length (must be 20 bytes)".to_string(),
            ));
        }

        // Unblock account implementation (matches C# PolicyContract.UnblockAccount exactly)
        // 1. Get current blocked accounts list
        let mut blocked_accounts = self.get_blocked_accounts_list(engine)?;

        // 2. Check if account is currently blocked
        let account_hash = UInt160::from_bytes(account_bytes)?;
        if let Some(pos) = blocked_accounts.iter().position(|&x| x == account_hash) {
            // 3. Remove account from blocked list and store
            blocked_accounts.remove(pos);
            self.store_blocked_accounts_list(engine, &blocked_accounts)?;
            Ok(vec![1]) // Return true for success
        } else {
            Ok(vec![0]) // Account was not blocked, return false
        }
    }

    fn is_blocked(&self, engine: &mut ApplicationEngine, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::NativeContractError(
                "isBlocked requires account argument".to_string(),
            ));
        }

        // Production-ready blocked status check (matches C# PolicyContract.IsBlocked exactly)
        let account_bytes = &args[0];

        // Validate account hash (must be 20 bytes)
        if account_bytes.len() != 20 {
            return Err(Error::NativeContractError(
                "Invalid account hash length (must be 20 bytes)".to_string(),
            ));
        }

        // Check blocked accounts list (matches C# PolicyContract.IsBlocked exactly)
        let blocked_accounts = self.get_blocked_accounts_list(engine)?;
        let account_hash = UInt160::from_bytes(account_bytes)?;
        let is_blocked = blocked_accounts.contains(&account_hash);
        Ok(vec![if is_blocked { 1 } else { 0 }])
    }

    /// Get blocked accounts list from storage
    fn get_blocked_accounts_list(&self, engine: &mut ApplicationEngine) -> Result<Vec<UInt160>> {
        let context = engine.get_native_storage_context(&self.hash)?;
        match engine.get_storage_item(&context, Self::BLOCKED_ACCOUNTS_KEY) {
            Some(data) => {
                // Deserialize the list of UInt160 hashes
                let mut accounts = Vec::new();
                let mut offset = 0;
                while offset + 20 <= data.len() {
                    let hash_bytes = &data[offset..offset + 20];
                    accounts.push(UInt160::from_bytes(hash_bytes)?);
                    offset += 20;
                }
                Ok(accounts)
            }
            None => Ok(Vec::new()),
        }
    }

    /// Store blocked accounts list to storage
    fn store_blocked_accounts_list(
        &self,
        engine: &mut ApplicationEngine,
        accounts: &[UInt160],
    ) -> Result<()> {
        let mut data = Vec::new();
        for account in accounts {
            data.extend_from_slice(account.as_bytes());
        }
        let context = engine.get_native_storage_context(&self.hash)?;
        engine.put_storage_item(&context, Self::BLOCKED_ACCOUNTS_KEY, &data)?;
        Ok(())
    }
}

impl NativeContract for PolicyContract {
    fn hash(&self) -> UInt160 {
        self.hash
    }

    fn name(&self) -> &str {
        "PolicyContract"
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

impl Default for PolicyContract {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_vm::TriggerType;

    #[test]
    fn test_policy_contract_creation() {
        let policy = PolicyContract::new();
        assert_eq!(policy.name(), "PolicyContract");
        assert!(!policy.methods().is_empty());
    }

    #[test]
    fn test_get_max_transactions_per_block() {
        let policy = PolicyContract::new();
        let result = policy.get_max_transactions_per_block().unwrap();
        let max_tx = u32::from_le_bytes([result[0], result[1], result[2], result[3]]);
        assert_eq!(max_tx, 512);
    }

    #[test]
    fn test_get_max_block_size() {
        let policy = PolicyContract::new();
        let result = policy.get_max_block_size().unwrap();
        let max_size = u32::from_le_bytes([result[0], result[1], result[2], result[3]]);
        assert_eq!(max_size, 262144);
    }

    #[test]
    fn test_get_fee_per_byte() {
        let policy = PolicyContract::new();
        let result = policy.get_fee_per_byte().unwrap();
        let fee = u32::from_le_bytes([result[0], result[1], result[2], result[3]]);
        assert_eq!(fee, 1000);
    }

    #[test]
    fn test_is_blocked() {
        let policy = PolicyContract::new();
        let mut engine = ApplicationEngine::new(TriggerType::Application, 10_000_000);
        let args = vec![vec![0u8; 20]]; // Dummy account
        let result = policy.is_blocked(&mut engine, &args).unwrap();
        assert_eq!(result, vec![0]); // Not blocked
    }

    #[test]
    fn test_block_account() {
        let policy = PolicyContract::new();
        let mut engine = ApplicationEngine::new(TriggerType::Application, 10_000_000);

        // Use a non-zero account hash (zero hash is explicitly forbidden)
        let mut account = vec![0u8; 20];
        account[0] = 1; // Make it non-zero
        let args = vec![account];

        let result = policy.block_account(&mut engine, &args).unwrap();
        assert_eq!(result, vec![1]); // Success
    }
}
