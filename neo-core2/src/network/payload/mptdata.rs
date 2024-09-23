use std::error::Error;
use std::fmt;

use crate::io::{BinReader, BinWriter};

#[derive(Debug)]
pub struct MPTData {
    pub nodes: Vec<Vec<u8>>,
}

impl MPTData {
    pub fn encode_binary(&self, w: &mut BinWriter) {
        w.write_var_uint(self.nodes.len() as u64);
        for n in &self.nodes {
            w.write_var_bytes(n);
        }
    }

    pub fn decode_binary(&mut self, r: &mut BinReader) -> Result<(), Box<dyn Error>> {
        let sz = r.read_var_uint();
        if sz == 0 {
            return Err(Box::new(MPTDataError::EmptyNodesList));
        }
        for _ in 0..sz {
            self.nodes.push(r.read_var_bytes());
            if r.err().is_some() {
                return Err(Box::new(MPTDataError::ReadError));
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
enum MPTDataError {
    EmptyNodesList,
    ReadError,
}

impl fmt::Display for MPTDataError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            MPTDataError::EmptyNodesList => write!(f, "empty MPT nodes list"),
            MPTDataError::ReadError => write!(f, "error reading MPT nodes"),
        }
    }
}

impl Error for MPTDataError {}
