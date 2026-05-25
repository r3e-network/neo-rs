use super::{Block, Header, Transaction};
use crate::constants::{BLOCK_MAX_TX_WIRE_LIMIT, MAX_BLOCK_SIZE};
use crate::neo_io::serializable::helper::{
    get_var_size, get_var_size_serializable_slice, serialize_array,
};
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
        let header_size = header.size();

        // Read transaction count
        let tx_count = reader.read_var_int(BLOCK_MAX_TX_WIRE_LIMIT as u64)? as usize;
        if tx_count > BLOCK_MAX_TX_WIRE_LIMIT {
            return Err(IoError::invalid_data(format!(
                "Too many transactions: {} exceeds wire limit {}",
                tx_count, BLOCK_MAX_TX_WIRE_LIMIT
            )));
        }

        // Track cumulative size to prevent DoS attacks
        // MAX_BLOCK_SIZE is 4MB (4,194,304 bytes)
        let mut cumulative_size = header_size + get_var_size(tx_count as u64);
        if cumulative_size > MAX_BLOCK_SIZE {
            return Err(IoError::invalid_data(format!(
                "Block size {} exceeds maximum {}",
                cumulative_size, MAX_BLOCK_SIZE
            )));
        }

        let mut transactions = Vec::with_capacity(tx_count.min(512)); // Cap initial capacity
        for _ in 0..tx_count {
            let tx = <Transaction as Serializable>::deserialize(reader)?;
            cumulative_size += tx.size();

            // Check cumulative size before accepting transaction
            if cumulative_size > MAX_BLOCK_SIZE {
                return Err(IoError::invalid_data(format!(
                    "Block size {} exceeds maximum {}",
                    cumulative_size, MAX_BLOCK_SIZE
                )));
            }

            transactions.push(tx);
        }

        Ok(Self {
            header,
            transactions,
        })
    }
}
