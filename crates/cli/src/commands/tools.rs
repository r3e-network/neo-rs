use super::CommandResult;
use crate::console_service::ConsoleHelper;
use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use hex;
use neo_core::{
    cryptography::crypto_utils::{ECCurve, ECPoint},
    neo_vm::{op_code::OpCode, Script},
    protocol_settings::ProtocolSettings,
    smart_contract::{contract::Contract, contract_state::NefFile},
    wallets::{helper::Helper as WalletHelper, KeyPair},
    UInt160,
};
use num_bigint::BigInt;
use std::{fmt::Write as _, fs, path::Path, str::FromStr, sync::Arc};

type ParseHandler = fn(&ToolCommands, &str) -> Result<Option<String>>;

struct ParseEntry {
    description: &'static str,
    handler: ParseHandler,
}

/// Miscellaneous utilities (`MainService.Tools`).
pub struct ToolCommands {
    settings: Arc<ProtocolSettings>,
    parse_entries: Vec<ParseEntry>,
}

impl ToolCommands {
    pub fn new(settings: Arc<ProtocolSettings>) -> Self {
        let parse_entries = vec![
            ParseEntry {
                description: ".nef file path to content base64",
                handler: ToolCommands::parse_nef_path_to_base64,
            },
            ParseEntry {
                description: "Little-endian to Big-endian",
                handler: ToolCommands::parse_little_to_big,
            },
            ParseEntry {
                description: "Big-endian to Little-endian",
                handler: ToolCommands::parse_big_to_little,
            },
            ParseEntry {
                description: "String to Base64",
                handler: ToolCommands::parse_string_to_base64,
            },
            ParseEntry {
                description: "Big Integer to Base64",
                handler: ToolCommands::parse_bigint_to_base64,
            },
            ParseEntry {
                description: "Address to ScriptHash (big-endian)",
                handler: ToolCommands::parse_address_to_script_hash_be,
            },
            ParseEntry {
                description: "Address to ScriptHash (little-endian)",
                handler: ToolCommands::parse_address_to_script_hash_le,
            },
            ParseEntry {
                description: "Address to Base64",
                handler: ToolCommands::parse_address_to_base64,
            },
            ParseEntry {
                description: "ScriptHash to Address",
                handler: ToolCommands::parse_script_hash_to_address,
            },
            ParseEntry {
                description: "Base64 to Address",
                handler: ToolCommands::parse_base64_to_address,
            },
            ParseEntry {
                description: "Base64 to String",
                handler: ToolCommands::parse_base64_to_string,
            },
            ParseEntry {
                description: "Base64 to Big Integer",
                handler: ToolCommands::parse_base64_to_bigint,
            },
            ParseEntry {
                description: "Public Key to Address",
                handler: ToolCommands::parse_public_key_to_address,
            },
            ParseEntry {
                description: "WIF to Public Key",
                handler: ToolCommands::parse_wif_to_public_key,
            },
            ParseEntry {
                description: "WIF to Address",
                handler: ToolCommands::parse_wif_to_address,
            },
            ParseEntry {
                description: "Base64 Smart Contract Script Analysis",
                handler: ToolCommands::parse_disassemble_base64_script,
            },
            ParseEntry {
                description: "Base64 .nef file Analysis",
                handler: ToolCommands::parse_nef_analysis,
            },
        ];

        Self {
            settings,
            parse_entries,
        }
    }

    /// Disassembles a script from a base64 string or filesystem path (mirrors `ScriptsToOpCode`).
    pub fn analyze_script(&self, input: &str) -> CommandResult {
        let bytes = self.read_script_bytes(input)?;
        let output = self.disassemble_script_bytes(&bytes)?;
        ConsoleHelper::info(["", output.as_str()]);
        Ok(())
    }

    /// General parse command (mirrors `parse` in the C# CLI).
    pub fn parse_value(&self, input: &str) -> CommandResult {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(anyhow!("parse input cannot be empty"));
        }

        let mut any = false;
        for entry in &self.parse_entries {
            match (entry.handler)(self, trimmed) {
                Ok(Some(result)) => {
                    any = true;
                    ConsoleHelper::info(["", "-----", entry.description, "-----"]);
                    ConsoleHelper::info(["", result.as_str(), ""]);
                }
                Ok(None) => {}
                Err(err) => {
                    ConsoleHelper::warning(format!(
                        "{} conversion failed: {}",
                        entry.description, err
                    ));
                }
            }
        }

        if !any {
            ConsoleHelper::warning("Was not possible to convert input.");
        }

        Ok(())
    }

    fn parse_nef_path_to_base64(&self, input: &str) -> Result<Option<String>> {
        let path = Path::new(input);
        if !path.exists() {
            return Ok(None);
        }

        if path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("nef"))
            != Some(true)
        {
            return Ok(None);
        }

        let bytes = fs::read(path)
            .with_context(|| format!("failed to read NEF file {}", path.display()))?;
        Ok(Some(BASE64_STANDARD.encode(bytes)))
    }

    fn parse_little_to_big(&self, input: &str) -> Result<Option<String>> {
        let cleaned = input.trim().strip_prefix("0x").unwrap_or(input.trim());
        if cleaned.is_empty() || cleaned.len() % 2 != 0 {
            return Ok(None);
        }
        let mut bytes = match hex::decode(cleaned) {
            Ok(bytes) => bytes,
            Err(_) => return Ok(None),
        };
        bytes.reverse();
        Ok(Some(format!("0x{}", hex::encode(bytes))))
    }

    fn parse_big_to_little(&self, input: &str) -> Result<Option<String>> {
        let trimmed = input.trim();
        if !trimmed.starts_with("0x") {
            return Ok(None);
        }
        let cleaned = &trimmed[2..];
        if cleaned.len() % 2 != 0 {
            return Ok(None);
        }
        let mut bytes = match hex::decode(cleaned) {
            Ok(bytes) => bytes,
            Err(_) => return Ok(None),
        };
        bytes.reverse();
        Ok(Some(hex::encode(bytes)))
    }

    fn parse_string_to_base64(&self, input: &str) -> Result<Option<String>> {
        Ok(Some(BASE64_STANDARD.encode(input.as_bytes())))
    }

    fn parse_bigint_to_base64(&self, input: &str) -> Result<Option<String>> {
        let value = BigInt::from_str(input).map_err(|err| anyhow!(err))?;
        Ok(Some(BASE64_STANDARD.encode(value.to_signed_bytes_le())))
    }

    fn parse_address_to_script_hash_be(&self, input: &str) -> Result<Option<String>> {
        let script = self.to_script_hash(input)?;
        Ok(Some(format!("0x{}", script.to_string())))
    }

    fn parse_address_to_script_hash_le(&self, input: &str) -> Result<Option<String>> {
        let script = self.to_script_hash(input)?;
        Ok(Some(hex::encode(script.to_array())))
    }

    fn parse_address_to_base64(&self, input: &str) -> Result<Option<String>> {
        let script = self.to_script_hash(input)?;
        Ok(Some(BASE64_STANDARD.encode(script.to_array())))
    }

    fn parse_script_hash_to_address(&self, input: &str) -> Result<Option<String>> {
        let trimmed = input.trim();
        let script_hash = if trimmed.starts_with("0x") {
            UInt160::from_str(trimmed).map_err(|err| anyhow!(err))?
        } else {
            let bytes = match hex::decode(trimmed) {
                Ok(bytes) => bytes,
                Err(_) => return Ok(None),
            };
            if bytes.len() != UInt160::LENGTH {
                return Ok(None);
            }
            let mut reversed = bytes.clone();
            reversed.reverse();
            let big_hex = format!("0x{}", hex::encode(reversed));
            UInt160::from_str(&big_hex).map_err(|err| anyhow!(err))?
        };

        Ok(Some(WalletHelper::to_address(
            &script_hash,
            self.settings.address_version,
        )))
    }

    fn parse_base64_to_address(&self, input: &str) -> Result<Option<String>> {
        let mut bytes = match BASE64_STANDARD.decode(input.trim()) {
            Ok(bytes) => bytes,
            Err(_) => return Ok(None),
        };

        if bytes.len() != UInt160::LENGTH {
            return Ok(None);
        }

        bytes.reverse();
        let hex = hex::encode(bytes);
        let script_hash = UInt160::from_str(&format!("0x{}", hex)).map_err(|err| anyhow!(err))?;
        Ok(Some(WalletHelper::to_address(
            &script_hash,
            self.settings.address_version,
        )))
    }

    fn parse_base64_to_string(&self, input: &str) -> Result<Option<String>> {
        let bytes = match BASE64_STANDARD.decode(input.trim()) {
            Ok(bytes) => bytes,
            Err(_) => return Ok(None),
        };
        let value = match String::from_utf8(bytes) {
            Ok(value) => value,
            Err(_) => return Ok(None),
        };
        if is_printable(&value) {
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    fn parse_base64_to_bigint(&self, input: &str) -> Result<Option<String>> {
        let bytes = match BASE64_STANDARD.decode(input.trim()) {
            Ok(bytes) => bytes,
            Err(_) => return Ok(None),
        };
        let number = BigInt::from_signed_bytes_le(&bytes);
        Ok(Some(number.to_string()))
    }

    fn parse_public_key_to_address(&self, input: &str) -> Result<Option<String>> {
        let point = match Self::decode_public_key(input.trim()) {
            Some(point) => point,
            None => return Ok(None),
        };
        let contract = Contract::create_signature_contract(point);
        Ok(Some(WalletHelper::to_address(
            &contract.script_hash(),
            self.settings.address_version,
        )))
    }

    fn parse_wif_to_public_key(&self, input: &str) -> Result<Option<String>> {
        match KeyPair::from_wif(input.trim()) {
            Ok(key) => Ok(Some(hex::encode(key.public_key()))),
            Err(_) => Ok(None),
        }
    }

    fn parse_wif_to_address(&self, input: &str) -> Result<Option<String>> {
        let key = match KeyPair::from_wif(input.trim()) {
            Ok(key) => key,
            Err(_) => return Ok(None),
        };
        let contract = Contract::create_signature_contract(
            key.get_public_key_point()
                .map_err(|err| anyhow!(err.to_string()))?,
        );
        Ok(Some(WalletHelper::to_address(
            &contract.script_hash(),
            self.settings.address_version,
        )))
    }

    fn parse_disassemble_base64_script(&self, input: &str) -> Result<Option<String>> {
        let bytes = match BASE64_STANDARD.decode(input.trim()) {
            Ok(bytes) => bytes,
            Err(_) => return Ok(None),
        };
        self.disassemble_script_bytes(&bytes).map(Some)
    }

    fn parse_nef_analysis(&self, input: &str) -> Result<Option<String>> {
        let nef_bytes = if Path::new(input).exists() {
            fs::read(input).with_context(|| format!("failed to read NEF file {}", input))?
        } else {
            match BASE64_STANDARD.decode(input.trim()) {
                Ok(bytes) => bytes,
                Err(_) => return Ok(None),
            }
        };

        let nef = match NefFile::parse(&nef_bytes) {
            Ok(nef) => nef,
            Err(_) => return Ok(None),
        };

        let mut strict_mode = true;
        if Script::new(nef.script.clone(), true).is_err() {
            strict_mode = false;
            Script::new(nef.script.clone(), false).map_err(|err| anyhow!(err))?;
        }

        let disassembly = self.disassemble_script_bytes(&nef.script)?;
        let mut prefix = format!("\n# Compiler: {}", nef.compiler);
        if !strict_mode {
            prefix.push_str("\n# Warning: Failed strict mode validation");
        }

        Ok(Some(format!("{prefix}\n{disassembly}")))
    }

    fn read_script_bytes(&self, input: &str) -> Result<Vec<u8>> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(anyhow!("script input cannot be empty"));
        }

        let path = Path::new(trimmed);
        if path.exists() {
            return fs::read(path)
                .with_context(|| format!("failed to read script file {}", path.display()));
        }

        BASE64_STANDARD
            .decode(trimmed)
            .map_err(|err| anyhow!("failed to decode base64 input: {err}"))
    }

    fn disassemble_script_bytes(&self, bytes: &[u8]) -> Result<String> {
        let mut offset = 0usize;
        let mut line = 0usize;
        let mut output = String::new();

        while offset < bytes.len() {
            let instruction = VmInstruction::parse(bytes, offset)
                .with_context(|| format!("failed to decode instruction at offset {offset}"))?;
            if instruction.operand_size() == 0 {
                writeln!(
                    &mut output,
                    "L{line:04}:{pos:04X} {opcode:?}",
                    pos = instruction.position(),
                    opcode = instruction.opcode()
                )
                .ok();
            } else {
                writeln!(
                    &mut output,
                    "L{line:04}:{pos:04X} {opcode:<10}{operand}",
                    pos = instruction.position(),
                    opcode = format!("{:?}", instruction.opcode()),
                    operand = instruction.decode_operand()
                )
                .ok();
            }

            offset = instruction.next_offset();
            line += 1;
        }
        Ok(output)
    }

    fn to_script_hash(&self, address: &str) -> Result<UInt160> {
        WalletHelper::to_script_hash(address, self.settings.address_version)
            .map_err(|err| anyhow!(err))
    }

    fn decode_public_key(input: &str) -> Option<ECPoint> {
        let cleaned = input.trim().trim_start_matches("0x");
        let bytes = hex::decode(cleaned).ok()?;
        if bytes.len() == 33 {
            ECPoint::decode_compressed(&bytes).ok()
        } else {
            ECPoint::decode(&bytes, ECCurve::secp256r1()).ok()
        }
    }
}

fn is_printable(value: &str) -> bool {
    !value.is_empty() && value.chars().any(|c| !c.is_control())
}

#[derive(Debug, Clone)]
struct VmInstruction<'a> {
    position: usize,
    opcode: OpCode,
    operand: &'a [u8],
    operand_prefix_size: usize,
}

impl<'a> VmInstruction<'a> {
    fn parse(script: &'a [u8], start: usize) -> Result<Self> {
        if start >= script.len() {
            return Err(anyhow!("script position {start} is out of bounds"));
        }

        let opcode = OpCode::try_from(script[start])
            .map_err(|_| anyhow!("invalid opcode at position {start}"))?;

        let operand_size_info = opcode.operand_size();
        let prefix_size = operand_size_info.size_prefix.max(0) as usize;

        let payload_len = if prefix_size == 0 {
            operand_size_info.size.max(0) as usize
        } else {
            let prefix_start = start
                .checked_add(1)
                .ok_or_else(|| anyhow!("operand prefix overflowed bounds"))?;
            let prefix_end = prefix_start
                .checked_add(prefix_size)
                .ok_or_else(|| anyhow!("operand prefix overflowed bounds"))?;
            if prefix_end > script.len() {
                return Err(anyhow!(
                    "operand prefix at offset {} exceeds script length",
                    prefix_start
                ));
            }
            match prefix_size {
                1 => script[prefix_start] as usize,
                2 => {
                    let mut buffer = [0u8; 2];
                    buffer.copy_from_slice(&script[prefix_start..prefix_end]);
                    u16::from_le_bytes(buffer) as usize
                }
                4 => {
                    let mut buffer = [0u8; 4];
                    buffer.copy_from_slice(&script[prefix_start..prefix_end]);
                    u32::from_le_bytes(buffer) as usize
                }
                _ => {
                    return Err(anyhow!(
                        "unsupported operand prefix size {} at offset {}",
                        prefix_size,
                        prefix_start
                    ))
                }
            }
        };

        let operand_len = prefix_size
            .checked_add(payload_len)
            .ok_or_else(|| anyhow!("operand size overflowed bounds"))?;

        let operand_start = start
            .checked_add(1)
            .ok_or_else(|| anyhow!("operand start overflowed bounds"))?;
        let operand_end = operand_start
            .checked_add(operand_len)
            .ok_or_else(|| anyhow!("operand end overflowed bounds"))?;

        if operand_end > script.len() {
            return Err(anyhow!(
                "operand at position {} exceeds script length",
                start
            ));
        }

        Ok(Self {
            position: start,
            opcode,
            operand: &script[operand_start..operand_end],
            operand_prefix_size: prefix_size.min(operand_len),
        })
    }

    fn position(&self) -> usize {
        self.position
    }

    fn opcode(&self) -> OpCode {
        self.opcode
    }

    fn operand_size(&self) -> usize {
        self.operand.len()
    }

    fn next_offset(&self) -> usize {
        self.position
            .saturating_add(1)
            .saturating_add(self.operand.len())
    }

    fn decode_operand(&self) -> String {
        let data = self.operand_data();
        let readable_ascii = data
            .iter()
            .all(|ch| ch.is_ascii_alphanumeric() || ch.is_ascii_whitespace());

        match self.opcode {
            OpCode::JMP
            | OpCode::JMPIF
            | OpCode::JMPIFNOT
            | OpCode::JMPEQ
            | OpCode::JMPNE
            | OpCode::JMPGT
            | OpCode::JMPLT
            | OpCode::CALL
            | OpCode::ENDTRY => format!("[{:08X}]", self.position + self.token_u8(0) as usize),
            OpCode::JMP_L
            | OpCode::JMPIF_L
            | OpCode::JMPIFNOT_L
            | OpCode::JMPEQ_L
            | OpCode::JMPNE_L
            | OpCode::JMPGT_L
            | OpCode::JMPLT_L
            | OpCode::CALL_L
            | OpCode::ENDTRY_L => {
                format!(
                    "[{:08X}]",
                    self.position as isize + self.token_i32(0) as isize
                )
            }
            OpCode::TRY => format!("[{:02X}, {:02X}]", self.token_u8(0), self.token_u8(1)),
            OpCode::INITSLOT => format!("{}, {}", self.token_u8(0), self.token_u8(1)),
            OpCode::TRY_L => format!(
                "[{:08X}, {:08X}]",
                self.position as isize + self.token_i32(0) as isize,
                self.position as isize + self.token_i32(4) as isize
            ),
            OpCode::CALLT => format!("[{:08X}]", self.token_u16(0)),
            OpCode::NEWARRAY_T | OpCode::ISTYPE | OpCode::CONVERT => {
                format!("{:02X}", self.token_u8(0))
            }
            OpCode::STLOC
            | OpCode::LDLOC
            | OpCode::LDSFLD
            | OpCode::STSFLD
            | OpCode::LDARG
            | OpCode::STARG
            | OpCode::INITSSLOT => format!("{}", self.token_u8(0)),
            OpCode::PUSHINT8 => format!("{}", self.token_i8(0)),
            OpCode::PUSHINT16 => format!("{}", self.token_i16(0)),
            OpCode::PUSHINT32 => format!("{}", self.token_i32(0)),
            OpCode::PUSHINT64 => format!("{}", self.token_i64(0)),
            OpCode::PUSHINT128 | OpCode::PUSHINT256 => {
                format!("{}", BigInt::from_signed_bytes_le(data))
            }
            OpCode::SYSCALL => format!("[0x{:08X}]", self.token_u32(0)),
            OpCode::PUSHDATA1 | OpCode::PUSHDATA2 | OpCode::PUSHDATA4 => {
                if readable_ascii {
                    format!(
                        "{} // {}",
                        hex::encode_upper(data),
                        String::from_utf8_lossy(data)
                    )
                } else {
                    hex::encode_upper(data)
                }
            }
            _ => {
                if readable_ascii {
                    format!("\"{}\"", String::from_utf8_lossy(data).trim())
                } else if data.is_empty() {
                    String::new()
                } else {
                    hex::encode_upper(data)
                }
            }
        }
    }

    fn operand_data(&self) -> &'a [u8] {
        if self.operand_prefix_size >= self.operand.len() {
            return &[];
        }
        &self.operand[self.operand_prefix_size..]
    }

    fn token_u8(&self, index: usize) -> u8 {
        *self.operand_data().get(index).unwrap_or(&0u8)
    }

    fn token_i8(&self, index: usize) -> i8 {
        self.token_u8(index) as i8
    }

    fn token_u16(&self, index: usize) -> u16 {
        let data = self.operand_data();
        if data.len() < index + 2 {
            return 0;
        }
        u16::from_le_bytes([data[index], data[index + 1]])
    }

    fn token_i16(&self, index: usize) -> i16 {
        self.token_u16(index) as i16
    }

    fn token_i32(&self, index: usize) -> i32 {
        let data = self.operand_data();
        if data.len() < index + 4 {
            return 0;
        }
        let mut buffer = [0u8; 4];
        buffer.copy_from_slice(&data[index..index + 4]);
        i32::from_le_bytes(buffer)
    }

    fn token_u32(&self, index: usize) -> u32 {
        self.token_i32(index) as u32
    }

    fn token_i64(&self, index: usize) -> i64 {
        let data = self.operand_data();
        if data.len() < index + 8 {
            return 0;
        }
        let mut buffer = [0u8; 8];
        buffer.copy_from_slice(&data[index..index + 8]);
        i64::from_le_bytes(buffer)
    }
}
