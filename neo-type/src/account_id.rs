use crate::ACCOUNT_SIZE;

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct AccountId {
    version: u8,
    account: [u8; ACCOUNT_SIZE],
}

impl AccountId {
    #[inline]
    pub fn version(&self) -> u8 {
        self.version
    }
}

impl AsRef<[u8; ACCOUNT_SIZE]> for AccountId {
    #[inline]
    fn as_ref(&self) -> &[u8; ACCOUNT_SIZE] {
        &self.account
    }
}

impl AsRef<[u8]> for AccountId {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.account
    }
}
