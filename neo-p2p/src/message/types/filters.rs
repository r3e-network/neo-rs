use neo_base::{
    encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite},
    Bytes,
};

const MAX_FILTER_BYTES: usize = 36_000;
const MAX_FILTERADD_BYTES: usize = 520;

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
        if filter.len() > MAX_FILTER_BYTES {
            return Err(DecodeError::LengthOutOfRange {
                len: filter.len() as u64,
                max: MAX_FILTER_BYTES as u64,
            });
        }
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
        let data = Bytes::neo_decode(reader)?;
        if data.len() > MAX_FILTERADD_BYTES {
            return Err(DecodeError::LengthOutOfRange {
                len: data.len() as u64,
                max: MAX_FILTERADD_BYTES as u64,
            });
        }
        Ok(Self { data })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_base::encoding::{NeoWrite, SliceReader};

    #[test]
    fn filter_load_payload_rejects_large_filter() {
        let mut bytes = Vec::new();
        let data = vec![0u8; MAX_FILTER_BYTES + 1];
        bytes.write_var_bytes(&data);
        bytes.write_u8(1);
        bytes.write_u32(0);

        let mut reader = SliceReader::new(bytes.as_slice());
        let err = FilterLoadPayload::neo_decode(&mut reader).unwrap_err();
        assert!(matches!(err, DecodeError::LengthOutOfRange { .. }));
    }

    #[test]
    fn filter_add_payload_rejects_large_data() {
        let mut bytes = Vec::new();
        let data = vec![0u8; MAX_FILTERADD_BYTES + 1];
        bytes.write_var_bytes(&data);

        let mut reader = SliceReader::new(bytes.as_slice());
        let err = FilterAddPayload::neo_decode(&mut reader).unwrap_err();
        assert!(matches!(err, DecodeError::LengthOutOfRange { .. }));
    }
}
