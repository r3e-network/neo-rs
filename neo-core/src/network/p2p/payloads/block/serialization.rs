use super::{Block, Header, Transaction};
use crate::constants::BLOCK_MAX_TX_WIRE_LIMIT;
use crate::neo_io::serializable::helper::{get_var_size_serializable_slice, serialize_array};
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};

impl Serializable for Block {
    fn size(&self) -> usize {
        self.header.size() + get_var_size_serializable_slice(&self.transactions)
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        Serializable::serialize(&self.header, writer)?;

        const MAX_TRANSACTIONS: u64 = u16::MAX as u64;
        if self.transactions.len() as u64 > MAX_TRANSACTIONS {
            return Err(IoError::invalid_data("Too many transactions"));
        }
        serialize_array(&self.transactions, writer)
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let header = <Header as Serializable>::deserialize(reader)?;

        // Read transaction count. C# Block.DeserializeTransactions bounds the count
        // by ushort.MaxValue (65535). There is NO block-byte-size consensus limit in
        // Neo core: the only hard cap is the 32MB P2P message payload enforced at the
        // message layer. A 2MB gate here would reject valid blocks C# accepts.
        let tx_count = reader.read_var_int(BLOCK_MAX_TX_WIRE_LIMIT as u64)? as usize;
        if tx_count > BLOCK_MAX_TX_WIRE_LIMIT {
            return Err(IoError::invalid_data(format!(
                "Too many transactions: {} exceeds wire limit {}",
                tx_count, BLOCK_MAX_TX_WIRE_LIMIT
            )));
        }

        let mut transactions = Vec::with_capacity(tx_count.min(512)); // Cap initial capacity
        for _ in 0..tx_count {
            transactions.push(<Transaction as Serializable>::deserialize(reader)?);
        }

        let block = Self {
            header,
            transactions,
        };

        // C# DeserializeTransactions rejects duplicate transaction hashes and a
        // merkle-root mismatch at parse time (FormatException). Mirror both here so
        // the wire-level accept/reject boundary matches C# exactly.
        if !block.verify_no_duplicate_transactions() {
            return Err(IoError::invalid_data(
                "Block contains duplicate transaction hashes",
            ));
        }
        if !block.verify_merkle_root() {
            return Err(IoError::invalid_data(
                "Computed merkle root does not match block header",
            ));
        }

        Ok(block)
    }
}
