use std::error::Error;
use std::fmt;

use crate::core::block::Header;
use crate::io::{BinReader, BinWriter};

// Headers payload.
pub struct Headers {
    pub hdrs: Vec<Header>,
    // StateRootInHeader specifies whether the header contains a state root.
    pub state_root_in_header: bool,
}

// Users can at most request 2k headers.
const MAX_HEADERS_ALLOWED: u64 = 2000;

// ErrTooManyHeaders is an error returned when too many headers have been received.
#[derive(Debug)]
pub struct ErrTooManyHeaders;

impl fmt::Display for ErrTooManyHeaders {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "too many headers were received (max: {})", MAX_HEADERS_ALLOWED)
    }
}

impl Error for ErrTooManyHeaders {}

// ErrNoHeaders is returned for zero-elements Headers payload which is considered to be invalid.
#[derive(Debug)]
pub struct ErrNoHeaders;

impl fmt::Display for ErrNoHeaders {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "no headers (zero length array)")
    }
}

impl Error for ErrNoHeaders {}

// DecodeBinary implements the Serializable interface.
impl Headers {
    pub fn decode_binary(&mut self, br: &mut BinReader) -> Result<(), Box<dyn Error>> {
        let len_headers = br.read_var_uint()?;

        if len_headers == 0 {
            return Err(Box::new(ErrNoHeaders));
        }

        let mut limit_exceeded = false;

        // C# node does it silently
        if len_headers > MAX_HEADERS_ALLOWED {
            limit_exceeded = true;
        }

        let len_headers = if limit_exceeded {
            MAX_HEADERS_ALLOWED
        } else {
            len_headers
        };

        self.hdrs = Vec::with_capacity(len_headers as usize);

        for _ in 0..len_headers {
            let mut header = Header::default();
            header.state_root_enabled = self.state_root_in_header;
            header.decode_binary(br)?;
            self.hdrs.push(header);
        }

        if limit_exceeded {
            return Err(Box::new(ErrTooManyHeaders));
        }

        Ok(())
    }

    // EncodeBinary implements the Serializable interface.
    pub fn encode_binary(&self, bw: &mut BinWriter) -> Result<(), Box<dyn Error>> {
        bw.write_array(&self.hdrs)?;
        Ok(())
    }
}
