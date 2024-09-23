use serde::{Deserialize, Serialize};
use serde_json::{self, Error};
use std::error::Error as StdError;

#[derive(Serialize, Deserialize)]
pub struct Block {
    #[serde(flatten)]
    block: block::Block,
    #[serde(flatten)]
    metadata: BlockMetadata,
}

#[derive(Serialize, Deserialize)]
pub struct BlockMetadata {
    size: i32,
    next_block_hash: Option<util::Uint256>,
    confirmations: u32,
}

impl Block {
    // Custom serialization to match the C# API
    pub fn to_json(&self) -> Result<String, Box<dyn StdError>> {
        let metadata_json = serde_json::to_string(&self.metadata)?;
        let block_json = serde_json::to_string(&self.block)?;

        if metadata_json.ends_with('}') && block_json.starts_with('{') {
            let mut combined_json = metadata_json;
            combined_json.pop(); // Remove the closing '}'
            combined_json.push(',');
            combined_json.push_str(&block_json[1..]); // Skip the opening '{' of block_json
            Ok(combined_json)
        } else {
            Err("can't merge internal jsons".into())
        }
    }

    // Custom deserialization to match the C# API
    pub fn from_json(data: &str) -> Result<Self, Box<dyn StdError>> {
        let metadata: BlockMetadata = serde_json::from_str(data)?;
        let block: block::Block = serde_json::from_str(data)?;

        Ok(Block {
            block,
            metadata,
        })
    }
}
