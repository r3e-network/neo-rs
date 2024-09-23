use serde::{Deserialize, Serialize};
use serde_json::{self, Error};
use std::error;

use crate::core::block::Header as BlockHeader;

#[derive(Serialize, Deserialize)]
pub struct Header {
    #[serde(flatten)]
    pub block_header: BlockHeader,
    #[serde(flatten)]
    pub block_metadata: BlockMetadata,
}

impl Header {
    // Serialize the Header struct to JSON
    pub fn to_json(&self) -> Result<String, Box<dyn error::Error>> {
        let metadata_json = serde_json::to_string(&self.block_metadata)?;
        let header_json = serde_json::to_string(&self.block_header)?;

        // We have to keep both "fields" at the same level in json in order to
        // match C# API, so there's no way to marshall Block correctly with
        // standard json.Marshaller tool.
        if !metadata_json.ends_with('}') || !header_json.starts_with('{') {
            return Err("can't merge internal jsons".into());
        }

        let mut output = metadata_json;
        output.pop(); // Remove the closing brace
        output.push(',');
        output.push_str(&header_json[1..]); // Append the header JSON without the opening brace

        Ok(output)
    }

    // Deserialize the Header struct from JSON
    pub fn from_json(data: &str) -> Result<Self, Error> {
        let mut header: Header = serde_json::from_str(data)?;
        let metadata: BlockMetadata = serde_json::from_str(data)?;
        header.block_metadata = metadata;
        Ok(header)
    }
}
