use crate::runtime::RuntimeSnapshot;

use super::Runtime;

impl Runtime {
    pub fn snapshot(&self) -> RuntimeSnapshot {
        RuntimeSnapshot {
            blockchain: self.blockchain.snapshot(),
            base_fee: self.fees.base_fee(),
            byte_fee: self.fees.byte_fee(),
            pending: self.tx_pool.snapshot(),
        }
    }

    pub fn restore_snapshot(&mut self, snapshot: RuntimeSnapshot) {
        self.blockchain.restore_snapshot(snapshot.blockchain);
        self.fees.update_base_fee(snapshot.base_fee);
        self.fees.update_byte_fee(snapshot.byte_fee);
        self.tx_pool.restore(snapshot.pending);
    }
}
