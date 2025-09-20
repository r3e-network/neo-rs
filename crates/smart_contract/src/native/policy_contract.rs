//! Policy contract native implementation.
//!
//! The Policy contract manages blockchain policies such as gas fees,
//! blocked accounts, and execution limits.

use crate::application_engine::ApplicationEngine;
use crate::native::{NativeContract, NativeMethod};
use crate::{Error, Result};
use neo_config::{
    ADDRESS_SIZE, HASH_SIZE, MAX_BLOCK_SIZE, MAX_SCRIPT_SIZE, MAX_TRANSACTIONS_PER_BLOCK,
    SECONDS_PER_BLOCK,
};
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
    pub const MAX_TRANSACTIONS_PER_BLOCK: u32 = MAX_TRANSACTIONS_PER_BLOCK as u32;

    /// The maximum block size.
    pub const MAX_BLOCK_SIZE: u32 = MAX_BLOCK_SIZE as u32;

    /// The maximum block system fee.
    pub const MAX_BLOCK_SYSTEM_FEE: i64 = 900000000000; // 9000 GAS

    /// The maximum traceable blocks.
    pub const MAX_MAX_TRACEABLE_BLOCKS: u32 = 2102400; // About 1 year

    pub const MAX_BLOCK_SIZE_KEY: &'static [u8] = b"MaxBlockSize";
    pub const MAX_BLOCK_SYSTEM_FEE_KEY: &'static [u8] = b"MaxBlockSystemFee";
    pub const MAX_TRANSACTIONS_PER_BLOCK_KEY: &'static [u8] = b"MaxTransactionsPerBlock";
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
        // Policy contract hash: 0xcc5e4edd9f5f8dba8bb65734541df7a1c081c67b
        let hash = UInt160::from_bytes(&[
            0xcc, 0x5e, 0x4e, 0xdd, 0x9f, 0x5f, 0x8d, 0xba, 0x8b, 0xb6, 0x57, 0x34, 0x54, 0x1d,
            0xf7, 0xa1, 0xc0, 0x81, 0xc6, 0x7b,
        ])
        .expect("Operation failed");

        let methods = vec![
            NativeMethod::safe("getFeePerByte".to_string(), 1 << SECONDS_PER_BLOCK),
            NativeMethod::safe("getExecFeeFactor".to_string(), 1 << SECONDS_PER_BLOCK),
            NativeMethod::safe("getStoragePrice".to_string(), 1 << SECONDS_PER_BLOCK),
            NativeMethod::safe("getAttributeFee".to_string(), 1 << SECONDS_PER_BLOCK),
            NativeMethod::safe(
                "getMaxTransactionsPerBlock".to_string(),
                1 << SECONDS_PER_BLOCK,
            ),
            NativeMethod::safe("getMaxBlockSize".to_string(), 1 << SECONDS_PER_BLOCK),
            NativeMethod::safe("getMaxBlockSystemFee".to_string(), 1 << SECONDS_PER_BLOCK),
            NativeMethod::safe("getMaxTraceableBlocks".to_string(), 1 << SECONDS_PER_BLOCK),
            NativeMethod::unsafe_method("setFeePerByte".to_string(), 1 << SECONDS_PER_BLOCK, 0x01),
            NativeMethod::unsafe_method(
                "setExecFeeFactor".to_string(),
                1 << SECONDS_PER_BLOCK,
                0x01,
            ),
            NativeMethod::unsafe_method(
                "setStoragePrice".to_string(),
                1 << SECONDS_PER_BLOCK,
                0x01,
            ),
            NativeMethod::unsafe_method(
                "setAttributeFee".to_string(),
                1 << SECONDS_PER_BLOCK,
                0x01,
            ),
            NativeMethod::unsafe_method(
                "setMaxTransactionsPerBlock".to_string(),
                1 << SECONDS_PER_BLOCK,
                0x01,
            ),
            NativeMethod::unsafe_method(
                "setMaxBlockSize".to_string(),
                1 << SECONDS_PER_BLOCK,
                0x01,
            ),
            NativeMethod::unsafe_method(
                "setMaxBlockSystemFee".to_string(),
                1 << SECONDS_PER_BLOCK,
                0x01,
            ),
            NativeMethod::unsafe_method(
                "setMaxTraceableBlocks".to_string(),
                1 << SECONDS_PER_BLOCK,
                0x01,
            ),
            // Account blocking methods
            NativeMethod::safe("getBlockedAccounts".to_string(), 1 << SECONDS_PER_BLOCK),
            NativeMethod::unsafe_method("blockAccount".to_string(), 1 << SECONDS_PER_BLOCK, 0x01),
            NativeMethod::unsafe_method("unblockAccount".to_string(), 1 << SECONDS_PER_BLOCK, 0x01),
            NativeMethod::safe("isBlocked".to_string(), 1 << SECONDS_PER_BLOCK),
        ];

        Self { hash, methods }
    }

    fn read_u32_setting(
        &self,
        engine: &mut ApplicationEngine,
        key: &[u8],
        default: u32,
    ) -> Result<u32> {
        let context = engine.get_native_storage_context(&self.hash)?;
        if let Some(value) = engine.get_storage_item(&context, key) {
            if value.is_empty() {
                return Ok(0);
            }
            if value.len() > 4 {
                return Err(Error::NativeContractError(
                    "Stored policy value exceeds u32 capacity".to_string(),
                ));
            }
            let mut buf = [0u8; 4];
            buf[..value.len()].copy_from_slice(&value);
            return Ok(u32::from_le_bytes(buf));
        }
        Ok(default)
    }

    fn read_i64_setting(
        &self,
        engine: &mut ApplicationEngine,
        key: &[u8],
        default: i64,
    ) -> Result<i64> {
        let context = engine.get_native_storage_context(&self.hash)?;
        if let Some(value) = engine.get_storage_item(&context, key) {
            if value.is_empty() {
                return Ok(0);
            }
            if value.len() > 8 {
                return Err(Error::NativeContractError(
                    "Stored policy value exceeds i64 capacity".to_string(),
                ));
            }
            let mut buf = [0u8; 8];
            buf[..value.len()].copy_from_slice(&value);
            return Ok(i64::from_le_bytes(buf));
        }
        Ok(default)
    }

    fn trim_le_bytes_u32(value: u32) -> Vec<u8> {
        if value == 0 {
            return Vec::new();
        }
        let mut bytes = value.to_le_bytes().to_vec();
        while bytes.len() > 1 && bytes.last() == Some(&0) {
            bytes.pop();
        }
        if let Some(&last) = bytes.last() {
            if last & 0x80 != 0 {
                bytes.push(0);
            }
        }
        bytes
    }

    fn trim_le_bytes_i64(value: i64) -> Vec<u8> {
        if value == 0 {
            return Vec::new();
        }
        let mut bytes = value.to_le_bytes().to_vec();
        while bytes.len() > 1 && bytes.last() == Some(&0) {
            bytes.pop();
        }
        if let Some(&last) = bytes.last() {
            if last & 0x80 != 0 {
                bytes.push(0);
            }
        }
        bytes
    }

    fn parse_u32_le(bytes: &[u8]) -> Result<u32> {
        if bytes.is_empty() {
            return Ok(0);
        }
        if bytes.len() > 4 {
            return Err(Error::NativeContractError(
                "Policy value exceeds 32-bit capacity".to_string(),
            ));
        }
        let mut buf = [0u8; 4];
        buf[..bytes.len()].copy_from_slice(bytes);
        Ok(u32::from_le_bytes(buf))
    }

    fn parse_i64_le(bytes: &[u8]) -> Result<i64> {
        if bytes.is_empty() {
            return Ok(0);
        }
        if bytes.len() > 8 {
            return Err(Error::NativeContractError(
                "Policy value exceeds 64-bit capacity".to_string(),
            ));
        }
        let mut buf = [0u8; 8];
        buf[..bytes.len()].copy_from_slice(bytes);
        Ok(i64::from_le_bytes(buf))
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
            "getFeePerByte" => self.get_fee_per_byte(engine),
            "getExecFeeFactor" => self.get_exec_fee_factor(engine),
            "getStoragePrice" => self.get_storage_price(engine),
            "getAttributeFee" => self.get_attribute_fee(engine),
            "getMaxTransactionsPerBlock" => self.get_max_transactions_per_block(engine),
            "getMaxBlockSize" => self.get_max_block_size(engine),
            "getMaxBlockSystemFee" => self.get_max_block_system_fee(engine),
            "getMaxTraceableBlocks" => self.get_max_traceable_blocks(engine),

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

    fn get_max_transactions_per_block(&self, engine: &mut ApplicationEngine) -> Result<Vec<u8>> {
        let value = self.read_u32_setting(
            engine,
            Self::MAX_TRANSACTIONS_PER_BLOCK_KEY,
            Self::MAX_TRANSACTIONS_PER_BLOCK,
        )?;
        Ok(Self::trim_le_bytes_u32(value))
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

        let value_bytes = &args[0];
        let value = Self::parse_u32_le(value_bytes)?;

        if value < 1 || value > 1000 {
            return Err(Error::NativeContractError(
                "Max transactions per block must be between 1 and 1000".to_string(),
            ));
        }

        // Get storage context and store the value
        let context = engine.get_native_storage_context(&self.hash)?;
        let encoded = Self::trim_le_bytes_u32(value);
        engine.put_storage_item(&context, Self::MAX_TRANSACTIONS_PER_BLOCK_KEY, &encoded)?;

        Ok(vec![1]) // Return true for success
    }

    fn get_max_block_size(&self, engine: &mut ApplicationEngine) -> Result<Vec<u8>> {
        let value =
            self.read_u32_setting(engine, Self::MAX_BLOCK_SIZE_KEY, Self::MAX_BLOCK_SIZE)?;
        Ok(Self::trim_le_bytes_u32(value))
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

        let value_bytes = &args[0];
        let value = Self::parse_u32_le(value_bytes)?;

        if value < (MAX_SCRIPT_SIZE as u32)
            || value
                > ((HASH_SIZE as u64) * (MAX_SCRIPT_SIZE as u64) * (MAX_SCRIPT_SIZE as u64)) as u32
        {
            return Err(Error::NativeContractError(
                "Max block size must be between 1KB and 32MB".to_string(),
            ));
        }

        let context = engine.get_native_storage_context(&self.hash)?;
        let encoded = Self::trim_le_bytes_u32(value);
        engine.put_storage_item(&context, Self::MAX_BLOCK_SIZE_KEY, &encoded)?;
        Ok(vec![1]) // Return true for success
    }

    fn get_max_block_system_fee(&self, engine: &mut ApplicationEngine) -> Result<Vec<u8>> {
        let value = self.read_i64_setting(
            engine,
            Self::MAX_BLOCK_SYSTEM_FEE_KEY,
            Self::MAX_BLOCK_SYSTEM_FEE,
        )?;
        Ok(Self::trim_le_bytes_i64(value))
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

        let value_bytes = &args[0];
        let value = Self::parse_i64_le(value_bytes)?;

        if value <= 0 || value > 10_000_000_000_000 {
            // 100,000 GAS max
            return Err(Error::NativeContractError(
                "Max block system fee must be positive and reasonable".to_string(),
            ));
        }

        let context = engine.get_native_storage_context(&self.hash)?;
        let encoded = Self::trim_le_bytes_i64(value);
        engine.put_storage_item(&context, Self::MAX_BLOCK_SYSTEM_FEE_KEY, &encoded)?;
        Ok(vec![1]) // Return true for success
    }

    fn get_fee_per_byte(&self, engine: &mut ApplicationEngine) -> Result<Vec<u8>> {
        let value = self.read_i64_setting(
            engine,
            Self::FEE_PER_BYTE_KEY,
            Self::DEFAULT_FEE_PER_BYTE as i64,
        )?;
        Ok(Self::trim_le_bytes_i64(value))
    }

    fn get_exec_fee_factor(&self, engine: &mut ApplicationEngine) -> Result<Vec<u8>> {
        let value = self.read_u32_setting(
            engine,
            Self::EXEC_FEE_FACTOR_KEY,
            Self::DEFAULT_EXEC_FEE_FACTOR,
        )?;
        Ok(Self::trim_le_bytes_u32(value))
    }

    fn get_storage_price(&self, engine: &mut ApplicationEngine) -> Result<Vec<u8>> {
        let value =
            self.read_u32_setting(engine, Self::STORAGE_PRICE_KEY, Self::DEFAULT_STORAGE_PRICE)?;
        Ok(Self::trim_le_bytes_u32(value))
    }

    fn get_attribute_fee(&self, engine: &mut ApplicationEngine) -> Result<Vec<u8>> {
        let value = self.read_i64_setting(
            engine,
            Self::ATTRIBUTE_FEE_KEY,
            Self::DEFAULT_ATTRIBUTE_FEE as i64,
        )?;
        Ok(Self::trim_le_bytes_i64(value))
    }

    fn get_max_traceable_blocks(&self, engine: &mut ApplicationEngine) -> Result<Vec<u8>> {
        let value = self.read_u32_setting(
            engine,
            Self::MAX_TRACEABLE_BLOCKS_KEY,
            Self::MAX_MAX_TRACEABLE_BLOCKS,
        )?;
        Ok(Self::trim_le_bytes_u32(value))
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

        let value_bytes = &args[0];
        let value = Self::parse_i64_le(value_bytes)?;

        if value < 0 || value > 100_000_000 {
            return Err(Error::NativeContractError(
                "Fee per byte must be between 0 and 100,000,000".to_string(),
            ));
        }

        let context = engine.get_native_storage_context(&self.hash)?;
        let encoded = Self::trim_le_bytes_i64(value);
        engine.put_storage_item(&context, Self::FEE_PER_BYTE_KEY, &encoded)?;
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

        let value_bytes = &args[0];
        let value = Self::parse_u32_le(value_bytes)?;

        if value == 0 || value > 1000 {
            return Err(Error::NativeContractError(
                "Exec fee factor must be between 1 and 1000".to_string(),
            ));
        }

        let context = engine.get_native_storage_context(&self.hash)?;
        let encoded = Self::trim_le_bytes_u32(value);
        engine.put_storage_item(&context, Self::EXEC_FEE_FACTOR_KEY, &encoded)?;
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

        let value_bytes = &args[0];
        let value = Self::parse_u32_le(value_bytes)?;

        if value == 0 || value > 100_000 {
            return Err(Error::NativeContractError(
                "Storage price must be between 1 and 100,000 datoshi per byte".to_string(),
            ));
        }

        let context = engine.get_native_storage_context(&self.hash)?;
        let encoded = Self::trim_le_bytes_u32(value);
        engine.put_storage_item(&context, Self::STORAGE_PRICE_KEY, &encoded)?;
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

        let value_bytes = &args[0];
        let value = Self::parse_i64_le(value_bytes)?;

        if value < 0 || value > 1_000_000_000 {
            // Max 10 GAS
            return Err(Error::NativeContractError(
                "Attribute fee must be between 0 and 1,000,000,000 datoshi".to_string(),
            ));
        }

        let context = engine.get_native_storage_context(&self.hash)?;
        let encoded = Self::trim_le_bytes_i64(value);
        engine.put_storage_item(&context, Self::ATTRIBUTE_FEE_KEY, &encoded)?;
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

        let value_bytes = &args[0];
        let value = Self::parse_u32_le(value_bytes)?;

        if value == 0 || value > 2_102_400 {
            return Err(Error::NativeContractError(
                "Max traceable blocks must be between 1 and 2,102,400".to_string(),
            ));
        }

        let context = engine.get_native_storage_context(&self.hash)?;
        let encoded = Self::trim_le_bytes_u32(value);
        engine.put_storage_item(&context, Self::MAX_TRACEABLE_BLOCKS_KEY, &encoded)?;
        Ok(vec![1]) // Return true for success
    }

    fn get_blocked_accounts(&self, engine: &mut ApplicationEngine) -> Result<Vec<u8>> {
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

        let account_bytes = &args[0];

        if account_bytes.len() != ADDRESS_SIZE {
            return Err(Error::NativeContractError(
                "Invalid account hash length (must be ADDRESS_SIZE bytes)".to_string(),
            ));
        }

        // Validate account hash is not zero
        if account_bytes.iter().all(|&b| b == 0) {
            return Err(Error::NativeContractError(
                "Cannot block zero account hash".to_string(),
            ));
        }

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

        let account_bytes = &args[0];

        if account_bytes.len() != ADDRESS_SIZE {
            return Err(Error::NativeContractError(
                "Invalid account hash length (must be ADDRESS_SIZE bytes)".to_string(),
            ));
        }

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

        let account_bytes = &args[0];

        if account_bytes.len() != ADDRESS_SIZE {
            return Err(Error::NativeContractError(
                "Invalid account hash length (must be ADDRESS_SIZE bytes)".to_string(),
            ));
        }

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
                while offset + ADDRESS_SIZE <= data.len() {
                    let hash_bytes = &data[offset..offset + ADDRESS_SIZE];
                    accounts.push(UInt160::from_bytes(hash_bytes)?);
                    offset += ADDRESS_SIZE;
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
            data.extend_from_slice(&account.as_bytes());
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
