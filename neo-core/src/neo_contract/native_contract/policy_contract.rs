use std::collections::HashMap;
use neo_proc_macros::{contract, contract_impl, contract_method};

/// A native contract that manages the system policies.
#[contract]
pub struct PolicyContract {

}

#[contract_impl]
impl PolicyContract {

    pub const DEFAULT_EXEC_FEE_FACTOR: u32 = 30;
    pub const DEFAULT_STORAGE_PRICE: u32 = 100000;
    pub const DEFAULT_FEE_PER_BYTE: u32 = 1000;
    pub const DEFAULT_ATTRIBUTE_FEE: u32 = 0;
    pub const MAX_EXEC_FEE_FACTOR: u32 = 100;
    pub const MAX_ATTRIBUTE_FEE: u32 = 1_000_000_000;
    pub const MAX_STORAGE_PRICE: u32 = 10_000_000;

    const PREFIX_BLOCKED_ACCOUNT: u8 = 15;
    const PREFIX_FEE_PER_BYTE: u8 = 10;
    const PREFIX_EXEC_FEE_FACTOR: u8 = 18;
    const PREFIX_STORAGE_PRICE: u8 = 19;
    const PREFIX_ATTRIBUTE_FEE: u8 = 20;

    pub fn new() -> Self {
        Self {}
    }

    #[contract_method(cpu_fee = 1 << 15, required_flags = CallFlags::READ_STATES)]
    pub fn get_fee_per_byte(&self, snapshot: &dyn DataCache) -> i64 {
        snapshot.get(&self.create_storage_key(Self::PREFIX_FEE_PER_BYTE))
            .map(|item| item.as_integer().unwrap())
            .unwrap_or(Self::DEFAULT_FEE_PER_BYTE as i64)
    }

    #[contract_method(cpu_fee = 1 << 15, required_flags = CallFlags::READ_STATES)]
    pub fn get_exec_fee_factor(&self, snapshot: &dyn DataCache) -> u32 {
        snapshot.get(&self.create_storage_key(Self::PREFIX_EXEC_FEE_FACTOR))
            .map(|item| item.as_integer().unwrap() as u32)
            .unwrap_or(Self::DEFAULT_EXEC_FEE_FACTOR)
    }

    #[contract_method(cpu_fee = 1 << 15, required_flags = CallFlags::READ_STATES)]
    pub fn get_storage_price(&self, snapshot: &dyn DataCache) -> u32 {
        snapshot.get(&self.create_storage_key(Self::PREFIX_STORAGE_PRICE))
            .map(|item| item.as_integer().unwrap() as u32)
            .unwrap_or(Self::DEFAULT_STORAGE_PRICE)
    }

    #[contract_method(cpu_fee = 1 << 15, required_flags = CallFlags::READ_STATES)]
    pub fn get_attribute_fee(&self, snapshot: &dyn DataCache, attribute_type: u8) -> u32 {
        if !TransactionAttributeType::is_valid(attribute_type) {
            panic!("Invalid attribute type");
        }
        snapshot.try_get(&self.create_storage_key(Self::PREFIX_ATTRIBUTE_FEE).add(&attribute_type))
            .map(|item| item.as_integer().unwrap() as u32)
            .unwrap_or(Self::DEFAULT_ATTRIBUTE_FEE)
    }

    #[contract_method(cpu_fee = 1 << 15, required_flags = CallFlags::READ_STATES)]
    pub fn is_blocked(&self, snapshot: &dyn DataCache, account: &H160) -> bool {
        snapshot.contains(&self.create_storage_key(Self::PREFIX_BLOCKED_ACCOUNT).add(account))
    }

    #[contract_method(cpu_fee = 1 << 15, required_flags = CallFlags::STATES)]
    fn set_attribute_fee(&mut self, engine: &mut ApplicationEngine, attribute_type: u8, value: u32) -> Result<(), String> {
        if !TransactionAttributeType::is_valid(attribute_type) {
            return Err("Invalid attribute type".into());
        }
        if value > Self::MAX_ATTRIBUTE_FEE {
            return Err("Value out of range".into());
        }
        if !self.check_committee(engine) {
            return Err("Not authorized".into());
        }

        let key = self.create_storage_key(Self::PREFIX_ATTRIBUTE_FEE).add(&attribute_type);
        engine.snapshot_cache().put(&key, &StackItem::Integer(value.into()));
        Ok(())
    }

    #[contract_method(cpu_fee = 1 << 15, required_flags = CallFlags::STATES)]
    fn set_fee_per_byte(&mut self, engine: &mut ApplicationEngine, value: i64) -> Result<(), String> {
        if value < 0 || value > 1_00000000 {
            return Err("Value out of range".into());
        }
        if !self.check_committee(engine) {
            return Err("Not authorized".into());
        }
        engine.snapshot_cache().put(&self.create_storage_key(Self::PREFIX_FEE_PER_BYTE), &StackItem::Integer(value.into()));
        Ok(())
    }

    #[contract_method(cpu_fee = 1 << 15, required_flags = CallFlags::STATES)]
    fn set_exec_fee_factor(&mut self, engine: &mut ApplicationEngine, value: u32) -> Result<(), String> {
        if value == 0 || value > Self::MAX_EXEC_FEE_FACTOR {
            return Err("Value out of range".into());
        }
        if !self.check_committee(engine) {
            return Err("Not authorized".into());
        }
        engine.snapshot_cache().put(&self.create_storage_key(Self::PREFIX_EXEC_FEE_FACTOR), &StackItem::Integer(value.into()));
        Ok(())
    }

    #[contract_method(cpu_fee = 1 << 15, required_flags = CallFlags::STATES)]
    fn set_storage_price(&mut self, engine: &mut ApplicationEngine, value: u32) -> Result<(), String> {
        if value == 0 || value > Self::MAX_STORAGE_PRICE {
            return Err("Value out of range".into());
        }
        if !self.check_committee(engine) {
            return Err("Not authorized".into());
        }
        engine.snapshot_cache().put(&self.create_storage_key(Self::PREFIX_STORAGE_PRICE), &StackItem::Integer(value.into()));
        Ok(())
    }

    #[contract_method(cpu_fee = 1 << 15, required_flags = CallFlags::STATES)]
    fn block_account(&mut self, engine: &mut ApplicationEngine, account: H160) -> Result<bool, String> {
        if !self.check_committee(engine) {
            return Err("Not authorized".into());
        }
        self.block_account_internal(engine.snapshot_cache(), account)
    }

    fn block_account_internal(&self, snapshot: &mut dyn DataCache, account: H160) -> Result<bool, String> {
        if self.is_native(&account) {
            return Err("It's impossible to block a native contract".into());
        }

        let key = self.create_storage_key(Self::PREFIX_BLOCKED_ACCOUNT).add(&account);
        if snapshot.contains(&key) {
            return Ok(false);
        }

        snapshot.put(&key, &StackItem::Array(Array::new_empty()));
        Ok(true)
    }

    #[contract_method(cpu_fee = 1 << 15, required_flags = CallFlags::STATES)]
    fn unblock_account(&mut self, engine: &mut ApplicationEngine, account: H160) -> Result<bool, String> {
        if !self.check_committee(engine) {
            return Err("Not authorized".into());
        }

        let key = self.create_storage_key(Self::PREFIX_BLOCKED_ACCOUNT).add(&account);
        if !engine.snapshot_cache().contains(&key) {
            return Ok(false);
        }

        engine.snapshot_cache().delete(&key);
        Ok(true)
    }
}
