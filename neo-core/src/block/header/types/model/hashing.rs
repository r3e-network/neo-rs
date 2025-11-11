use alloc::vec::Vec;

use neo_base::{
    encoding::{NeoEncode, NeoWrite},
    hash::double_sha256,
};

use crate::{h256::H256, io};

use super::Header;

impl Header {
    fn encoded_version(&self) -> u32 {
        let mut version = self.version & !super::STATE_ROOT_FLAG;
        if self.state_root_enabled {
            version |= super::STATE_ROOT_FLAG;
        }
        version
    }

    fn encode_unsigned<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_u32(self.encoded_version());
        self.prev_hash.neo_encode(writer);
        self.merkle_root.neo_encode(writer);
        writer.write_u64(self.unix_milli);
        writer.write_u64(self.nonce);
        writer.write_u32(self.index);
        writer.write_u8(self.primary);
        self.next_consensus.neo_encode(writer);
        if self.state_root_enabled {
            self.prev_state_root.unwrap_or_default().neo_encode(writer);
        }
    }

    fn unsigned_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(4 + 32 * 3 + 8 * 2 + 4 + 1 + 20 + 32);
        self.encode_unsigned(&mut buf);
        buf
    }

    pub fn calculate_hash(&self) -> H256 {
        H256::from_le_bytes(double_sha256(self.unsigned_bytes()))
    }

    pub fn ensure_hash(&mut self) -> H256 {
        if let Some(hash) = self.hash {
            hash
        } else {
            let computed = self.calculate_hash();
            self.hash = Some(computed);
            computed
        }
    }
}

impl NeoEncode for Header {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.encode_unsigned(writer);
        io::write_array(writer, &self.witnesses);
    }
}
