use crate::testserdes;
use crate::network::payload::MPTData;
use std::error::Error;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testserdes;
    use crate::network::payload::MPTData;
    use std::error::Error;

    #[test]
    fn test_mptdata_encode_decode_binary() -> Result<(), Box<dyn Error>> {
        // Empty case
        {
            let d = MPTData::default();
            let bytes = testserdes::encode_binary(&d)?;
            assert!(testserdes::decode_binary(&bytes, &mut MPTData::default()).is_err());
        }

        // Good case
        {
            let d = MPTData {
                nodes: vec![vec![], vec![1], vec![1, 2, 3]],
            };
            testserdes::encode_decode_binary(&d, &MPTData::default())?;
        }

        // Exceeds MaxArraySize case
        {
            let bytes: Vec<u8> = vec![
                // The first byte represents the number 0x1.
                // It encodes the size of the outer array (the number or rows in the Nodes matrix).
                0x1,
                // This sequence of 9 bytes represents the number 0xffffffffffffffff.
                // It encodes the size of the first row in the Nodes matrix.
                // This size exceeds the maximum array size, thus the decoder should
                // return an error.
                0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
            ];
            assert!(testserdes::decode_binary(&bytes, &mut MPTData::default()).is_err());
        }

        Ok(())
    }
}
