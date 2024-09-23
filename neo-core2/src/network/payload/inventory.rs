use crate::io::{BinReader, BinWriter};
use crate::util::Uint256;

// A node can broadcast the object information it owns by this message.
// The message can be sent automatically or can be used to answer getblock messages.

// InventoryType is the type of an object in the Inventory message.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum InventoryType {
    TXType = 0x2b,
    BlockType = 0x2c,
    ExtensibleType = 0x2e,
    P2PNotaryRequestType = 0x50,
}

impl InventoryType {
    // String implements the Stringer interface.
    pub fn to_string(&self) -> &str {
        match self {
            InventoryType::TXType => "TX",
            InventoryType::BlockType => "block",
            InventoryType::ExtensibleType => "extensible",
            InventoryType::P2PNotaryRequestType => "p2pNotaryRequest",
            _ => "unknown inventory type",
        }
    }

    // Valid returns true if the inventory (type) is known.
    pub fn valid(&self, p2p_sig_extensions_enabled: bool) -> bool {
        *self == InventoryType::BlockType
            || *self == InventoryType::TXType
            || *self == InventoryType::ExtensibleType
            || (p2p_sig_extensions_enabled && *self == InventoryType::P2PNotaryRequestType)
    }
}

// Inventory payload.
#[derive(Debug, Clone)]
pub struct Inventory {
    // Type of the object hash.
    pub inv_type: InventoryType,

    // A list of hashes.
    pub hashes: Vec<Uint256>,
}

impl Inventory {
    // NewInventory returns a pointer to an Inventory.
    pub fn new(inv_type: InventoryType, hashes: Vec<Uint256>) -> Self {
        Inventory {
            inv_type,
            hashes,
        }
    }

    // DecodeBinary implements the Serializable interface.
    pub fn decode_binary(&mut self, br: &mut BinReader) {
        self.inv_type = InventoryType::from(br.read_u8());
        br.read_array(&mut self.hashes, MaxHashesCount);
    }

    // EncodeBinary implements the Serializable interface.
    pub fn encode_binary(&self, bw: &mut BinWriter) {
        bw.write_u8(self.inv_type as u8);
        bw.write_array(&self.hashes);
    }
}

impl From<u8> for InventoryType {
    fn from(value: u8) -> Self {
        match value {
            0x2b => InventoryType::TXType,
            0x2c => InventoryType::BlockType,
            0x2e => InventoryType::ExtensibleType,
            0x50 => InventoryType::P2PNotaryRequestType,
            _ => panic!("Unknown inventory type"),
        }
    }
}
