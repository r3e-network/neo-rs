use std::convert::TryInto;
use std::io::{Read, Write};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

use crate::config;
use crate::crypto::hash;
use crate::io::{BinReader, BinWriter};
use crate::vm::stackitem;

// NEO Executable Format 3 (NEF3)
// Standard: https://github.com/neo-project/proposals/pull/121/files
// Implementation: https://github.com/neo-project/neo/blob/v3.0.0-preview2/src/neo/SmartContract/NefFile.cs#L8
// +------------+-----------+------------------------------------------------------------+
// |   Field    |  Length   |                          Comment                           |
// +------------+-----------+------------------------------------------------------------+
// | Magic      | 4 bytes   | Magic header                                               |
// | Compiler   | 64 bytes  | Compiler used and it's version                             |
// | Source     | Var bytes | Source file URL.                                           |
// +------------+-----------+------------------------------------------------------------+
// | Reserved   | 1 byte    | Reserved for extensions. Must be 0.                        |
// | Tokens     | Var array | List of method tokens                                      |
// | Reserved   | 2-bytes   | Reserved for extensions. Must be 0.                        |
// | Script     | Var bytes | Var bytes for the payload                                  |
// +------------+-----------+------------------------------------------------------------+
// | Checksum   | 4 bytes   | First four bytes of double SHA256 hash of the header       |
// +------------+-----------+------------------------------------------------------------+

const MAGIC: u32 = 0x3346454E;
const MAX_SOURCE_URL_LENGTH: usize = 256;
const COMPILER_FIELD_SIZE: usize = 64;

#[derive(Debug, Clone)]
pub struct File {
    pub header: Header,
    pub source: String,
    pub tokens: Vec<MethodToken>,
    pub script: Vec<u8>,
    pub checksum: u32,
}

#[derive(Debug, Clone)]
pub struct Header {
    pub magic: u32,
    pub compiler: String,
}

impl File {
    pub fn new(script: Vec<u8>) -> Result<Self, Box<dyn std::error::Error>> {
        let mut file = File {
            header: Header {
                magic: MAGIC,
                compiler: format!("neo-go-{}", config::VERSION),
            },
            source: String::new(),
            tokens: Vec::new(),
            script,
            checksum: 0,
        };

        if file.header.compiler.len() > COMPILER_FIELD_SIZE {
            return Err("Too long compiler field".into());
        }

        file.checksum = file.calculate_checksum()?;
        Ok(file)
    }

    pub fn calculate_checksum(&self) -> Result<u32, Box<dyn std::error::Error>> {
        let bytes = self.bytes_long()?;
        let hash = hash::checksum(&bytes[..bytes.len() - 4]);
        Ok(u32::from_le_bytes(hash[..4].try_into()?))
    }

    pub fn bytes(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        self.bytes_impl(true)
    }

    pub fn bytes_long(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        self.bytes_impl(false)
    }

    fn bytes_impl(&self, check_size: bool) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut writer = BinWriter::new();
        self.encode_binary(&mut writer)?;

        let res = writer.into_vec();
        if check_size && res.len() > stackitem::MAX_SIZE {
            return Err(format!("Serialized NEF size exceeds VM stackitem limits: {} bytes is allowed at max, got {}", stackitem::MAX_SIZE, res.len()).into());
        }
        Ok(res)
    }

    pub fn from_bytes(source: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        if source.len() > stackitem::MAX_SIZE {
            return Err(format!("Invalid NEF file size: expected {} at max, got {}", stackitem::MAX_SIZE, source.len()).into());
        }

        let mut reader = BinReader::new(source);
        let mut result = File {
            header: Header { magic: 0, compiler: String::new() },
            source: String::new(),
            tokens: Vec::new(),
            script: Vec::new(),
            checksum: 0,
        };
        result.decode_binary(&mut reader)?;
        Ok(result)
    }
}

impl BinWriter for File {
    fn encode_binary<W: Write>(&self, writer: &mut W) -> Result<(), Box<dyn std::error::Error>> {
        self.header.encode_binary(writer)?;
        
        if self.source.len() > MAX_SOURCE_URL_LENGTH {
            return Err("Source URL too long".into());
        }
        writer.write_all(self.source.as_bytes())?;
        
        writer.write_u8(0)?;
        
        writer.write_u32::<LittleEndian>(self.tokens.len() as u32)?;
        for token in &self.tokens {
            token.encode_binary(writer)?;
        }
        
        writer.write_u16::<LittleEndian>(0)?;
        
        writer.write_u32::<LittleEndian>(self.script.len() as u32)?;
        writer.write_all(&self.script)?;
        
        writer.write_u32::<LittleEndian>(self.checksum)?;
        
        Ok(())
    }
}

impl BinReader for File {
    fn decode_binary<R: Read>(&mut self, reader: &mut R) -> Result<(), Box<dyn std::error::Error>> {
        self.header.decode_binary(reader)?;
        
        let mut source = vec![0u8; MAX_SOURCE_URL_LENGTH];
        reader.read_exact(&mut source)?;
        self.source = String::from_utf8(source.into_iter().take_while(|&x| x != 0).collect())?;
        
        let reserved_b = reader.read_u8()?;
        if reserved_b != 0 {
            return Err("Reserved byte must be 0".into());
        }
        
        let token_count = reader.read_u32::<LittleEndian>()?;
        self.tokens = Vec::with_capacity(token_count as usize);
        for _ in 0..token_count {
            let mut token = MethodToken::default();
            token.decode_binary(reader)?;
            self.tokens.push(token);
        }
        
        let reserved = reader.read_u16::<LittleEndian>()?;
        if reserved != 0 {
            return Err("Reserved bytes must be 0".into());
        }
        
        let script_len = reader.read_u32::<LittleEndian>()? as usize;
        if script_len > stackitem::MAX_SIZE {
            return Err("Script too long".into());
        }
        self.script = vec![0u8; script_len];
        reader.read_exact(&mut self.script)?;
        
        if self.script.is_empty() {
            return Err("Empty script".into());
        }
        
        self.checksum = reader.read_u32::<LittleEndian>()?;
        let calculated_checksum = self.calculate_checksum()?;
        if calculated_checksum != self.checksum {
            return Err("Checksum verification failure".into());
        }
        
        Ok(())
    }
}

impl BinWriter for Header {
    fn encode_binary<W: Write>(&self, writer: &mut W) -> Result<(), Box<dyn std::error::Error>> {
        writer.write_u32::<LittleEndian>(self.magic)?;
        
        if self.compiler.len() > COMPILER_FIELD_SIZE {
            return Err("Invalid compiler name length".into());
        }
        
        let mut compiler_bytes = [0u8; COMPILER_FIELD_SIZE];
        compiler_bytes[..self.compiler.len()].copy_from_slice(self.compiler.as_bytes());
        writer.write_all(&compiler_bytes)?;
        
        Ok(())
    }
}

impl BinReader for Header {
    fn decode_binary<R: Read>(&mut self, reader: &mut R) -> Result<(), Box<dyn std::error::Error>> {
        self.magic = reader.read_u32::<LittleEndian>()?;
        if self.magic != MAGIC {
            return Err("Invalid Magic".into());
        }
        
        let mut compiler_bytes = [0u8; COMPILER_FIELD_SIZE];
        reader.read_exact(&mut compiler_bytes)?;
        self.compiler = String::from_utf8(compiler_bytes.into_iter().take_while(|&x| x != 0).collect())?;
        
        Ok(())
    }
}

// Note: MethodToken struct and its implementations are not provided in the original code snippet.
// You would need to implement it separately based on the actual definition and requirements.
#[derive(Debug, Clone, Default)]
pub struct MethodToken {
    // Define fields as needed
}

impl BinWriter for MethodToken {
    fn encode_binary<W: Write>(&self, _writer: &mut W) -> Result<(), Box<dyn std::error::Error>> {
        // Implement encoding logic
        Ok(())
    }
}

impl BinReader for MethodToken {
    fn decode_binary<R: Read>(&mut self, _reader: &mut R) -> Result<(), Box<dyn std::error::Error>> {
        // Implement decoding logic
        Ok(())
    }
}
