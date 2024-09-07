// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


pub const DEFAULT_EXEC_FEE_FACTOR: u64 = 30;
pub const DEFAULT_FEE_PER_BYTE: u64 = 1000;
pub const DEFAULT_ATTR_FEE: u64 = 0;
pub const DEFAULT_STORAGE_PRICE: u64 = 100000;

pub const MAX_EXEC_FEE_FACTOR: u64 = 100;
pub const MAX_NETFEE_PER_BYTE: u64 = 100_000_000;
pub const MAX_STORAGE_PRICE: u64 = 10_000_000;
pub const MAX_ATTR_FEE: u64 = 100_0000_000;

pub const KEY_NETFEE_PER_BYTE: u8 = 10;
pub const KEY_EXEC_FEE_FACTOR: u8 = 18;
pub const KEY_STORAGE_PRICE: u8 = 19;

pub const PREFIX_ATTR_FEE: u8 = 20;
pub const PREFIX_BLOCKED_ACCOUNTS: u8 = 15;


pub struct Policy {
    //
}


#[cfg(test)]
mod test {
    #[test]
    fn test_policy() {
        //
    }
}