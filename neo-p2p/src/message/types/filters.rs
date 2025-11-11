use neo_base::{
    encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite},
    Bytes,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterLoadPayload {
    pub filter: Bytes,
    pub k: u8,
    pub tweak: u32,
}

impl NeoEncode for FilterLoadPayload {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.filter.neo_encode(writer);
        self.k.neo_encode(writer);
        self.tweak.neo_encode(writer);
    }
}

impl NeoDecode for FilterLoadPayload {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let filter = Bytes::neo_decode(reader)?;
        let k = u8::neo_decode(reader)?;
        if k > 50 {
            return Err(DecodeError::InvalidValue("filterload k"));
        }
        let tweak = u32::neo_decode(reader)?;
        Ok(Self { filter, k, tweak })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterAddPayload {
    pub data: Bytes,
}

impl NeoEncode for FilterAddPayload {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.data.neo_encode(writer);
    }
}

impl NeoDecode for FilterAddPayload {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        Ok(Self {
            data: Bytes::neo_decode(reader)?,
        })
    }
}
