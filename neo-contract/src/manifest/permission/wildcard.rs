use alloc::vec::Vec;
use core::fmt;

use neo_base::encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite};
use serde::{
    de::{value::SeqAccessDeserializer, Error as DeError, SeqAccess, Visitor},
    Deserialize, Serialize,
};

use crate::manifest::util::{decode_vec, encode_vec};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WildcardContainer<T> {
    Wildcard,
    List(Vec<T>),
}

impl<T> WildcardContainer<T> {
    pub fn wildcard() -> Self {
        Self::Wildcard
    }

    pub fn list(items: Vec<T>) -> Self {
        Self::List(items)
    }

    pub fn is_wildcard(&self) -> bool {
        matches!(self, Self::Wildcard)
    }
}

impl<T> Default for WildcardContainer<T> {
    fn default() -> Self {
        Self::Wildcard
    }
}

impl<T> NeoEncode for WildcardContainer<T>
where
    T: NeoEncode,
{
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        match self {
            Self::Wildcard => writer.write_u8(0),
            Self::List(items) => {
                writer.write_u8(1);
                encode_vec(writer, items);
            }
        }
    }
}

impl<T> NeoDecode for WildcardContainer<T>
where
    T: NeoDecode,
{
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        Ok(match reader.read_u8()? {
            0 => Self::Wildcard,
            1 => Self::List(decode_vec(reader)?),
            _ => return Err(DecodeError::InvalidValue("WildcardContainer")),
        })
    }
}

impl<T> Serialize for WildcardContainer<T>
where
    T: Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Wildcard => serializer.serialize_str("*"),
            Self::List(items) => items.serialize(serializer),
        }
    }
}

impl<'de, T> Deserialize<'de> for WildcardContainer<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct WildVisitor<T>(core::marker::PhantomData<T>);

        impl<'de, T> Visitor<'de> for WildVisitor<T>
        where
            T: Deserialize<'de>,
        {
            type Value = WildcardContainer<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("'*' or array")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: DeError,
            {
                if v == "*" {
                    Ok(WildcardContainer::Wildcard)
                } else {
                    Err(E::custom("expected '*' for wildcard"))
                }
            }

            fn visit_seq<A>(self, seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let items = Deserialize::deserialize(SeqAccessDeserializer::new(seq))?;
                Ok(WildcardContainer::List(items))
            }
        }

        deserializer.deserialize_any(WildVisitor(core::marker::PhantomData))
    }
}
