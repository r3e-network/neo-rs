// Transaction represents a NEO transaction. It's similar to Transaction class
// in Neo .net framework.
use crate::interop::Hash256;
use crate::interop::Hash160;

pub struct Transaction {
    // Hash represents the hash (256 bit BE value in a 32 byte slice) of the
    // given transaction (which also is its ID).
    pub hash: Hash256,
    // Version represents the transaction version.
    pub version: i32,
    // Nonce is a random number to avoid hash collision.
    pub nonce: i32,
    // Sender represents the sender (160 bit BE value in a 20 byte slice) of the
    // given Transaction.
    pub sender: Hash160,
    // SysFee represents the fee to be burned.
    pub sys_fee: i32,
    // NetFee represents the fee to be distributed to consensus nodes.
    pub net_fee: i32,
    // ValidUntilBlock is the maximum blockchain height exceeding which
    // a transaction should fail verification.
    pub valid_until_block: i32,
    // Script represents a code to run in NeoVM for this transaction.
    pub script: Vec<u8>,
}
