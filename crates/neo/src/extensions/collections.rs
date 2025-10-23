use crate::io::{BinaryWriter, IoResult, Serializable};

/// Collection helpers matching `Neo.Extensions.Collections.ICollectionExtensions`.
pub trait CollectionExtensions<T> {
    /// Computes the size of the collection when encoded using Neo's var-size rules.
    fn get_var_size(&self) -> IoResult<usize>;

    /// Serialises the collection into a byte array using [`BinaryWriter`].
    fn to_byte_array(&self) -> IoResult<Vec<u8>>;
}

impl<T> CollectionExtensions<T> for [T]
where
    T: Serializable,
{
    fn get_var_size(&self) -> IoResult<usize> {
        let item_sizes: usize = self.iter().map(|item| item.size()).sum();
        Ok(var_size(self.len()) + item_sizes)
    }

    fn to_byte_array(&self) -> IoResult<Vec<u8>> {
        let mut writer = BinaryWriter::new();
        writer.write_var_uint(self.len() as u64)?;
        for item in self {
            item.serialize(&mut writer)?;
        }
        Ok(writer.into_bytes())
    }
}

impl<T> CollectionExtensions<T> for Vec<T>
where
    T: Serializable,
{
    fn get_var_size(&self) -> IoResult<usize> {
        self.as_slice().get_var_size()
    }

    fn to_byte_array(&self) -> IoResult<Vec<u8>> {
        self.as_slice().to_byte_array()
    }
}

fn var_size(value: usize) -> usize {
    if value < 0xFD {
        1
    } else if value <= 0xFFFF {
        3
    } else if value <= 0xFFFF_FFFF {
        5
    } else {
        9
    }
}
