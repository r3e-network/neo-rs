use neo_base::encoding::{NeoEncode, NeoWrite};

use crate::io;

use super::super::Tx;

impl Tx {
    pub(crate) fn encode_unsigned<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_u8(self.version);
        writer.write_u32(self.nonce);
        writer.write_u32(self.valid_until_block);
        writer.write_u64(self.sysfee);
        writer.write_u64(self.netfee);
        io::write_array(writer, &self.signers);
        io::write_array(writer, &self.attributes);
        self.script.neo_encode(writer);
    }
}

impl NeoEncode for Tx {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.encode_unsigned(writer);
        io::write_array(writer, &self.witnesses);
    }
}
