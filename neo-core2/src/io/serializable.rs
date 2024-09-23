// Serializable defines the binary encoding/decoding interface. Errors are
// returned via BinReader/BinWriter Err field. These functions must have safe
// behavior when the passed BinReader/BinWriter with Err is already set. Invocations
// to these functions tend to be nested, with this mechanism only the top-level
// caller should handle an error once and all the other code should just not
// panic while there is an error.

pub trait Serializable {
    fn decode_binary(&mut self, reader: &mut BinReader);
    fn encode_binary(&self, writer: &mut BinWriter);
}

pub trait Decodable {
    fn decode_binary(&mut self, reader: &mut BinReader);
}

pub trait Encodable {
    fn encode_binary(&self, writer: &mut BinWriter);
}
