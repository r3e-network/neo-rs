use alloc::{string::String, vec::Vec};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{
    de::{Error as DeError, MapAccess, Visitor},
    Deserialize, Deserializer,
};

use crate::nef::token::MethodToken;

use super::super::model::NefFile;

pub struct NefDeserializer;

impl<'de> Deserialize<'de> for NefFile {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "lowercase")]
        enum Field {
            Compiler,
            Source,
            Tokens,
            Script,
            Checksum,
        }

        struct NefVisitor;

        impl<'de> Visitor<'de> for NefVisitor {
            type Value = NefFile;

            fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
                formatter.write_str("NEF file")
            }

            fn visit_map<M>(self, mut access: M) -> Result<NefFile, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut compiler = None;
                let mut source = None;
                let mut tokens = None;
                let mut script = None;
                let mut checksum = None;
                while let Some(field) = access.next_key::<Field>()? {
                    match field {
                        Field::Compiler => compiler = Some(access.next_value()?),
                        Field::Source => source = Some(access.next_value()?),
                        Field::Tokens => {
                            let values: Vec<serde_json::Value> = access.next_value()?;
                            let parsed = values
                                .into_iter()
                                .map(|value| {
                                    serde_json::from_value::<MethodToken>(value)
                                        .map_err(DeError::custom)
                                })
                                .collect::<Result<Vec<_>, _>>()?;
                            tokens = Some(parsed);
                        }
                        Field::Script => {
                            let encoded: String = access.next_value()?;
                            let bytes = BASE64
                                .decode(encoded.trim().as_bytes())
                                .map_err(|err| DeError::custom(err.to_string()))?;
                            script = Some(bytes);
                        }
                        Field::Checksum => {
                            let text: String = access.next_value()?;
                            let normalized = text.trim_start_matches("0x").trim_start_matches("0X");
                            let value = u32::from_str_radix(normalized, 16)
                                .map_err(|_| DeError::custom("invalid checksum hex"))?;
                            checksum = Some(value);
                        }
                    }
                }

                let nef = NefFile {
                    compiler: compiler.ok_or_else(|| DeError::missing_field("compiler"))?,
                    source: source.ok_or_else(|| DeError::missing_field("source"))?,
                    tokens: tokens.ok_or_else(|| DeError::missing_field("tokens"))?,
                    script: script.ok_or_else(|| DeError::missing_field("script"))?,
                    checksum: checksum.ok_or_else(|| DeError::missing_field("checksum"))?,
                };
                nef.validate().map_err(DeError::custom)?;
                if !nef.verify_checksum() {
                    return Err(DeError::custom("checksum mismatch"));
                }
                Ok(nef)
            }
        }

        deserializer.deserialize_struct(
            "NefFile",
            &["compiler", "source", "tokens", "script", "checksum"],
            NefVisitor,
        )
    }
}
