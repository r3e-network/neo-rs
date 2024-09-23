use std::fs;
use std::collections::HashMap;
use std::error::Error;
use hex;
use serde_json::Value;
use crate::testserdes;
use crate::block::Block;

fn get_decoded_block(i: usize) -> Result<Block, Box<dyn Error>> {
    let data = get_block_data(i)?;

    let b = hex::decode(data["raw"].as_str().unwrap())?;
    
    let mut block = Block::new(false);
    testserdes::decode_binary(&b, &mut block)?;

    Ok(block)
}

fn get_block_data(i: usize) -> Result<HashMap<String, Value>, Box<dyn Error>> {
    let b = fs::read_to_string(format!("../test_data/block_{}.json", i))?;
    let data: HashMap<String, Value> = serde_json::from_str(&b)?;
    Ok(data)
}
