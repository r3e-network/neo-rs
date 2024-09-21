// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use neo_base::encoding::bin::{BinDecoder, RefBuffer};
use neo_base::encoding::hex::ToHex;
use neo_core::contract::Nef3;

use crate::*;


#[derive(clap::Args)]
pub(crate) struct Nef3Cmd {
    #[arg(long, help = "The NEF file path")]
    pub file: String,
}


pub fn parse_nef3_file(file: &str) -> anyhow::Result<()> {
    let mut file = File::open(file)?;
    let mut content = Vec::new();
    let _ = file.read_to_end(&mut content)?;

    let mut buf = RefBuffer::from(content.as_slice());
    let nef: Nef3 = BinDecoder::decode_bin(&mut buf)?;

    std::println!("Valid: {}", nef.is_valid().is_ok());
    std::println!("NEF3 Magic: {:x}", nef.magic);
    std::println!("NEF3 Magic: {}", String::from_utf8_lossy(nef.compiler.as_bytes()));
    std::println!("NEF3 Source: {}", nef.source);
    std::println!("NEF3 Script: {}", nef.script.to_hex());
    std::println!("NEF3 Methods: {:?}", &nef.tokens);
    std::println!("NEF3 Checksum: {:x}", nef.checksum);
    Ok(())
}