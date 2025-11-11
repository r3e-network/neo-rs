use alloc::vec::Vec;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{ser::SerializeStruct, Serializer};

use crate::nef::token::MethodToken;

use super::super::model::NefFile;

impl serde::Serialize for NefFile {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("NefFile", 5)?;
        state.serialize_field("compiler", &self.compiler)?;
        state.serialize_field("source", &self.source)?;
        state.serialize_field(
            "tokens",
            &self
                .tokens
                .iter()
                .map(|token| serde_json::to_value(token).expect("token"))
                .collect::<Vec<_>>(),
        )?;
        state.serialize_field("script", &BASE64.encode(&self.script))?;
        state.serialize_field("checksum", &format!("{:#010X}", self.checksum))?;
        state.end()
    }
}
