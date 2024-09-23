pub struct Account {
    nonce: u32,
}

impl Account {
    pub fn new() -> Self {
        Account { nonce: 0 }
    }

    // Nonce returns a unique number that can be used as a nonce for new transactions.
    pub fn nonce(&mut self) -> u32 {
        self.nonce += 1;
        self.nonce
    }
}
