use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::sync::RwLock;
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use serde_json::Value;
use crate::core::transaction::Transaction;
use crate::crypto::hash;
use crate::io::{BinReader, BinWriter};
use crate::util::{self, Uint256};
use crate::vm::stackitem::{self, StackItem};

const MAX_TRANSACTIONS_PER_BLOCK: u16 = u16::MAX;

#[derive(Debug, Clone)]
pub struct MaxContentsPerBlockError;

impl fmt::Display for MaxContentsPerBlockError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "the number of contents exceeds the maximum number of contents per block")
    }
}

impl Error for MaxContentsPerBlockError {}

lazy_static! {
    static ref EXPECTED_HEADER_SIZE_WITH_EMPTY_WITNESS: usize = {
        let header = Header::default();
        io::get_var_size(&header)
    };
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Block {
    pub header: Header,
    pub transactions: Vec<Transaction>,
    pub trimmed: bool,
}

#[derive(Serialize, Deserialize)]
struct AuxBlockOut {
    transactions: Vec<Transaction>,
}

#[derive(Serialize, Deserialize)]
struct AuxBlockIn {
    transactions: Vec<Value>,
}

impl Block {
    pub fn compute_merkle_root(&self) -> Uint256 {
        let hashes: Vec<Uint256> = self.transactions.iter().map(|tx| tx.hash()).collect();
        hash::calc_merkle_root(&hashes)
    }

    pub fn rebuild_merkle_root(&mut self) {
        self.header.merkle_root = self.compute_merkle_root();
    }

    pub fn new_trimmed_from_reader(state_root_enabled: bool, br: &mut BinReader) -> Result<Block, Box<dyn Error>> {
        let mut block = Block {
            header: Header {
                state_root_enabled,
                ..Default::default()
            },
            transactions: Vec::new(),
            trimmed: true,
        };

        block.header.decode_binary(br)?;
        let len_hashes = br.read_var_uint()?;
        if len_hashes > MAX_TRANSACTIONS_PER_BLOCK as u64 {
            return Err(Box::new(MaxContentsPerBlockError));
        }
        if len_hashes > 0 {
            block.transactions = Vec::with_capacity(len_hashes as usize);
            for _ in 0..len_hashes {
                let mut hash = Uint256::default();
                hash.decode_binary(br)?;
                block.transactions.push(Transaction::new_trimmed_tx(hash));
            }
        }

        Ok(block)
    }

    pub fn new(state_root_enabled: bool) -> Block {
        Block {
            header: Header {
                state_root_enabled,
                ..Default::default()
            },
            transactions: Vec::new(),
            trimmed: false,
        }
    }

    pub fn encode_trimmed(&self, w: &mut BinWriter) {
        self.header.encode_binary(w);
        w.write_var_uint(self.transactions.len() as u64);
        for tx in &self.transactions {
            let h = tx.hash();
            h.encode_binary(w);
        }
    }

    pub fn decode_binary(&mut self, br: &mut BinReader) -> Result<(), Box<dyn Error>> {
        self.header.decode_binary(br)?;
        let contents_count = br.read_var_uint()?;
        if contents_count > MAX_TRANSACTIONS_PER_BLOCK as u64 {
            return Err(Box::new(MaxContentsPerBlockError));
        }
        self.transactions = Vec::with_capacity(contents_count as usize);
        for _ in 0..contents_count {
            let mut tx = Transaction::default();
            tx.decode_binary(br)?;
            self.transactions.push(tx);
        }
        Ok(())
    }

    pub fn encode_binary(&self, bw: &mut BinWriter) {
        self.header.encode_binary(bw);
        bw.write_var_uint(self.transactions.len() as u64);
        for tx in &self.transactions {
            tx.encode_binary(bw);
        }
    }

    pub fn marshal_json(&self) -> Result<String, Box<dyn Error>> {
        let abo = AuxBlockOut {
            transactions: self.transactions.clone(),
        };
        let auxb = serde_json::to_string(&abo)?;
        let base_bytes = serde_json::to_string(&self.header)?;

        // Stitch them together.
        if !base_bytes.ends_with('}') || !auxb.starts_with('{') {
            return Err(Box::new(fmt::Error));
        }
        let mut base_bytes = base_bytes;
        base_bytes.pop();
        base_bytes.push(',');
        base_bytes.push_str(&auxb[1..]);
        Ok(base_bytes)
    }

    pub fn unmarshal_json(&mut self, data: &str) -> Result<(), Box<dyn Error>> {
        let auxb: AuxBlockIn = serde_json::from_str(data)?;
        self.header = serde_json::from_str(data)?;
        if !auxb.transactions.is_empty() {
            self.transactions = Vec::with_capacity(auxb.transactions.len());
            for tx_bytes in auxb.transactions {
                let tx: Transaction = serde_json::from_value(tx_bytes)?;
                self.transactions.push(tx);
            }
        }
        Ok(())
    }

    pub fn get_expected_block_size(&self) -> usize {
        let transactions_size: usize = self.transactions.iter().map(|tx| tx.size()).sum();
        self.get_expected_block_size_without_transactions(self.transactions.len()) + transactions_size
    }

    pub fn get_expected_block_size_without_transactions(&self, tx_count: usize) -> usize {
        let size = *EXPECTED_HEADER_SIZE_WITH_EMPTY_WITNESS - 1 - 1 + // 1 is for the zero-length (new(Header)).script.invocation/verification
            io::get_var_size(&self.header.script) +
            io::get_var_size(&tx_count);
        if self.header.state_root_enabled {
            size + Uint256::size()
        } else {
            size
        }
    }

    pub fn to_stack_item(&self) -> StackItem {
        let mut items = vec![
            stackitem::new_byte_array(self.header.hash().to_bytes_be()),
            stackitem::new_big_integer(self.header.version.into()),
            stackitem::new_byte_array(self.header.prev_hash.to_bytes_be()),
            stackitem::new_byte_array(self.header.merkle_root.to_bytes_be()),
            stackitem::new_big_integer(self.header.timestamp.into()),
            stackitem::new_big_integer(self.header.nonce.into()),
            stackitem::new_big_integer(self.header.index.into()),
            stackitem::new_big_integer(self.header.primary_index.into()),
            stackitem::new_byte_array(self.header.next_consensus.to_bytes_be()),
            stackitem::new_big_integer(self.transactions.len().into()),
        ];
        if self.header.state_root_enabled {
            items.push(stackitem::new_byte_array(self.header.prev_state_root.to_bytes_be()));
        }

        stackitem::new_array(items)
    }
}
