use std::fmt;
use crate::core::state;
use crate::io::{self, Serializable};

// MessageType represents message type.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MessageType {
    Vote = 0,
    Root = 1,
}

impl From<u8> for MessageType {
    fn from(value: u8) -> Self {
        match value {
            0 => MessageType::Vote,
            1 => MessageType::Root,
            _ => panic!("Invalid MessageType value"),
        }
    }
}

// Message represents a state-root related message.
pub struct Message {
    pub typ: MessageType,
    pub payload: Box<dyn Serializable>,
}

impl Message {
    // NewMessage creates a new message of the specified type.
    pub fn new(typ: MessageType, payload: Box<dyn Serializable>) -> Self {
        Message { typ, payload }
    }
}

impl Serializable for Message {
    fn encode_binary(&self, writer: &mut io::BinWriter) -> io::Result<()> {
        writer.write_u8(self.typ as u8)?;
        self.payload.encode_binary(writer)
    }

    fn decode_binary(&mut self, reader: &mut io::BinReader) -> io::Result<()> {
        self.typ = MessageType::from(reader.read_u8()?);
        self.payload = match self.typ {
            MessageType::Vote => Box::new(Vote::default()),
            MessageType::Root => Box::new(state::MPTRoot::default()),
        };
        self.payload.decode_binary(reader)
    }
}

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Message")
            .field("typ", &self.typ)
            .field("payload", &"<dyn Serializable>")
            .finish()
    }
}
